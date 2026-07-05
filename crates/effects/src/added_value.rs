//! Messiaen's added values: a short value slipped into the rhythm.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};
use rand::Rng;

use crate::defer::DeferTracker;
use crate::router::push;

/// Slip added values into the rhythm after Messiaen: each note-off is
/// delayed by `unit_ns` with probability `extend_p` (the dot on the
/// note), and each note-on is deferred by `unit_ns` with probability
/// `defer_p` (the added rest before it). Both decisions are drawn at
/// note-on time, in a fixed order per note-on, so the same seed replays
/// the same rhythm no matter in what order the player releases.
///
/// Deferred ons follow the ordering rule: the matching off is held to at
/// least 10ms past the emitted on, a retrigger during deferral cuts the
/// pending note first, and `flush` releases whatever sounds. Note-offs
/// with nothing sounding are dropped. Poly pressure follows the sounding
/// note and is dropped otherwise; non-note events pass unchanged.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// note-on), well under `MAX_FANOUT`.
pub struct AddedValue {
    rng: Prng,
    unit_ns: u64,
    extend_p: f32,
    defer_p: f32,
    tracker: DeferTracker,
    /// The extend decision drawn at note-on time, per active note.
    extend: PerNote<bool>,
}

impl AddedValue {
    /// `extend_p` and `defer_p` are clamped to 0.0..=1.0 and `unit_ns` is
    /// raised to at least 1ms.
    pub fn new(seed: u64, unit_ns: u64, extend_p: f32, defer_p: f32) -> Self {
        Self {
            rng: seeded(seed, 0),
            unit_ns: unit_ns.max(1_000_000),
            extend_p: extend_p.clamp(0.0, 1.0),
            defer_p: defer_p.clamp(0.0, 1.0),
            tracker: DeferTracker::new(),
            extend: PerNote::new(),
        }
    }
}

impl Effect for AddedValue {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, .. } => {
                // Always two draws, defer then extend, so the stream
                // position depends only on how many note-ons came before.
                let defer = self.rng.random::<f32>() < self.defer_p;
                let extend = self.rng.random::<f32>() < self.extend_p;
                self.extend.set(ch, key, extend);
                let on_time = if defer {
                    ev.time.saturating_add(self.unit_ns)
                } else {
                    ev.time
                };
                self.tracker.note_on(ev, Some(on_time), out, cx);
            }
            EventKind::NoteOff { ch, key, .. } => {
                let extra = if self.extend.take(ch, key) {
                    self.unit_ns
                } else {
                    0
                };
                self.tracker.note_off(ev, extra, out, cx);
            }
            EventKind::PolyPressure { .. } => self.tracker.poly_pressure(ev, out, cx),
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.extend = PerNote::new();
        self.tracker.flush(out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    const UNIT: u64 = 100_000_000;

    #[test]
    fn defer_p_one_defers_every_on_by_the_unit() {
        let mut fx = AddedValue::new(1, UNIT, 0.0, 1.0);
        assert_eq!(
            run_timed(&mut fx, 1_000, on(60)),
            vec![at(1_000 + UNIT, on(60))]
        );
    }

    #[test]
    fn defer_p_zero_leaves_every_on_alone() {
        let mut fx = AddedValue::new(1, UNIT, 0.0, 0.0);
        assert_eq!(run_timed(&mut fx, 1_000, on(60)), vec![at(1_000, on(60))]);
    }

    #[test]
    fn extend_p_one_delays_every_off_by_the_unit() {
        let mut fx = AddedValue::new(1, UNIT, 1.0, 0.0);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(
            run_timed(&mut fx, 5 * UNIT, off(60)),
            vec![at(6 * UNIT, off(60))]
        );
    }

    #[test]
    fn extend_p_zero_leaves_every_off_alone() {
        let mut fx = AddedValue::new(1, UNIT, 0.0, 0.0);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(
            run_timed(&mut fx, 5 * UNIT, off(60)),
            vec![at(5 * UNIT, off(60))]
        );
    }

    #[test]
    fn out_of_range_probabilities_clamp_to_one() {
        let mut fx = AddedValue::new(1, UNIT, 7.0, 7.0);
        assert_eq!(run_timed(&mut fx, 0, on(60)), vec![at(UNIT, on(60))]);
        assert_eq!(
            run_timed(&mut fx, 5 * UNIT, off(60)),
            vec![at(6 * UNIT, off(60))]
        );
    }

    #[test]
    fn decisions_are_per_note_regardless_of_off_order() {
        // Both instances see the same note-ons; the offs come back in
        // opposite orders. Each note must keep the decision drawn at its
        // own note-on.
        for seed in 0..8u64 {
            let mut a = AddedValue::new(seed, UNIT, 0.5, 0.0);
            let mut b = AddedValue::new(seed, UNIT, 0.5, 0.0);
            for fx in [&mut a, &mut b] {
                run_timed(fx, 0, on(60));
                run_timed(fx, 10, on(64));
            }
            let a_60 = run_timed(&mut a, 5 * UNIT, off(60));
            let a_64 = run_timed(&mut a, 5 * UNIT + 10, off(64));
            let b_64 = run_timed(&mut b, 5 * UNIT, off(64));
            let b_60 = run_timed(&mut b, 5 * UNIT + 10, off(60));
            // The extension per key matches across the two release
            // orders, arrival offsets aside.
            assert_eq!(a_60[0].time - 5 * UNIT, b_60[0].time - (5 * UNIT + 10));
            assert_eq!(a_64[0].time - (5 * UNIT + 10), b_64[0].time - 5 * UNIT);
        }
    }

    #[test]
    fn same_seed_same_output() {
        let mut a = AddedValue::new(9, UNIT, 0.5, 0.5);
        let mut b = AddedValue::new(9, UNIT, 0.5, 0.5);
        for (i, key) in [60u8, 64, 67, 60, 64].iter().enumerate() {
            let t = i as u64 * UNIT;
            assert_eq!(
                run_timed(&mut a, t, on(*key)),
                run_timed(&mut b, t, on(*key))
            );
            assert_eq!(
                run_timed(&mut a, t + UNIT / 2, off(*key)),
                run_timed(&mut b, t + UNIT / 2, off(*key))
            );
        }
    }

    #[test]
    fn an_early_off_never_beats_the_deferred_on() {
        let mut fx = AddedValue::new(1, UNIT, 0.0, 1.0);
        assert_eq!(run_timed(&mut fx, 0, on(60)), vec![at(UNIT, on(60))]);
        // The player releases while the on is still pending.
        assert_eq!(
            run_timed(&mut fx, 1_000, off(60)),
            vec![at(UNIT + 10_000_000, off(60))]
        );
    }

    #[test]
    fn retrigger_during_deferral_cuts_first() {
        let mut fx = AddedValue::new(1, UNIT, 0.0, 1.0);
        run_timed(&mut fx, 0, on(60));
        // The cut lands 10ms past the pending on; the new on defers from
        // its own arrival and is raised to the cut when needed.
        assert_eq!(
            run_timed(&mut fx, 5_000_000, on(60)),
            vec![
                at(UNIT + 10_000_000, off(60)),
                at(UNIT + 10_000_000, on(60)),
            ]
        );
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = AddedValue::new(1, UNIT, 1.0, 1.0);
        assert_eq!(run_timed(&mut fx, 0, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_whatever_sounds() {
        let mut fx = AddedValue::new(1, UNIT, 1.0, 1.0);
        run_timed(&mut fx, 0, on(60));
        let cx = ProcCx::at(5 * UNIT);
        let mut out = EventBuf::new();
        fx.flush(&mut out, &cx);
        assert_eq!(out.as_slice(), &[at(5 * UNIT, off(60))]);
    }

    #[test]
    fn non_note_events_pass_unchanged() {
        let mut fx = AddedValue::new(1, UNIT, 1.0, 1.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 42, pedal), vec![at(42, pedal)]);
    }
}
