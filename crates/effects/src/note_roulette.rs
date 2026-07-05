//! Cage roulette: every note-on gambles on pass, replace, or silence.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::{NoteRouter, push};

/// Chance operations in the spirit of Cage: each note-on tosses one coin
/// and either passes unchanged (probability `pass`), becomes a uniform
/// key in `lo..=hi` (probability `replace`), or falls silent (the
/// remainder), dropping its note-off with it. The router remembers the
/// outcome per input note, so the matching note-off, a retrigger cut, and
/// `flush` land on whatever the coin chose. Orphan note-offs and poly
/// pressure are dropped, since their coin was never tossed.
///
/// Randomness is seeded and deterministic (`rng::seeded`). The outcome
/// coin advances the stream once per note-on; a replacement key costs one
/// more draw. Non-note events pass unchanged.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// new note-on), well under `MAX_FANOUT`.
pub struct NoteRoulette {
    pass: f32,
    replace: f32,
    lo: u8,
    hi: u8,
    rng: Prng,
    router: NoteRouter,
}

impl NoteRoulette {
    /// `pass` and `replace` are clamped to 0.0..=1.0 and, when their sum
    /// exceeds 1, scaled down proportionally so `pass + replace <= 1`.
    /// `hi` is clamped to 127 and `lo` to at most `hi`.
    pub fn new(seed: u64, pass: f32, replace: f32, lo: u8, hi: u8) -> Self {
        let pass = pass.clamp(0.0, 1.0);
        let replace = replace.clamp(0.0, 1.0);
        let total = pass + replace;
        let scale = if total > 1.0 { 1.0 / total } else { 1.0 };
        let hi = hi.min(127);
        Self {
            pass: pass * scale,
            replace: replace * scale,
            lo: lo.min(hi),
            hi,
            rng: seeded(seed, 0),
            router: NoteRouter::new(),
        }
    }
}

impl Effect for NoteRoulette {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                let u: f32 = self.rng.random();
                let mapped = if u < self.pass {
                    Some(key)
                } else if u < self.pass + self.replace {
                    Some(self.rng.random_range(self.lo..=self.hi))
                } else {
                    None
                };
                self.router.note_on(ev, mapped, out, cx);
            }
            EventKind::NoteOff { .. } => {
                self.router.note_off(ev, None, out, cx);
            }
            EventKind::PolyPressure { .. } => {
                self.router.poly_pressure(ev, None, out, cx);
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.router.flush(out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flush, off, on, run};

    fn on_key(out: &[EventKind]) -> Option<u8> {
        match out {
            [EventKind::NoteOn { key, .. }] => Some(*key),
            [] => None,
            other => panic!("expected at most one note-on, got {other:?}"),
        }
    }

    #[test]
    fn pass_one_is_identity() {
        let mut fx = NoteRoulette::new(1, 1.0, 0.0, 40, 80);
        for key in [0, 60, 61, 127] {
            assert_eq!(run(&mut fx, on(key)), vec![on(key)]);
            assert_eq!(run(&mut fx, off(key)), vec![off(key)]);
        }
    }

    #[test]
    fn replace_one_draws_in_range_and_the_off_follows() {
        let mut fx = NoteRoulette::new(1, 0.0, 1.0, 40, 80);
        for _ in 0..50 {
            let key_out = on_key(&run(&mut fx, on(60))).expect("replace must sound");
            assert!((40..=80).contains(&key_out));
            assert_eq!(run(&mut fx, off(60)), vec![off(key_out)]);
        }
    }

    #[test]
    fn both_zero_silences_the_note_and_its_off() {
        let mut fx = NoteRoulette::new(1, 0.0, 0.0, 0, 127);
        assert_eq!(run(&mut fx, on(60)), vec![]);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn overweight_odds_scale_proportionally() {
        // pass 3 and replace 1 scale to 0.75 and 0.25: no silence, both
        // outcomes occur.
        let mut fx = NoteRoulette::new(7, 3.0, 1.0, 90, 100);
        let mut passed = 0;
        let mut replaced = 0;
        for _ in 0..200 {
            match on_key(&run(&mut fx, on(60))) {
                Some(60) => passed += 1,
                Some(k) if (90..=100).contains(&k) => replaced += 1,
                other => panic!("unexpected outcome {other:?}"),
            }
            run(&mut fx, off(60));
        }
        assert!(passed > 100, "passed {passed}");
        assert!(replaced > 20, "replaced {replaced}");
    }

    #[test]
    fn all_three_outcomes_occur_and_replay_per_seed() {
        let outcomes = |seed: u64| -> Vec<Option<u8>> {
            let mut fx = NoteRoulette::new(seed, 0.4, 0.4, 0, 127);
            (0..100)
                .map(|_| {
                    let key_out = on_key(&run(&mut fx, on(60)));
                    run(&mut fx, off(60));
                    key_out
                })
                .collect()
        };
        let first = outcomes(11);
        assert!(first.iter().any(|o| o == &Some(60)), "some must pass");
        assert!(first.iter().any(|o| o.is_none()), "some must fall silent");
        assert!(
            first.iter().any(|o| matches!(o, Some(k) if *k != 60)),
            "some must be replaced"
        );
        assert_eq!(first, outcomes(11));
    }

    #[test]
    fn retrigger_cuts_the_previous_outcome() {
        let mut fx = NoteRoulette::new(1, 0.0, 1.0, 0, 127);
        let first = on_key(&run(&mut fx, on(60))).expect("replace must sound");
        let out = run(&mut fx, on(60));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], off(first));
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = NoteRoulette::new(1, 1.0, 0.0, 0, 127);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_the_outcomes() {
        let mut fx = NoteRoulette::new(1, 0.0, 1.0, 40, 80);
        let a = on_key(&run(&mut fx, on(60))).expect("replace must sound");
        let b = on_key(&run(&mut fx, on(61))).expect("replace must sound");
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        let mut expected = vec![off(a), off(b)];
        expected.sort_by_key(|kind| kind.key());
        assert_eq!(released, expected);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = NoteRoulette::new(1, 0.0, 0.0, 0, 127);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
