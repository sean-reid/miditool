//! Boundary note tracking: what is currently sounding at the output.
//!
//! The engine observes every event it sends to the destination. On
//! shutdown, panic, or a killed scene it can then emit exactly the
//! note-offs (and pedal releases) needed to silence the DAW without
//! clobbering unrelated state.

use crate::Event;
use crate::event::{CC_SUSTAIN, EventKind};
use crate::graph::EventBuf;
use crate::notemap::PerNote;

#[derive(Debug, Default)]
pub struct NoteTracker {
    /// How many note-ons are sounding per (channel, key). Counted, because
    /// forks can legitimately double a note on some synths.
    sounding: PerNote<u8>,
    /// Channels with sustain currently down at the output.
    sustain_down: u16,
}

impl NoteTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event that was sent to the output.
    pub fn observe(&mut self, kind: &EventKind) {
        match *kind {
            EventKind::NoteOn { ch, key, .. } => {
                let n = self.sounding.get(ch, key);
                self.sounding.set(ch, key, n.saturating_add(1));
            }
            EventKind::NoteOff { ch, key, .. } => {
                let n = self.sounding.get(ch, key);
                self.sounding.set(ch, key, n.saturating_sub(1));
            }
            EventKind::ControlChange {
                ch,
                cc: CC_SUSTAIN,
                value,
            } => {
                if value >= 64 {
                    self.sustain_down |= 1 << ch;
                } else {
                    self.sustain_down &= !(1 << ch);
                }
            }
            _ => {}
        }
    }

    /// Number of distinct (channel, key) slots currently sounding.
    pub fn active(&self) -> usize {
        let mut n = 0;
        self.sounding.for_each(|_, _, count| {
            if count > 0 {
                n += 1;
            }
        });
        n
    }

    /// Append the events that silence everything this tracker has seen:
    /// a note-off per sounding note plus sustain-up for held pedals,
    /// clearing each slot as it is emitted. A full buffer stops the walk
    /// with the remaining slots intact, so callers loop until a call
    /// leaves the buffer with room to spare, at which point nothing was
    /// left unsaid.
    pub fn silence(&mut self, time: u64, out: &mut EventBuf) {
        for ch in 0..16u8 {
            for key in 0..128u8 {
                if self.sounding.get(ch, key) == 0 {
                    continue;
                }
                if out.is_full() {
                    return;
                }
                out.push(Event::new(time, EventKind::NoteOff { ch, key, vel: 0 }));
                self.sounding.set(ch, key, 0);
            }
        }
        for ch in 0..16u8 {
            if self.sustain_down & (1 << ch) == 0 {
                continue;
            }
            if out.is_full() {
                return;
            }
            out.push(Event::new(
                time,
                EventKind::ControlChange {
                    ch,
                    cc: CC_SUSTAIN,
                    value: 0,
                },
            ));
            self.sustain_down &= !(1 << ch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_emits_note_offs_and_pedal_up() {
        let mut t = NoteTracker::new();
        t.observe(&EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 100,
        });
        t.observe(&EventKind::NoteOn {
            ch: 1,
            key: 72,
            vel: 100,
        });
        t.observe(&EventKind::NoteOn {
            ch: 0,
            key: 62,
            vel: 100,
        });
        t.observe(&EventKind::NoteOff {
            ch: 0,
            key: 62,
            vel: 0,
        });
        t.observe(&EventKind::ControlChange {
            ch: 0,
            cc: CC_SUSTAIN,
            value: 127,
        });
        assert_eq!(t.active(), 2);

        let mut out = EventBuf::new();
        t.silence(0, &mut out);
        let kinds: Vec<_> = out.iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EventKind::NoteOff {
            ch: 0,
            key: 60,
            vel: 0
        }));
        assert!(kinds.contains(&EventKind::NoteOff {
            ch: 1,
            key: 72,
            vel: 0
        }));
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, EventKind::NoteOff { key: 62, .. }))
        );
        assert!(kinds.contains(&EventKind::ControlChange {
            ch: 0,
            cc: CC_SUSTAIN,
            value: 0
        }));
        assert_eq!(t.active(), 0);
    }

    #[test]
    fn silence_past_a_full_buffer_keeps_the_rest_for_the_next_call() {
        let mut t = NoteTracker::new();
        for ch in 0..2u8 {
            for key in 0..75u8 {
                t.observe(&EventKind::NoteOn { ch, key, vel: 100 });
            }
        }
        t.observe(&EventKind::ControlChange {
            ch: 1,
            cc: CC_SUSTAIN,
            value: 127,
        });
        assert_eq!(t.active(), 150);

        // One EventBuf holds 128 events; the 22 notes and the pedal that
        // did not fit must survive for a second call, not be forgotten.
        let mut out = EventBuf::new();
        t.silence(0, &mut out);
        assert_eq!(out.len(), 128);
        assert_eq!(t.active(), 22);

        let mut out = EventBuf::new();
        t.silence(0, &mut out);
        assert_eq!(out.len(), 23);
        assert_eq!(t.active(), 0);
        assert_eq!(
            out.last().map(|e| e.kind),
            Some(EventKind::ControlChange {
                ch: 1,
                cc: CC_SUSTAIN,
                value: 0
            })
        );
        assert!(!out.is_full(), "a spare slot signals the walk finished");
    }

    #[test]
    fn balanced_notes_leave_nothing() {
        let mut t = NoteTracker::new();
        t.observe(&EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 100,
        });
        t.observe(&EventKind::NoteOff {
            ch: 0,
            key: 60,
            vel: 0,
        });
        let mut out = EventBuf::new();
        t.silence(0, &mut out);
        assert!(out.is_empty());
    }
}
