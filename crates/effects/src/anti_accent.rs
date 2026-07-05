//! A governor that keeps the music quiet, with one indulgence per window.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Cap note-on velocities at `level` in the manner of late Feldman,
/// except that once per rolling window of `every_ns` a single note above
/// the cap passes unmodified: the first loud candidate after the window
/// has elapsed since the last allowance (the very first loud note is
/// allowed, there being no allowance before it). Everything at or under
/// the cap passes untouched and never spends the allowance.
///
/// Keys are unchanged, so note-offs need no routing: they pass untouched,
/// as does everything else. `seed` is reserved for future selection modes
/// and draws nothing yet.
///
/// Fanout bound: exactly one output per input.
pub struct AntiAccent {
    /// Reserved for future selection modes; no draws yet.
    #[allow(dead_code)]
    seed: u64,
    level: u8,
    every_ns: u64,
    /// When the last spike was allowed through; `None` before the first.
    last_allowed: Option<u64>,
}

impl AntiAccent {
    /// `level` is clamped to 1..=127 and `every_ns` raised to at least
    /// one second.
    pub fn new(seed: u64, level: u8, every_ns: u64) -> Self {
        Self {
            seed,
            level: level.clamp(1, 127),
            every_ns: every_ns.max(1_000_000_000),
            last_allowed: None,
        }
    }

    /// The velocity a note-on at `now` comes out with.
    fn govern(&mut self, now: u64, vel: u8) -> u8 {
        if vel <= self.level {
            return vel;
        }
        let elapsed = self
            .last_allowed
            .is_none_or(|t| now.saturating_sub(t) >= self.every_ns);
        if elapsed {
            self.last_allowed = Some(now);
            vel
        } else {
            self.level
        }
    }
}

impl Effect for AntiAccent {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                ch,
                key,
                vel: self.govern(ev.time, vel),
            },
            other => other,
        };
        push(out, cx, Event::new(ev.time, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{off, run_timed};

    const S: u64 = 1_000_000_000;

    fn on_vel(fx: &mut AntiAccent, time: u64, vel: u8) -> u8 {
        let out = run_timed(
            fx,
            time,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel,
            },
        );
        match out[..] {
            [
                Event {
                    kind: EventKind::NoteOn { vel, .. },
                    ..
                },
            ] => vel,
            ref other => panic!("expected one note-on, got {other:?}"),
        }
    }

    #[test]
    fn quiet_playing_passes_untouched() {
        let mut fx = AntiAccent::new(1, 60, S);
        for (i, vel) in [1u8, 30, 60, 45].iter().enumerate() {
            assert_eq!(on_vel(&mut fx, i as u64, *vel), *vel);
        }
    }

    #[test]
    fn exactly_one_spike_passes_per_window() {
        let mut fx = AntiAccent::new(1, 60, S);
        // The first loud note is allowed; the rest of its window is
        // capped; the first candidate of the next window is allowed
        // again.
        assert_eq!(on_vel(&mut fx, 0, 100), 100);
        assert_eq!(on_vel(&mut fx, 300_000_000, 110), 60);
        assert_eq!(on_vel(&mut fx, 600_000_000, 90), 60);
        assert_eq!(on_vel(&mut fx, 900_000_000, 127), 60);
        assert_eq!(on_vel(&mut fx, S, 95), 95);
        assert_eq!(on_vel(&mut fx, S + 300_000_000, 120), 60);
        assert_eq!(on_vel(&mut fx, 2 * S, 120), 120);
    }

    #[test]
    fn the_window_rolls_from_the_allowance_not_a_clock() {
        let mut fx = AntiAccent::new(1, 60, S);
        // The first loud candidate arrives late; its window starts there.
        assert_eq!(on_vel(&mut fx, 700_000_000, 100), 100);
        assert_eq!(on_vel(&mut fx, S + 500_000_000, 100), 60);
        assert_eq!(on_vel(&mut fx, S + 700_000_000, 100), 100);
    }

    #[test]
    fn quiet_notes_never_spend_the_allowance() {
        let mut fx = AntiAccent::new(1, 60, S);
        assert_eq!(on_vel(&mut fx, 0, 40), 40);
        assert_eq!(on_vel(&mut fx, 1, 55), 55);
        // Still nothing spent: the first spike passes.
        assert_eq!(on_vel(&mut fx, 2, 127), 127);
    }

    #[test]
    fn constructor_clamps_level_and_window() {
        // level 0 rises to 1: everything above whispers.
        let mut fx = AntiAccent::new(1, 0, S);
        assert_eq!(on_vel(&mut fx, 0, 100), 100);
        assert_eq!(on_vel(&mut fx, 1, 100), 1);
        // every_ns 0 rises to one second.
        let mut fx = AntiAccent::new(1, 60, 0);
        assert_eq!(on_vel(&mut fx, 0, 100), 100);
        assert_eq!(on_vel(&mut fx, S - 1, 100), 60);
        assert_eq!(on_vel(&mut fx, S, 100), 100);
    }

    #[test]
    fn note_offs_and_other_events_pass_untouched() {
        let mut fx = AntiAccent::new(1, 60, S);
        assert_eq!(run_timed(&mut fx, 0, off(60)), vec![Event::new(0, off(60))]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 1, pedal), vec![Event::new(1, pedal)]);
    }
}
