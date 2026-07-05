//! The web remote's view of a running engine.

use std::sync::Mutex;

use miditool_core::{Event, EventKind};
use miditool_engine::EngineHandle;
use miditool_remote::{Backend, MonitorEvent, Status};

use crate::pretty::note_name;

/// At most this many monitor events per drain; the ring holds 1024, the
/// UI shows 100, and the drain runs at 30 Hz, so this never starves.
const DRAIN_MAX: usize = 256;

pub struct EngineBackend {
    handle: EngineHandle,
    tap: Mutex<Option<rtrb::Consumer<Event>>>,
}

impl EngineBackend {
    pub fn new(handle: EngineHandle, tap: Option<rtrb::Consumer<Event>>) -> Self {
        Self {
            handle,
            tap: Mutex::new(tap),
        }
    }
}

impl Backend for EngineBackend {
    fn status(&self) -> Status {
        Status {
            scenes: self.handle.scenes().into_iter().map(|s| s.name).collect(),
            active: self.handle.active(),
            dropped: self.handle.dropped(),
        }
    }

    fn set_scene(&self, idx: usize) -> Result<(), String> {
        self.handle.set_scene(idx)
    }

    fn panic(&self) {
        self.handle.panic();
    }

    fn drain_events(&self) -> Vec<MonitorEvent> {
        let mut guard = self.tap.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tap) = guard.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while out.len() < DRAIN_MAX {
            match tap.pop() {
                Ok(ev) => out.push(monitor_event(&ev)),
                Err(_) => break,
            }
        }
        out
    }
}

fn monitor_event(ev: &Event) -> MonitorEvent {
    let t_ms = ev.time / 1_000_000;
    let ch = ev.kind.channel() + 1;
    let (kind, detail) = match ev.kind {
        EventKind::NoteOn { key, vel, .. } => ("note-on", format!("{} vel {vel}", note_name(key))),
        EventKind::NoteOff { key, .. } => ("note-off", note_name(key)),
        EventKind::PolyPressure { key, value, .. } => {
            ("pressure", format!("{} {value}", note_name(key)))
        }
        EventKind::ControlChange { cc, value, .. } => ("cc", format!("cc{cc} = {value}")),
        EventKind::ProgramChange { program, .. } => ("program", format!("{program}")),
        EventKind::ChannelPressure { value, .. } => ("pressure", format!("{value}")),
        EventKind::PitchBend { value, .. } => ("bend", format!("{value:+}")),
    };
    MonitorEvent {
        t_ms,
        kind: kind.to_string(),
        ch,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_humanize() {
        let ev = Event::new(
            2_500_000_000,
            EventKind::NoteOn {
                ch: 1,
                key: 60,
                vel: 96,
            },
        );
        let m = monitor_event(&ev);
        assert_eq!(m.t_ms, 2500);
        assert_eq!(m.ch, 2);
        assert_eq!(m.kind, "note-on");
        assert_eq!(m.detail, "C4 vel 96");
    }

    /// The whole stack at once: virtual keyboard, engine with two scenes,
    /// backend over the live handle, web server. Scene switching through
    /// the backend changes what the DAW-side capture hears, and the tap
    /// feeds the monitor.
    #[test]
    #[cfg(target_os = "macos")]
    fn backend_drives_a_live_engine() {
        use std::io::{Read as _, Write as _};
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};

        use miditool_core::graph::Node;
        use miditool_effects::Transpose;
        use miditool_engine::{Engine, SceneDef};
        use miditool_io::{OutputTarget, open_input, open_output};

        let mut keyboard = open_output(&OutputTarget::Virtual("miditool rl kb".into()))
            .expect("create fake keyboard");
        let scenes = vec![
            SceneDef {
                name: "plain".into(),
                kill_on_exit: false,
            },
            SceneDef {
                name: "octave up".into(),
                kill_on_exit: true,
            },
        ];
        let (engine, mut handle) = Engine::run(
            Some("miditool rl kb"),
            &OutputTarget::Virtual("miditool rl out".into()),
            scenes,
            Box::new(|idx| Ok(Node::Leaf(Box::new(Transpose::new(12 * idx as i16))))),
            None,
        )
        .expect("start engine");

        let backend = EngineBackend::new(handle.clone(), handle.take_tap());
        let server = miditool_remote::Server::start(0, Arc::new(backend)).expect("start server");

        // The server answers over real HTTP.
        let mut sock = std::net::TcpStream::connect(server.addr()).expect("connect");
        sock.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        sock.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "health check: {response}"
        );

        // A second backend view for direct assertions.
        let direct = EngineBackend::new(handle.clone(), None);
        assert_eq!(direct.status().scenes, vec!["plain", "octave up"]);

        // Capture what the DAW would hear.
        let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let sink = Arc::clone(&received);
        let _capture = open_input(Some("miditool rl out"), move |_stamp, bytes| {
            sink.lock().unwrap().push(bytes.to_vec());
        })
        .expect("open capture");
        std::thread::sleep(Duration::from_millis(300));

        let wait_for = |pred: &mut dyn FnMut() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if pred() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        };

        // Scene 0 passes through untransposed.
        keyboard.send(&[0x90, 60, 100]).unwrap();
        keyboard.send(&[0x80, 60, 0]).unwrap();
        assert!(wait_for(&mut || received.lock().unwrap().len() >= 2));
        assert_eq!(received.lock().unwrap()[0], vec![0x90, 60, 100]);

        // Switching through the backend changes the mapping.
        direct.set_scene(1).expect("switch scene");
        assert_eq!(direct.status().active, 1);
        keyboard.send(&[0x90, 60, 100]).unwrap();
        keyboard.send(&[0x80, 60, 0]).unwrap();
        assert!(wait_for(&mut || {
            received
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == &[0x90u8, 72, 100])
        }));

        drop(server);
        engine.stop().expect("stop engine");
    }
}
