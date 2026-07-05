//! Dynamics as orchestration: soft, middling, and loud each get a stage.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::push;

/// Deal each note-on to a channel by its velocity: below `low` it plays
/// on `soft_ch`, below `high` on `mid_ch`, and from `high` up on
/// `loud_ch`, so dynamics become orchestration and a crescendo walks
/// across instruments.
///
/// Keys and velocities are never rewritten, only channels, so the
/// klangfarben idiom applies: a per-note map of the dealt channel routes
/// the matching note-off, the retrigger cut, and poly pressure to
/// whichever channel the note went, and `flush` silences every dealt
/// note. Note-offs and poly pressure with nothing sounding are dropped.
/// Non-note events pass unchanged on their original channel.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// note-on), well under `MAX_FANOUT`.
pub struct VelocityRouter {
    low: u8,
    high: u8,
    soft_ch: u8,
    mid_ch: u8,
    loud_ch: u8,
    /// The dealt output channel per active input (channel, key).
    active: PerNote<Option<u8>>,
}

impl VelocityRouter {
    /// `low` is clamped to 1..=126 and `high` to `low + 1..=127`, so the
    /// bands are always in order; the channels are 0-based and masked to
    /// 0..=15.
    pub fn new(low: u8, high: u8, soft_ch: u8, mid_ch: u8, loud_ch: u8) -> Self {
        let low = low.clamp(1, 126);
        Self {
            low,
            high: high.clamp(low + 1, 127),
            soft_ch: soft_ch & 15,
            mid_ch: mid_ch & 15,
            loud_ch: loud_ch & 15,
            active: PerNote::new(),
        }
    }

    fn deal(&self, vel: u8) -> u8 {
        if vel < self.low {
            self.soft_ch
        } else if vel < self.high {
            self.mid_ch
        } else {
            self.loud_ch
        }
    }
}

impl Effect for VelocityRouter {
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
                let ch_out = self.deal(vel);
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
    use crate::testutil::{flush, off, run};

    fn on_vel(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    fn on_ch(ch: u8, key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch, key, vel }
    }

    fn off_ch(ch: u8, key: u8) -> EventKind {
        EventKind::NoteOff { ch, key, vel: 0 }
    }

    #[test]
    fn the_band_edges_deal_to_the_right_channels() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        for (vel, expect) in [(1, 2), (39, 2), (40, 5), (79, 5), (80, 9), (127, 9)] {
            assert_eq!(
                run(&mut fx, on_vel(60, vel)),
                vec![on_ch(expect, 60, vel)],
                "vel {vel}"
            );
            assert_eq!(run(&mut fx, off(60)), vec![off_ch(expect, 60)]);
        }
    }

    #[test]
    fn the_note_off_follows_the_dealt_channel() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        assert_eq!(run(&mut fx, on_vel(60, 10)), vec![on_ch(2, 60, 10)]);
        assert_eq!(run(&mut fx, on_vel(62, 100)), vec![on_ch(9, 62, 100)]);
        // Released in the opposite order: each off finds its own channel.
        assert_eq!(run(&mut fx, off(62)), vec![off_ch(9, 62)]);
        assert_eq!(run(&mut fx, off(60)), vec![off_ch(2, 60)]);
    }

    #[test]
    fn retrigger_cuts_on_the_previous_channel() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        assert_eq!(run(&mut fx, on_vel(60, 10)), vec![on_ch(2, 60, 10)]);
        // The same key strikes again, loud: the soft note ends on
        // channel 2 and the new one is dealt to channel 9.
        assert_eq!(
            run(&mut fx, on_vel(60, 120)),
            vec![off_ch(2, 60), on_ch(9, 60, 120)]
        );
        assert_eq!(run(&mut fx, off(60)), vec![off_ch(9, 60)]);
    }

    #[test]
    fn poly_pressure_follows_the_dealt_channel() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        run(&mut fx, on_vel(60, 50));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        assert_eq!(
            run(&mut fx, pressure),
            vec![EventKind::PolyPressure {
                ch: 5,
                key: 60,
                value: 33
            }]
        );
        // Pressure for a silent key is dropped.
        let orphan = EventKind::PolyPressure {
            ch: 0,
            key: 72,
            value: 33,
        };
        assert_eq!(run(&mut fx, orphan), vec![]);
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_on_the_dealt_channels() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        run(&mut fx, on_vel(60, 10));
        run(&mut fx, on_vel(62, 100));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off_ch(2, 60), off_ch(9, 62)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn inverted_thresholds_clamp_back_into_order() {
        // low 80 forces high up to 81: vel 80 is mid, 81 loud.
        let mut fx = VelocityRouter::new(80, 40, 2, 5, 9);
        assert_eq!(run(&mut fx, on_vel(60, 79)), vec![on_ch(2, 60, 79)]);
        run(&mut fx, off(60));
        assert_eq!(run(&mut fx, on_vel(60, 80)), vec![on_ch(5, 60, 80)]);
        run(&mut fx, off(60));
        assert_eq!(run(&mut fx, on_vel(60, 81)), vec![on_ch(9, 60, 81)]);
    }

    #[test]
    fn channels_mask_to_the_low_four_bits() {
        let mut fx = VelocityRouter::new(40, 80, 18, 21, 25);
        assert_eq!(run(&mut fx, on_vel(60, 10)), vec![on_ch(2, 60, 10)]);
    }

    #[test]
    fn non_note_events_pass_on_their_own_channel() {
        let mut fx = VelocityRouter::new(40, 80, 2, 5, 9);
        let pedal = EventKind::ControlChange {
            ch: 3,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
