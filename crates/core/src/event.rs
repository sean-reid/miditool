/// Nanoseconds on the monotonic clock.
pub type Timestamp = u64;

/// A timestamped MIDI event flowing through the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub time: Timestamp,
    pub kind: EventKind,
}

impl Event {
    pub fn new(time: Timestamp, kind: EventKind) -> Self {
        Self { time, kind }
    }
}

/// Channel voice messages. Field invariants: `ch` is 0..=15, `key`, `vel`,
/// `cc`, `value`, and `program` are 0..=127, pitch bend is -8192..=8191.
///
/// Note-on with velocity 0 never appears here: the wire decoder normalizes
/// it to `NoteOff`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    NoteOn { ch: u8, key: u8, vel: u8 },
    NoteOff { ch: u8, key: u8, vel: u8 },
    PolyPressure { ch: u8, key: u8, value: u8 },
    ControlChange { ch: u8, cc: u8, value: u8 },
    ProgramChange { ch: u8, program: u8 },
    ChannelPressure { ch: u8, value: u8 },
    PitchBend { ch: u8, value: i16 },
}

impl EventKind {
    pub fn channel(&self) -> u8 {
        match *self {
            EventKind::NoteOn { ch, .. }
            | EventKind::NoteOff { ch, .. }
            | EventKind::PolyPressure { ch, .. }
            | EventKind::ControlChange { ch, .. }
            | EventKind::ProgramChange { ch, .. }
            | EventKind::ChannelPressure { ch, .. }
            | EventKind::PitchBend { ch, .. } => ch,
        }
    }

    /// The key for note-on, note-off, and poly pressure events.
    pub fn key(&self) -> Option<u8> {
        match *self {
            EventKind::NoteOn { key, .. }
            | EventKind::NoteOff { key, .. }
            | EventKind::PolyPressure { key, .. } => Some(key),
            _ => None,
        }
    }

    pub fn is_note(&self) -> bool {
        matches!(self, EventKind::NoteOn { .. } | EventKind::NoteOff { .. })
    }
}

/// The sustain pedal controller.
pub const CC_SUSTAIN: u8 = 64;
/// All Sound Off.
pub const CC_ALL_SOUND_OFF: u8 = 120;
/// Reset All Controllers.
pub const CC_RESET_CONTROLLERS: u8 = 121;
/// All Notes Off.
pub const CC_ALL_NOTES_OFF: u8 = 123;
