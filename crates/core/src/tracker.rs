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
    /// a note-off per sounding note plus sustain-up for held pedals.
    /// Resets the tracker.
    pub fn silence(&mut self, time: u64, out: &mut EventBuf) {
        let sounding = std::mem::take(&mut self.sounding);
        sounding.for_each(|ch, key, count| {
            if count > 0 && !out.is_full() {
                out.push(Event::new(time, EventKind::NoteOff { ch, key, vel: 0 }));
            }
        });
        let pedals = std::mem::take(&mut self.sustain_down);
        for ch in 0..16u8 {
            if pedals & (1 << ch) != 0 && !out.is_full() {
                out.push(Event::new(
                    time,
                    EventKind::ControlChange {
                        ch,
                        cc: CC_SUSTAIN,
                        value: 0,
                    },
                ));
            }
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
