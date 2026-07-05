//! Durations by lottery: the player's rhythm proposes, the draw disposes.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::push;

/// Discard the player's durations, the way a Feldman page fixes them on
/// paper or a Xenakis screen draws them from a law. Each note-on is
/// emitted together with a scheduled note-off at a drawn duration, a
/// self-contained pair via `push_pair`: exponential with mean `mean_ns`
/// (`-mean * ln(1 - u)`) or, with `uniform`, uniform in `min_ns..=max_ns`;
/// either way the draw is clamped into `min_ns..=max_ns`.
///
/// The player's real note-offs are swallowed: the drawn off already
/// balances every on, so the real one would end a note the lottery still
/// owns, and an orphan off has no drawn note to end. Poly pressure passes
/// through on the played key unchanged, since the drawn note sounds on
/// that same key while it lasts. Retriggering a held key emits a fresh
/// pair without a cut; the downstream tracker counts stacked ons and each
/// carries its own scheduled off. All other events pass untouched.
///
/// Randomness is seeded and deterministic (`rng::seeded`); the stream
/// advances once per note-on. `flush` emits nothing: every off is already
/// scheduled at note-on time.
///
/// Fanout bound: exactly 2 outputs per note-on, well under `MAX_FANOUT`.
pub struct DurationLottery {
    rng: Prng,
    mean_ns: u64,
    min_ns: u64,
    max_ns: u64,
    uniform: bool,
}

impl DurationLottery {
    /// `min_ns` is raised to at least 1 (a zero-length note would release
    /// at its own onset) and `max_ns` to at least `min_ns`.
    pub fn new(seed: u64, mean_ns: u64, min_ns: u64, max_ns: u64, uniform: bool) -> Self {
        let min_ns = min_ns.max(1);
        Self {
            rng: seeded(seed, 0),
            mean_ns,
            min_ns,
            max_ns: max_ns.max(min_ns),
            uniform,
        }
    }

    fn draw(&mut self) -> u64 {
        let drawn = if self.uniform {
            self.rng.random_range(self.min_ns..=self.max_ns)
        } else {
            let u: f64 = self.rng.random();
            (-(self.mean_ns as f64) * (1.0 - u).ln()).round() as u64
        };
        drawn.clamp(self.min_ns, self.max_ns)
    }
}

impl Effect for DurationLottery {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, .. } => {
                let duration = self.draw();
                let release = EventKind::NoteOff { ch, key, vel: 0 };
                // Pushed as a pair so truncation can never keep the on and
                // drop the off, which would leave the note stuck.
                cx.push_pair(
                    out,
                    *ev,
                    Event::new(ev.time.saturating_add(duration), release),
                );
            }
            // The drawn off balances the on; the player's off (or an
            // orphan) is swallowed.
            EventKind::NoteOff { .. } => {}
            _ => push(out, cx, *ev),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    /// The duration of the single pair an on produces.
    fn duration(fx: &mut DurationLottery, time: u64, key: u8) -> u64 {
        let out = run_timed(fx, time, on(key));
        match out[..] {
            [on_ev, off_ev] => {
                assert!(matches!(on_ev.kind, EventKind::NoteOn { .. }));
                assert!(matches!(off_ev.kind, EventKind::NoteOff { .. }));
                assert_eq!(on_ev.kind.key(), off_ev.kind.key());
                off_ev.time - on_ev.time
            }
            ref other => panic!("expected one pair, got {other:?}"),
        }
    }

    #[test]
    fn uniform_durations_stay_in_range() {
        let mut fx = DurationLottery::new(1, 0, 100, 200, true);
        let draws: Vec<u64> = (0..100).map(|i| duration(&mut fx, i, 60)).collect();
        assert!(draws.iter().all(|d| (100..=200).contains(d)));
        assert!(draws.iter().any(|&d| d != draws[0]));
    }

    #[test]
    fn a_pinned_range_is_exact() {
        let mut fx = DurationLottery::new(1, 0, 500, 500, true);
        assert_eq!(
            run_timed(&mut fx, 1_000, on(60)),
            vec![at(1_000, on(60)), at(1_500, off(60))]
        );
    }

    #[test]
    fn exponential_clamps_into_the_bounds() {
        // Mean 0 draws 0, clamped up to min.
        let mut fx = DurationLottery::new(1, 0, 100, 200, false);
        for i in 0..50 {
            assert_eq!(duration(&mut fx, i, 60), 100);
        }
        // A mean vastly past max clamps down to max.
        let mut fx = DurationLottery::new(1, u64::MAX / 2, 100, 200, false);
        for i in 0..50 {
            assert_eq!(duration(&mut fx, i, 60), 200);
        }
    }

    #[test]
    fn exponential_spreads_within_the_bounds() {
        let mut fx = DurationLottery::new(3, 1_000, 1, 1_000_000, false);
        let draws: Vec<u64> = (0..50).map(|i| duration(&mut fx, i, 60)).collect();
        assert!(draws.iter().all(|d| (1..=1_000_000).contains(d)));
        assert!(draws.iter().any(|&d| d != draws[0]));
    }

    #[test]
    fn the_players_note_off_is_swallowed() {
        let mut fx = DurationLottery::new(1, 0, 100, 200, true);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(run_timed(&mut fx, 50, off(60)), vec![]);
        // An orphan off is swallowed the same way.
        assert_eq!(run_timed(&mut fx, 60, off(72)), vec![]);
    }

    #[test]
    fn retrigger_emits_a_fresh_pair_without_a_cut() {
        let mut fx = DurationLottery::new(1, 0, 500, 500, true);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(500, off(60))]
        );
        assert_eq!(
            run_timed(&mut fx, 100, on(60)),
            vec![at(100, on(60)), at(600, off(60))]
        );
    }

    #[test]
    fn same_seed_same_output() {
        let mut a = DurationLottery::new(9, 5_000, 100, 100_000, false);
        let mut b = DurationLottery::new(9, 5_000, 100, 100_000, false);
        for (time, key) in [(0, 60), (10, 64), (20, 60)] {
            assert_eq!(
                run_timed(&mut a, time, on(key)),
                run_timed(&mut b, time, on(key))
            );
        }
    }

    #[test]
    fn a_nearly_full_buffer_never_splits_a_pair() {
        use miditool_core::MAX_FANOUT;
        use std::sync::atomic::Ordering;

        let mut fx = DurationLottery::new(1, 0, 100, 200, true);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        // One slot left: the pair must drop whole, not push a lone on.
        let filler = EventKind::PitchBend { ch: 0, value: 0 };
        for _ in 0..MAX_FANOUT - 1 {
            out.push(Event::new(0, filler));
        }
        fx.process(&Event::new(0, on(60)), &mut out, &cx);
        assert_eq!(out.len(), MAX_FANOUT - 1);
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn poly_pressure_and_other_events_pass() {
        let mut fx = DurationLottery::new(1, 0, 100, 200, true);
        run_timed(&mut fx, 0, on(60));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        assert_eq!(run_timed(&mut fx, 10, pressure), vec![at(10, pressure)]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 20, pedal), vec![at(20, pedal)]);
    }
}
