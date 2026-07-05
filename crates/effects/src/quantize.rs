//! Live quantization: onsets nudged onto a grid, forward only.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::defer::DeferTracker;
use crate::router::push;

/// Pull note-ons toward a time grid of period `grid_ns`, anchored at the
/// first note-on the effect sees. Live quantization can only delay: the
/// target is the nearest grid point to the arrival (round to nearest),
/// clamped forward, so when the nearest point is already in the past the
/// arrival stands unchanged. The emitted on-time is
/// `arrival + strength * (target - arrival)`: strength 0 leaves the
/// playing alone, 1 lands square on the grid, values between blend.
///
/// Deferred ons follow the ordering rule: the matching off is held to at
/// least 10ms past the emitted on, a retrigger during deferral cuts the
/// pending note first, and `flush` releases whatever sounds. Note-offs
/// with nothing sounding are dropped. Poly pressure follows the sounding
/// note and is dropped otherwise; non-note events pass unchanged.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// note-on), well under `MAX_FANOUT`.
pub struct Quantize {
    grid_ns: u64,
    strength: f32,
    /// Grid origin, fixed by the first note-on.
    anchor: Option<u64>,
    tracker: DeferTracker,
}

impl Quantize {
    /// `grid_ns` is raised to at least 1ms and `strength` clamped to
    /// 0.0..=1.0.
    pub fn new(grid_ns: u64, strength: f32) -> Self {
        Self {
            grid_ns: grid_ns.max(1_000_000),
            strength: strength.clamp(0.0, 1.0),
            anchor: None,
            tracker: DeferTracker::new(),
        }
    }

    /// The emitted on-time for a note-on arriving at `t`.
    fn on_time(&mut self, t: u64) -> u64 {
        let anchor = *self.anchor.get_or_insert(t);
        let rel = t.saturating_sub(anchor);
        let nearest =
            (rel.saturating_add(self.grid_ns / 2) / self.grid_ns).saturating_mul(self.grid_ns);
        if nearest <= rel {
            // The nearest grid point is at or behind the arrival; live
            // quantization cannot pull backward.
            return t;
        }
        let delta = (f64::from(self.strength) * (nearest - rel) as f64).round() as u64;
        t.saturating_add(delta)
    }
}

impl Effect for Quantize {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { .. } => {
                let on_time = self.on_time(ev.time);
                self.tracker.note_on(ev, Some(on_time), out, cx);
            }
            EventKind::NoteOff { .. } => self.tracker.note_off(ev, 0, out, cx),
            EventKind::PolyPressure { .. } => self.tracker.poly_pressure(ev, out, cx),
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.tracker.flush(out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    const GRID: u64 = 100_000_000;
    const MS: u64 = 1_000_000;

    #[test]
    fn the_anchor_note_passes_unchanged() {
        let mut fx = Quantize::new(GRID, 1.0);
        assert_eq!(run_timed(&mut fx, 7_777, on(60)), vec![at(7_777, on(60))]);
    }

    #[test]
    fn a_late_leaning_onset_rounds_forward_to_the_grid() {
        let mut fx = Quantize::new(GRID, 1.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        // 160ms is nearer 200ms than 100ms: it lands on 200ms.
        assert_eq!(
            run_timed(&mut fx, 160 * MS, on(60)),
            vec![at(200 * MS, on(60))]
        );
    }

    #[test]
    fn a_nearest_point_in_the_past_leaves_the_arrival_alone() {
        let mut fx = Quantize::new(GRID, 1.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        // 130ms rounds to 100ms, already behind it: it passes unchanged.
        assert_eq!(
            run_timed(&mut fx, 130 * MS, on(60)),
            vec![at(130 * MS, on(60))]
        );
    }

    #[test]
    fn strength_blends_toward_the_target() {
        let mut fx = Quantize::new(GRID, 0.5);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        // Halfway between 160ms and its 200ms target.
        assert_eq!(
            run_timed(&mut fx, 160 * MS, on(60)),
            vec![at(180 * MS, on(60))]
        );
    }

    #[test]
    fn strength_zero_is_identity_for_onsets() {
        let mut fx = Quantize::new(GRID, 0.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        assert_eq!(
            run_timed(&mut fx, 160 * MS, on(60)),
            vec![at(160 * MS, on(60))]
        );
        // Out-of-range strength clamps into 0..=1.
        let mut fx = Quantize::new(GRID, 7.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        assert_eq!(
            run_timed(&mut fx, 160 * MS, on(60)),
            vec![at(200 * MS, on(60))]
        );
    }

    #[test]
    fn the_off_never_beats_the_deferred_on() {
        let mut fx = Quantize::new(GRID, 1.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        // The on defers 160ms -> 200ms; the player releases at 165ms. The
        // off is held to 10ms past the emitted on.
        run_timed(&mut fx, 160 * MS, on(60));
        assert_eq!(
            run_timed(&mut fx, 165 * MS, off(60)),
            vec![at(210 * MS, off(60))]
        );
    }

    #[test]
    fn retrigger_during_deferral_cuts_first() {
        let mut fx = Quantize::new(GRID, 1.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        // Pending at 200ms when the key strikes again at 165ms: the cut
        // lands 10ms past the pending on, the new on at its own target,
        // raised to the cut so the pair stays ordered.
        run_timed(&mut fx, 160 * MS, on(60));
        assert_eq!(
            run_timed(&mut fx, 165 * MS, on(60)),
            vec![at(210 * MS, off(60)), at(210 * MS, on(60))]
        );
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = Quantize::new(GRID, 1.0);
        assert_eq!(run_timed(&mut fx, 0, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_past_the_pending_on() {
        let mut fx = Quantize::new(GRID, 1.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 50 * MS, off(60));
        run_timed(&mut fx, 160 * MS, on(60));
        let cx = ProcCx::at(165 * MS);
        let mut out = EventBuf::new();
        fx.flush(&mut out, &cx);
        assert_eq!(out.as_slice(), &[at(210 * MS, off(60))]);
    }

    #[test]
    fn non_note_events_pass_unchanged() {
        let mut fx = Quantize::new(GRID, 1.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 42, pedal), vec![at(42, pedal)]);
    }
}
