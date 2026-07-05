//! Channel routing.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Rewrite the channel of every channel event to `ch` (0..=15).
#[derive(Debug, Clone, Copy)]
pub struct Channelize {
    pub ch: u8,
}

impl Effect for Channelize {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        // Masked so a bad config cannot break the 0..=15 invariant.
        let ch = self.ch & 15;
        let kind = match ev.kind {
            EventKind::NoteOn { key, vel, .. } => EventKind::NoteOn { ch, key, vel },
            EventKind::NoteOff { key, vel, .. } => EventKind::NoteOff { ch, key, vel },
            EventKind::PolyPressure { key, value, .. } => {
                EventKind::PolyPressure { ch, key, value }
            }
            EventKind::ControlChange { cc, value, .. } => {
                EventKind::ControlChange { ch, cc, value }
            }
            EventKind::ProgramChange { program, .. } => EventKind::ProgramChange { ch, program },
            EventKind::ChannelPressure { value, .. } => EventKind::ChannelPressure { ch, value },
            EventKind::PitchBend { value, .. } => EventKind::PitchBend { ch, value },
        };
        push(out, cx, Event::new(ev.time, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::run;

    #[test]
    fn rewrites_every_channel_event() {
        let mut fx = Channelize { ch: 5 };
        let cases = [
            (
                EventKind::NoteOn {
                    ch: 2,
                    key: 60,
                    vel: 100,
                },
                EventKind::NoteOn {
                    ch: 5,
                    key: 60,
                    vel: 100,
                },
            ),
            (
                EventKind::NoteOff {
                    ch: 3,
                    key: 60,
                    vel: 0,
                },
                EventKind::NoteOff {
                    ch: 5,
                    key: 60,
                    vel: 0,
                },
            ),
            (
                EventKind::PolyPressure {
                    ch: 4,
                    key: 60,
                    value: 10,
                },
                EventKind::PolyPressure {
                    ch: 5,
                    key: 60,
                    value: 10,
                },
            ),
            (
                EventKind::ControlChange {
                    ch: 6,
                    cc: 64,
                    value: 127,
                },
                EventKind::ControlChange {
                    ch: 5,
                    cc: 64,
                    value: 127,
                },
            ),
            (
                EventKind::ProgramChange { ch: 7, program: 12 },
                EventKind::ProgramChange { ch: 5, program: 12 },
            ),
            (
                EventKind::ChannelPressure { ch: 8, value: 33 },
                EventKind::ChannelPressure { ch: 5, value: 33 },
            ),
            (
                EventKind::PitchBend {
                    ch: 9,
                    value: -1000,
                },
                EventKind::PitchBend {
                    ch: 5,
                    value: -1000,
                },
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(run(&mut fx, input), vec![expected]);
        }
    }
}
