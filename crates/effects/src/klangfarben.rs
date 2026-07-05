//! Klangfarbenmelodie: the line dealt out across instruments.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};
use rand::Rng;

use crate::router::push;

/// Deal successive note-ons across output channels, one instrument per
/// note, so a single line becomes a Klangfarbenmelodie in the manner of
/// Webern's orchestration of the Bach ricercar. The dealing cycles through
/// `channels` in order, or draws seeded-random per note with `random`.
///
/// Keys are never rewritten, only channels, so `NoteRouter` (which maps
/// keys) does not fit; a per-note map of the dealt channel gives the same
/// guarantees directly: the matching note-off, the retrigger cut, and poly
/// pressure all follow the note to its dealt channel, and `flush` silences
/// every dealt note. Note-offs and poly pressure with nothing sounding are
/// dropped, since the deal that would place them never happened. Non-note
/// events pass unchanged on their original channel.
pub struct Klangfarben {
    /// The output channels dealt to, fixed at construction.
    channels: Vec<u8>,
    random: bool,
    /// Next cycling position; unused when `random`.
    next: usize,
    rng: Prng,
    /// The dealt output channel per active input (channel, key).
    active: PerNote<Option<u8>>,
}

impl Klangfarben {
    /// `channels` are 0-based output channels; each is masked to 0..=15
    /// and an empty list falls back to channel 0.
    pub fn new(channels: &[u8], random: bool, seed: u64) -> Self {
        let channels = if channels.is_empty() {
            vec![0]
        } else {
            channels.iter().map(|&ch| ch & 15).collect()
        };
        Self {
            channels,
            random,
            next: 0,
            rng: seeded(seed, 0),
            active: PerNote::new(),
        }
    }

    fn deal(&mut self) -> u8 {
        if self.random {
            self.channels[self.rng.random_range(0..self.channels.len())]
        } else {
            let ch = self.channels[self.next];
            self.next = (self.next + 1) % self.channels.len();
            ch
        }
    }
}

impl Effect for Klangfarben {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut the note on whichever channel it was
                // dealt to before dealing again.
                if let Some(prev) = self.active.take(ch, key) {
                    let cut = EventKind::NoteOff {
                        ch: prev,
                        key,
                        vel: 0,
                    };
                    push(out, cx, Event::new(ev.time, cut));
                }
                let ch_out = self.deal();
                let kind = EventKind::NoteOn {
                    ch: ch_out,
                    key,
                    vel,
                };
                push(out, cx, Event::new(ev.time, kind));
                self.active.set(ch, key, Some(ch_out));
            }
            EventKind::NoteOff { ch, key, vel } => {
                if let Some(ch_out) = self.active.take(ch, key) {
                    let kind = EventKind::NoteOff {
                        ch: ch_out,
                        key,
                        vel,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
            }
            EventKind::PolyPressure { ch, key, value } => {
                if let Some(ch_out) = self.active.get(ch, key) {
                    let kind = EventKind::PolyPressure {
                        ch: ch_out,
                        key,
                        value,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        let active = std::mem::take(&mut self.active);
        active.for_each(|_ch, key, dealt| {
            if let Some(ch_out) = dealt {
                let kind = EventKind::NoteOff {
                    ch: ch_out,
                    key,
                    vel: 0,
                };
                push(out, cx, Event::new(cx.now, kind));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flush, off, on, run};

    fn on_ch(ch: u8, key: u8) -> EventKind {
        EventKind::NoteOn { ch, key, vel: 100 }
    }

    fn off_ch(ch: u8, key: u8) -> EventKind {
        EventKind::NoteOff { ch, key, vel: 0 }
    }

    #[test]
    fn cycling_deals_in_order_and_wraps() {
        let mut fx = Klangfarben::new(&[2, 5, 9], false, 1);
        assert_eq!(run(&mut fx, on(60)), vec![on_ch(2, 60)]);
        assert_eq!(run(&mut fx, on(62)), vec![on_ch(5, 62)]);
        assert_eq!(run(&mut fx, on(64)), vec![on_ch(9, 64)]);
        assert_eq!(run(&mut fx, on(65)), vec![on_ch(2, 65)]);
    }

    #[test]
    fn the_note_off_follows_the_dealt_channel() {
        let mut fx = Klangfarben::new(&[2, 5], false, 1);
        assert_eq!(run(&mut fx, on(60)), vec![on_ch(2, 60)]);
        assert_eq!(run(&mut fx, on(62)), vec![on_ch(5, 62)]);
        // Released in the opposite order: each off finds its own channel.
        assert_eq!(run(&mut fx, off(62)), vec![off_ch(5, 62)]);
        assert_eq!(run(&mut fx, off(60)), vec![off_ch(2, 60)]);
    }

    #[test]
    fn poly_pressure_follows_the_dealt_channel() {
        let mut fx = Klangfarben::new(&[7], false, 1);
        run(&mut fx, on(60));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        assert_eq!(
            run(&mut fx, pressure),
            vec![EventKind::PolyPressure {
                ch: 7,
                key: 60,
                value: 33
            }]
        );
    }

    #[test]
    fn retrigger_cuts_on_the_previous_channel() {
        let mut fx = Klangfarben::new(&[2, 5], false, 1);
        assert_eq!(run(&mut fx, on(60)), vec![on_ch(2, 60)]);
        // The same input key strikes again: the old note ends on channel 2
        // and the new one is dealt to channel 5.
        assert_eq!(run(&mut fx, on(60)), vec![off_ch(2, 60), on_ch(5, 60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off_ch(5, 60)]);
    }

    #[test]
    fn random_dealing_is_seeded_and_stays_in_the_set() {
        let mut a = Klangfarben::new(&[1, 4, 8], true, 42);
        let mut b = Klangfarben::new(&[1, 4, 8], true, 42);
        for key in [60, 62, 64, 60, 65] {
            let out = run(&mut a, on(key));
            assert_eq!(out, run(&mut b, on(key)));
            let [EventKind::NoteOn { ch, .. }] = out[..] else {
                panic!("expected exactly one note-on, got {out:?}");
            };
            assert!([1, 4, 8].contains(&ch));
            assert_eq!(run(&mut a, off(key)), run(&mut b, off(key)));
        }
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = Klangfarben::new(&[2, 5], false, 1);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_on_the_dealt_channels() {
        let mut fx = Klangfarben::new(&[2, 5], false, 1);
        run(&mut fx, on(60));
        run(&mut fx, on(62));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off_ch(2, 60), off_ch(5, 62)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn non_note_events_pass_on_their_own_channel() {
        let mut fx = Klangfarben::new(&[5], false, 1);
        let pedal = EventKind::ControlChange {
            ch: 3,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
