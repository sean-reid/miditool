//! MIDI port discovery and connections: a thin wrapper around midir.
//!
//! Everything here runs on the cold path (setup and teardown). The only
//! hot-path type is [`Output::send`], which forwards straight to the
//! backend. midir's error types carry connection state and awkward type
//! parameters, so they are stringified into [`IoError::Midir`] at the
//! boundary.

#[cfg(target_os = "macos")]
pub mod hide;

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use thiserror::Error;

/// Client name registered with the OS MIDI service.
const CLIENT_NAME: &str = "miditool";

/// Errors from port discovery and connection.
#[derive(Debug, Error)]
pub enum IoError {
    /// No port could be picked automatically.
    #[error("no suitable MIDI input port; available: {}", list(.available))]
    NoPorts { available: Vec<String> },
    /// No port name contained the requested substring.
    #[error("no MIDI port matching {wanted:?}; available: {}", list(.available))]
    NotFound {
        wanted: String,
        available: Vec<String>,
    },
    /// An underlying midir error, stringified.
    #[error("midir: {0}")]
    Midir(String),
    /// Virtual ports require a unix backend (CoreMIDI or ALSA).
    #[error(
        "virtual MIDI ports are not supported on this platform; on Windows, \
         install loopMIDI, create a port there, and use `output device=\"...\"`"
    )]
    VirtualUnsupported,
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "(none)".to_string()
    } else {
        names.join(", ")
    }
}

fn midir_err(err: impl std::fmt::Display) -> IoError {
    IoError::Midir(err.to_string())
}

/// Display names of all MIDI input ports on the system.
pub fn input_ports() -> Result<Vec<String>, IoError> {
    let midi_in = MidiInput::new(CLIENT_NAME).map_err(midir_err)?;
    let ports = midi_in.ports();
    ports
        .iter()
        .map(|p| midi_in.port_name(p).map_err(midir_err))
        .collect()
}

/// Display names of all MIDI output ports on the system.
pub fn output_ports() -> Result<Vec<String>, IoError> {
    let midi_out = MidiOutput::new(CLIENT_NAME).map_err(midir_err)?;
    let ports = midi_out.ports();
    ports
        .iter()
        .map(|p| midi_out.port_name(p).map_err(midir_err))
        .collect()
}

/// Where processed events go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    /// Create a virtual output port with this name for other applications
    /// (a DAW, a softsynth) to connect to. Unix only.
    Virtual(String),
    /// Connect to the first existing output port whose name contains this
    /// substring, case-insensitively.
    Device(String),
}

/// An open MIDI output connection. Dropping it disconnects.
pub struct Output {
    conn: MidiOutputConnection,
}

impl Output {
    /// Send one complete MIDI message (or realtime byte) to the port.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        self.conn.send(bytes).map_err(midir_err)
    }
}

/// Open the given output target.
pub fn open_output(target: &OutputTarget) -> Result<Output, IoError> {
    let midi_out = MidiOutput::new(CLIENT_NAME).map_err(midir_err)?;
    match target {
        OutputTarget::Virtual(name) => create_virtual(midi_out, name),
        OutputTarget::Device(wanted) => {
            let ports = midi_out.ports();
            let names: Vec<String> = ports
                .iter()
                .map(|p| midi_out.port_name(p).map_err(midir_err))
                .collect::<Result<_, _>>()?;
            let idx = pick_named(&names, wanted)?;
            let conn = midi_out
                .connect(&ports[idx], "miditool output")
                .map_err(midir_err)?;
            Ok(Output { conn })
        }
    }
}

#[cfg(unix)]
fn create_virtual(midi_out: MidiOutput, name: &str) -> Result<Output, IoError> {
    use midir::os::unix::VirtualOutput;
    let conn = midi_out.create_virtual(name).map_err(midir_err)?;
    Ok(Output { conn })
}

#[cfg(not(unix))]
fn create_virtual(_midi_out: MidiOutput, _name: &str) -> Result<Output, IoError> {
    Err(IoError::VirtualUnsupported)
}

/// An open MIDI input connection. Dropping it disconnects.
///
/// `T` is state owned by the callback thread; see [`open_input_with`].
/// Plain [`open_input`] uses `T = ()`.
pub struct Input<T: Send + 'static = ()> {
    conn: MidiInputConnection<T>,
}

impl<T: Send + 'static> Input<T> {
    /// Disconnect and return the state that was moved into the callback.
    pub fn close(self) -> T {
        self.conn.close().1
    }
}

/// Connect to a MIDI input port.
///
/// With `Some(name)`, picks the first port whose name contains `name`
/// case-insensitively. With `None`, picks the first port whose name does
/// not contain "miditool", so we never connect to our own virtual output
/// and feed it back into itself.
///
/// The callback receives midir's microsecond timestamp and the raw packet
/// bytes; it runs on the backend's MIDI thread.
pub fn open_input(
    name: Option<&str>,
    mut callback: impl FnMut(u64, &[u8]) + Send + 'static,
) -> Result<Input, IoError> {
    open_input_with(name, move |stamp, bytes, ()| callback(stamp, bytes), ())
}

/// Like [`open_input`], but moves `data` into the callback thread and hands
/// the callback exclusive access to it, with no locking. [`Input::close`]
/// returns the data, which is how the engine reclaims its pipeline and
/// output connection at shutdown.
pub fn open_input_with<T: Send + 'static>(
    name: Option<&str>,
    callback: impl FnMut(u64, &[u8], &mut T) + Send + 'static,
    data: T,
) -> Result<Input<T>, IoError> {
    let mut midi_in = MidiInput::new(CLIENT_NAME).map_err(midir_err)?;
    // Deliver everything: SysEx, clock, and active sense are forwarded
    // downstream, not dropped at the door.
    midi_in.ignore(Ignore::None);
    let ports = midi_in.ports();
    let names: Vec<String> = ports
        .iter()
        .map(|p| midi_in.port_name(p).map_err(midir_err))
        .collect::<Result<_, _>>()?;
    let idx = match name {
        Some(wanted) => pick_named(&names, wanted)?,
        None => pick_auto(&names)?,
    };
    let conn = midi_in
        .connect(&ports[idx], "miditool input", callback, data)
        .map_err(midir_err)?;
    Ok(Input { conn })
}

/// Index of the first name containing `wanted`, case-insensitively.
fn pick_named(names: &[String], wanted: &str) -> Result<usize, IoError> {
    let wanted_lc = wanted.to_lowercase();
    names
        .iter()
        .position(|n| n.to_lowercase().contains(&wanted_lc))
        .ok_or_else(|| IoError::NotFound {
            wanted: wanted.to_string(),
            available: names.to_vec(),
        })
}

/// Index of the first name that is not one of our own ports.
fn pick_auto(names: &[String]) -> Result<usize, IoError> {
    names
        .iter()
        .position(|n| !n.to_lowercase().contains("miditool"))
        .ok_or_else(|| IoError::NoPorts {
            available: names.to_vec(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn named_pick_is_case_insensitive_substring() {
        let names = names(&["Piano X-2", "Launchkey Mini MK3"]);
        assert_eq!(pick_named(&names, "launchkey").unwrap(), 1);
        assert_eq!(pick_named(&names, "PIANO").unwrap(), 0);
    }

    #[test]
    fn named_pick_reports_available_on_miss() {
        let err = pick_named(&names(&["Piano X-2"]), "drums").unwrap_err();
        match err {
            IoError::NotFound { wanted, available } => {
                assert_eq!(wanted, "drums");
                assert_eq!(available, vec!["Piano X-2".to_string()]);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn auto_pick_skips_our_own_ports() {
        let names = names(&["miditool out", "Piano X-2"]);
        assert_eq!(pick_auto(&names).unwrap(), 1);
    }

    #[test]
    fn auto_pick_fails_when_only_our_ports_exist() {
        assert!(matches!(
            pick_auto(&names(&["MidiTool out"])),
            Err(IoError::NoPorts { .. })
        ));
        assert!(matches!(pick_auto(&[]), Err(IoError::NoPorts { .. })));
    }
}
