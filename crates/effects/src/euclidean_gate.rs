//! A Euclidean rhythm as a gate: the player proposes onsets, the grid
//! decides which of them sound.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::defer::DeferTracker;
use crate::router::push;

/// Gate note-ons through a Euclidean pulse grid after Toussaint: an
/// endless grid of period `pulse_ns`, anchored at the first note-on the
/// effect sees, where pulse `i` is open iff
/// `((i + rotation) * k) % n < k`. That is the standard Euclidean
/// construction, spreading `k` open pulses as evenly as possible over
/// every `n`: k=3, n=8 gives the tresillo, 10010010.
///
/// With `defer`, a note-on lands on the next open pulse at or after its
/// arrival (arriving inside an open pulse it sounds immediately, since
/// that pulse's start is not in the future). Without, it passes only when
/// the pulse containing it is open and otherwise drops, its note-off
/// swallowed with it.
///
/// Deferred ons follow the ordering rule: the matching off is held to at
/// least 10ms past the emitted on, a retrigger during deferral cuts the
/// pending note first, and `flush` releases whatever sounds. The same
/// tracker guards undeferred notes, so a note shorter than 10ms is
/// stretched to the minimum rather than risked out of order. Poly
/// pressure follows the sounding note and is dropped otherwise; non-note
/// events pass unchanged.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// note-on), well under `MAX_FANOUT`.
pub struct EuclideanGate {
    k: u64,
    n: u64,
    rotation: u64,
    pulse_ns: u64,
    defer: bool,
    /// Grid origin, fixed by the first note-on.
    anchor: Option<u64>,
    tracker: DeferTracker,
}

impl EuclideanGate {
    /// `n` is clamped to 1..=64, `k` to 1..=n, `rotation` wraps modulo
    /// `n`, and `pulse_ns` is raised to at least 1ms.
    pub fn new(k: u8, n: u8, rotation: u8, pulse_ns: u64, defer: bool) -> Self {
        let n = u64::from(n).clamp(1, 64);
        let k = u64::from(k).clamp(1, n);
        Self {
            k,
            n,
            rotation: u64::from(rotation) % n,
            pulse_ns: pulse_ns.max(1_000_000),
            defer,
            anchor: None,
            tracker: DeferTracker::new(),
        }
    }

    /// Whether pulse `i` is open under the Euclidean construction.
    fn open(&self, i: u64) -> bool {
        ((i + self.rotation) * self.k) % self.n < self.k
    }

    /// Where a note-on arriving at `t` sounds, or `None` when it drops.
    fn admit(&mut self, t: u64) -> Option<u64> {
        let anchor = *self.anchor.get_or_insert(t);
        let i = t.saturating_sub(anchor) / self.pulse_ns;
        if self.defer {
            // At least one of any n consecutive pulses is open (k >= 1),
            // so the search always lands; the fallback is unreachable.
            let j = (i..=i.saturating_add(self.n))
                .find(|&j| self.open(j))
                .unwrap_or(i);
            let start = anchor.saturating_add(j.saturating_mul(self.pulse_ns));
            Some(t.max(start))
        } else {
            self.open(i).then_some(t)
        }
    }
}

impl Effect for EuclideanGate {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { .. } => {
                let on_time = self.admit(ev.time);
                self.tracker.note_on(ev, on_time, out, cx);
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

    const PULSE: u64 = 1_000_000_000;

    /// Play a short note in pulse `i` and report whether it passed. The
    /// note-off of a dropped note must be swallowed with it.
    fn pulse_passes(fx: &mut EuclideanGate, i: u64) -> bool {
        let t = i * PULSE;
        let passed = match run_timed(fx, t, on(60))[..] {
            [ev] => {
                assert!(matches!(ev.kind, EventKind::NoteOn { .. }));
                true
            }
            [] => false,
            ref other => panic!("unexpected output {other:?}"),
        };
        let offs = run_timed(fx, t + PULSE / 2, off(60));
        assert_eq!(!offs.is_empty(), passed, "off must match on");
        passed
    }

    #[test]
    fn k3_n8_is_the_tresillo() {
        // ((i + 0) * 3) % 8 < 3 opens pulses 0, 3, and 6: 10010010,
        // repeating identically in the next period.
        let mut fx = EuclideanGate::new(3, 8, 0, PULSE, false);
        let pattern: Vec<bool> = (0..16).map(|i| pulse_passes(&mut fx, i)).collect();
        let tresillo = [
            true, false, false, true, false, false, true, false, //
            true, false, false, true, false, false, true, false,
        ];
        assert_eq!(pattern, tresillo);
    }

    #[test]
    fn rotation_wraps_modulo_n() {
        // rotation 8 over n=8 is rotation 0: the same tresillo.
        let mut a = EuclideanGate::new(3, 8, 8, PULSE, false);
        let mut b = EuclideanGate::new(3, 8, 0, PULSE, false);
        for i in 0..8 {
            assert_eq!(pulse_passes(&mut a, i), pulse_passes(&mut b, i), "{i}");
        }
    }

    #[test]
    fn rotation_shifts_the_pattern() {
        // rotation 1 opens ((i + 1) * 3) % 8 < 3: pulses 2, 5, and 7.
        let mut fx = EuclideanGate::new(3, 8, 1, PULSE, false);
        let pattern: Vec<bool> = (0..8).map(|i| pulse_passes(&mut fx, i)).collect();
        assert_eq!(
            pattern,
            vec![false, false, true, false, false, true, false, true]
        );
    }

    #[test]
    fn defer_lands_on_the_next_open_pulse() {
        // k=1, n=2: pulses alternate open, closed. The anchor note is in
        // pulse 0 (open) and sounds at its arrival.
        let mut fx = EuclideanGate::new(1, 2, 0, PULSE, true);
        assert_eq!(run_timed(&mut fx, 0, on(60)), vec![at(0, on(60))]);
        run_timed(&mut fx, PULSE / 2, off(60));
        // Arriving inside closed pulse 1, the on waits for pulse 2.
        assert_eq!(
            run_timed(&mut fx, 3 * PULSE / 2, on(60)),
            vec![at(2 * PULSE, on(60))]
        );
        run_timed(&mut fx, 2 * PULSE + PULSE / 4, off(60));
        // Arriving inside open pulse 4, the on sounds immediately.
        assert_eq!(
            run_timed(&mut fx, 4 * PULSE + PULSE / 3, on(60)),
            vec![at(4 * PULSE + PULSE / 3, on(60))]
        );
    }

    #[test]
    fn a_deferred_ons_early_off_waits_ten_ms_past_it() {
        let mut fx = EuclideanGate::new(1, 2, 0, PULSE, true);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 1, off(60));
        // Deferred to pulse 2; the player releases while it is pending.
        assert_eq!(
            run_timed(&mut fx, 3 * PULSE / 2, on(60)),
            vec![at(2 * PULSE, on(60))]
        );
        assert_eq!(
            run_timed(&mut fx, 3 * PULSE / 2 + 1, off(60)),
            vec![at(2 * PULSE + 10_000_000, off(60))]
        );
    }

    #[test]
    fn retrigger_during_deferral_cuts_first() {
        let mut fx = EuclideanGate::new(1, 2, 0, PULSE, true);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 1, off(60));
        // The first on is pending at 2s when the key strikes again: the
        // cut lands 10ms past the pending on and the new on never
        // precedes its own cut.
        run_timed(&mut fx, 3 * PULSE / 2, on(60));
        assert_eq!(
            run_timed(&mut fx, 3 * PULSE / 2 + 1, on(60)),
            vec![
                at(2 * PULSE + 10_000_000, off(60)),
                at(2 * PULSE + 10_000_000, on(60)),
            ]
        );
    }

    #[test]
    fn drop_mode_swallows_the_off_and_orphans() {
        let mut fx = EuclideanGate::new(1, 2, 0, PULSE, false);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 1_000, off(60));
        // Pulse 1 is closed: the on drops and so does its off.
        assert_eq!(run_timed(&mut fx, PULSE + 1, on(60)), vec![]);
        assert_eq!(run_timed(&mut fx, PULSE + 2, off(60)), vec![]);
        // An orphan off is swallowed the same way.
        assert_eq!(run_timed(&mut fx, PULSE + 3, off(72)), vec![]);
    }

    #[test]
    fn constructor_clamps_degenerate_parameters() {
        // k=0, n=0 clamp to k=1, n=1: every pulse open; pulse_ns 0 rises
        // to 1ms, and everything passes at its arrival.
        let mut fx = EuclideanGate::new(0, 0, 5, 0, false);
        for i in 0..4 {
            assert!(pulse_passes(&mut fx, i), "pulse {i}");
        }
        // k past n clamps to k=n: also everything open.
        let mut fx = EuclideanGate::new(9, 4, 0, PULSE, false);
        for i in 0..4 {
            assert!(pulse_passes(&mut fx, i), "pulse {i}");
        }
    }

    #[test]
    fn poly_pressure_follows_the_sounding_note() {
        let mut fx = EuclideanGate::new(1, 2, 0, PULSE, true);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 1, off(60));
        run_timed(&mut fx, 3 * PULSE / 2, on(60));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        // Pressure while the on is pending waits for it.
        assert_eq!(
            run_timed(&mut fx, 3 * PULSE / 2 + 1, pressure),
            vec![at(2 * PULSE, pressure)]
        );
        // Pressure for a silent key is dropped.
        let orphan = EventKind::PolyPressure {
            ch: 0,
            key: 72,
            value: 33,
        };
        assert_eq!(run_timed(&mut fx, 3 * PULSE / 2 + 2, orphan), vec![]);
    }

    #[test]
    fn flush_releases_past_the_pending_on() {
        let mut fx = EuclideanGate::new(1, 2, 0, PULSE, true);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 1, off(60));
        run_timed(&mut fx, 3 * PULSE / 2, on(60));
        let cx = ProcCx::at(3 * PULSE / 2 + 2);
        let mut out = EventBuf::new();
        fx.flush(&mut out, &cx);
        assert_eq!(
            out.as_slice(),
            &[at(2 * PULSE + 10_000_000, off(60))],
            "flush must not orphan the pending on"
        );
    }

    #[test]
    fn non_note_events_pass_unchanged() {
        let mut fx = EuclideanGate::new(3, 8, 0, PULSE, false);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 7, pedal), vec![at(7, pedal)]);
    }
}
