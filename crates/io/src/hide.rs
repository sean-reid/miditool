//! Hiding a MIDI source from other apps (macOS only).
//!
//! CoreMIDI endpoints carry a `private` property: setting it hides the
//! endpoint from every other client's enumeration while clients that are
//! already connected keep receiving. miditool uses it to take the raw
//! keyboard away from DAWs that listen to every port (GarageBand), leaving
//! only the transformed output visible.
//!
//! The flag lives in the MIDI server, not in this process, so it survives
//! a crash. [`HiddenSource`] restores on drop, the CLI restores on Ctrl-C,
//! and `miditool unhide` recovers after a hard kill by walking the device
//! tree, which still reaches endpoints hidden from flat enumeration.

use std::ffi::c_void;

use crate::IoError;

type CFStringRef = *const c_void;
type MIDIObjectRef = u32;
type OSStatus = i32;

#[link(name = "CoreMIDI", kind = "framework")]
unsafe extern "C" {
    static kMIDIPropertyPrivate: CFStringRef;
    static kMIDIPropertyDisplayName: CFStringRef;
    fn MIDIClientCreate(
        name: CFStringRef,
        notify_proc: *const c_void,
        notify_ref: *const c_void,
        out_client: *mut MIDIObjectRef,
    ) -> OSStatus;
    fn MIDIGetNumberOfSources() -> usize;
    fn MIDIGetSource(index: usize) -> MIDIObjectRef;
    fn MIDIGetNumberOfDevices() -> usize;
    fn MIDIGetDevice(index: usize) -> MIDIObjectRef;
    fn MIDIDeviceGetNumberOfEntities(device: MIDIObjectRef) -> usize;
    fn MIDIDeviceGetEntity(device: MIDIObjectRef, index: usize) -> MIDIObjectRef;
    fn MIDIEntityGetNumberOfSources(entity: MIDIObjectRef) -> usize;
    fn MIDIEntityGetSource(entity: MIDIObjectRef, index: usize) -> MIDIObjectRef;
    fn MIDIObjectSetIntegerProperty(
        object: MIDIObjectRef,
        property: CFStringRef,
        value: i32,
    ) -> OSStatus;
    fn MIDIObjectGetStringProperty(
        object: MIDIObjectRef,
        property: CFStringRef,
        out: *mut CFStringRef,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: isize, encoding: u32) -> u8;
    fn CFRelease(cf: *const c_void);
}

const UTF8: u32 = 0x0800_0100;

/// CoreMIDI requires a client before endpoint calls behave; one per
/// process is plenty.
fn ensure_client() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let name = CFStringCreateWithBytes(
            std::ptr::null(),
            "miditool-hide".as_ptr(),
            "miditool-hide".len() as isize,
            UTF8,
            0,
        );
        let mut client: MIDIObjectRef = 0;
        MIDIClientCreate(name, std::ptr::null(), std::ptr::null(), &mut client);
        CFRelease(name);
    });
}

fn display_name(endpoint: MIDIObjectRef) -> Option<String> {
    unsafe {
        let mut cf: CFStringRef = std::ptr::null();
        if MIDIObjectGetStringProperty(endpoint, kMIDIPropertyDisplayName, &mut cf) != 0
            || cf.is_null()
        {
            return None;
        }
        let mut buf = [0u8; 256];
        let ok = CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8);
        CFRelease(cf);
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(0);
        Some(String::from_utf8_lossy(&buf[..len]).into_owned())
    }
}

fn set_private(endpoint: MIDIObjectRef, hidden: bool) -> Result<(), IoError> {
    let status =
        unsafe { MIDIObjectSetIntegerProperty(endpoint, kMIDIPropertyPrivate, hidden as i32) };
    if status == 0 {
        Ok(())
    } else {
        Err(IoError::Midir(format!(
            "setting the private property failed (OSStatus {status})"
        )))
    }
}

/// A source hidden from other apps. Restores visibility on drop; call
/// [`HiddenSource::restore`] to handle the error explicitly.
pub struct HiddenSource {
    endpoint: MIDIObjectRef,
    name: String,
    restored: bool,
}

impl HiddenSource {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn restore(mut self) -> Result<(), IoError> {
        self.restored = true;
        set_private(self.endpoint, false)
    }
}

impl Drop for HiddenSource {
    fn drop(&mut self) {
        if !self.restored {
            let _ = set_private(self.endpoint, false);
        }
    }
}

/// Hide the first source whose display name contains `name`
/// (case-insensitive). Connect to the source before hiding it: existing
/// connections keep receiving, new clients no longer see it.
pub fn hide_source(name: &str) -> Result<HiddenSource, IoError> {
    ensure_client();
    let wanted = name.to_lowercase();
    let mut available = Vec::new();
    unsafe {
        for i in 0..MIDIGetNumberOfSources() {
            let endpoint = MIDIGetSource(i);
            let Some(display) = display_name(endpoint) else {
                continue;
            };
            if display.to_lowercase().contains(&wanted) {
                set_private(endpoint, true)?;
                return Ok(HiddenSource {
                    endpoint,
                    name: display,
                    restored: false,
                });
            }
            available.push(display);
        }
    }
    Err(IoError::NotFound {
        wanted: name.to_string(),
        available,
    })
}

/// Display names of sources that are present in the device tree but
/// absent from flat enumeration. A hidden (private) source disappears
/// from flat enumeration, but so does an offline device, and the two
/// look the same from here; callers should present the result as
/// "possibly hidden" and point at [`unhide_sources`], which clears the
/// flag either way.
pub fn hidden_sources() -> Result<Vec<String>, IoError> {
    ensure_client();
    let mut visible = Vec::new();
    let mut hidden = Vec::new();
    unsafe {
        for i in 0..MIDIGetNumberOfSources() {
            if let Some(display) = display_name(MIDIGetSource(i)) {
                visible.push(display);
            }
        }
        for d in 0..MIDIGetNumberOfDevices() {
            let device = MIDIGetDevice(d);
            for e in 0..MIDIDeviceGetNumberOfEntities(device) {
                let entity = MIDIDeviceGetEntity(device, e);
                for s in 0..MIDIEntityGetNumberOfSources(entity) {
                    let Some(display) = display_name(MIDIEntityGetSource(entity, s)) else {
                        continue;
                    };
                    if !visible.contains(&display) && !hidden.contains(&display) {
                        hidden.push(display);
                    }
                }
            }
        }
    }
    Ok(hidden)
}

/// Restore visibility of hidden sources after a crash. Walks the device
/// tree, which reaches endpoints that flat enumeration no longer shows,
/// and clears the private flag on every source whose name contains `name`
/// (or on all of them when `name` is `None`). Returns the names touched.
pub fn unhide_sources(name: Option<&str>) -> Result<Vec<String>, IoError> {
    ensure_client();
    let wanted = name.map(str::to_lowercase);
    let mut touched = Vec::new();
    unsafe {
        for d in 0..MIDIGetNumberOfDevices() {
            let device = MIDIGetDevice(d);
            for e in 0..MIDIDeviceGetNumberOfEntities(device) {
                let entity = MIDIDeviceGetEntity(device, e);
                for s in 0..MIDIEntityGetNumberOfSources(entity) {
                    let endpoint = MIDIEntityGetSource(entity, s);
                    let display = display_name(endpoint).unwrap_or_default();
                    let matches = wanted
                        .as_ref()
                        .is_none_or(|w| display.to_lowercase().contains(w));
                    if matches {
                        set_private(endpoint, false)?;
                        touched.push(display);
                    }
                }
            }
        }
    }
    Ok(touched)
}
