//! Burst a note into rapid repeats whose gaps follow a curve.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Re-attack every note-on `repeats` times in a rapid burst. The gaps
/// follow a geometric curve, `gap_k = first_gap_ns * curve^(k - 1)`, summed
/// cumulatively: a curve below 1 accelerates the burst, above 1 slows it
/// down, and exactly 1 keeps it even.
///
/// The original note-on passes unchanged. Repeat `k` (1..=repeats) is a
/// self-contained pair at the original velocity: a note-on at
/// `time + gap_1 + ... + gap_k` plus a matching note-off half the following
/// gap later (half its own gap for the last repeat), so each re-attack
/// releases before the next lands and nothing orphans regardless of what
/// the player does. The player's note-off ends only the original note;
/// note-offs and all other events pass through untouched. Stateless.
///
/// Fanout bound: at most `1 + 2 * repeats` outputs per input, and
/// `repeats` is clamped to 24, so 49 events, well under `MAX_FANOUT`.
pub struct Stutter {
    repeats: u8,
    first_gap_ns: u64,
    curve: f32,
}

impl Stutter {
    /// `repeats` is clamped to 1..=24, `curve` to 0.25..=4.0.
    pub fn new(repeats: u8, first_gap_ns: u64, curve: f32) -> Self {
        Self {
            repeats: repeats.clamp(1, 24),
            first_gap_ns,
            curve: curve.clamp(0.25, 4.0),
        }
    }

    /// The k-th gap (1-based), rounded to whole nanoseconds.
    fn gap(&self, k: u8) -> u64 {
        let scale = (self.curve as f64).powi(k as i32 - 1);
        (self.first_gap_ns as f64 * scale).round() as u64
    }
}

impl Effect for Stutter {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        push(out, cx, *ev);
        let EventKind::NoteOn { ch, key, vel } = ev.kind else {
            return;
        };
        let mut time = ev.time;
        for k in 1..=self.repeats {
            time = time.saturating_add(self.gap(k));
            let hold = if k == self.repeats {
                self.gap(k) / 2
            } else {
                self.gap(k + 1) / 2
            };
            let release = EventKind::NoteOff { ch, key, vel: 0 };
            // Pushed as a pair so truncation can never keep the on and
            // drop the off, which would leave the re-attack stuck.
            cx.push_pair(
                out,
                Event::new(time, EventKind::NoteOn { ch, key, vel }),
                Event::new(time.saturating_add(hold), release),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run, run_timed};

    #[test]
    fn decelerating_gaps_follow_the_curve() {
        // Gaps 100, 200, 400: repeats at 100, 300, 700; offs at half the
        // following gap (half its own for the last).
        let mut fx = Stutter::new(3, 100, 2.0);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![
                at(0, on(60)),
                at(100, on(60)),
                at(200, off(60)),
                at(300, on(60)),
                at(500, off(60)),
                at(700, on(60)),
                at(900, off(60)),
            ]
        );
    }

    #[test]
    fn even_gaps_when_curve_is_one() {
        let mut fx = Stutter::new(3, 1_000, 1.0);
        let ons: Vec<u64> = run_timed(&mut fx, 10_000, on(60))
            .iter()
            .filter(|ev| matches!(ev.kind, EventKind::NoteOn { .. }))
            .map(|ev| ev.time)
            .collect();
        assert_eq!(ons, vec![10_000, 11_000, 12_000, 13_000]);
    }

    #[test]
    fn accelerating_gaps_shrink() {
        // Gaps 1000, 500, 250: repeats at 1000, 1500, 1750; each off at
        // half the following gap, before the next re-attack.
        let mut fx = Stutter::new(3, 1_000, 0.5);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![
                at(0, on(60)),
                at(1_000, on(60)),
                at(1_250, off(60)),
                at(1_500, on(60)),
                at(1_625, off(60)),
                at(1_750, on(60)),
                at(1_875, off(60)),
            ]
        );
    }

    #[test]
    fn repeats_keep_the_original_velocity() {
        let mut fx = Stutter::new(4, 100, 1.0);
        let loud = EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 99,
        };
        for ev in run_timed(&mut fx, 0, loud) {
            if let EventKind::NoteOn { vel, .. } = ev.kind {
                assert_eq!(vel, 99);
            }
        }
    }

    #[test]
    fn repeats_and_curve_clamp() {
        let mut fx = Stutter::new(u8::MAX, 1, 1.0);
        assert_eq!(run(&mut fx, on(60)).len(), 1 + 2 * 24);
        // Curve 0.1 clamps to 0.25: the second gap is a quarter of the
        // first, not a tenth.
        let mut fx = Stutter::new(2, 1_000, 0.1);
        let out = run_timed(&mut fx, 0, on(60));
        assert_eq!(out[3].time, 1_250);
    }

    #[test]
    fn a_nearly_full_buffer_never_splits_a_pair() {
        use miditool_core::MAX_FANOUT;

        let mut fx = Stutter::new(24, 100, 1.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        // Three slots left: the pass-through on, one whole pair, and one
        // slot that must not receive a lone re-attack on.
        let filler = EventKind::PitchBend { ch: 0, value: 0 };
        for _ in 0..MAX_FANOUT - 4 {
            out.push(Event::new(0, filler));
        }
        fx.process(&Event::new(0, on(60)), &mut out, &cx);
        let net: i32 = out
            .iter()
            .map(|ev| match ev.kind {
                EventKind::NoteOn { .. } => 1,
                EventKind::NoteOff { .. } => -1,
                _ => 0,
            })
            .sum();
        assert_eq!(net, 1, "only the original on awaits the player's off");
    }

    #[test]
    fn note_offs_and_other_events_pass_through() {
        let mut fx = Stutter::new(8, 100, 1.0);
        assert_eq!(run_timed(&mut fx, 5, off(60)), vec![at(5, off(60))]);
        let bend = EventKind::PitchBend { ch: 0, value: 512 };
        assert_eq!(run_timed(&mut fx, 5, bend), vec![at(5, bend)]);
    }
}
