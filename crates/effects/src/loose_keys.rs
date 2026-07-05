//! Re-draw every note-on's key from a distribution.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::router::{NoteRouter, push};

/// Where a note-on's output key is drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyDist {
    /// Uniform over the inclusive key range.
    Uniform { lo: u8, hi: u8 },
    /// Normal around the played key with standard deviation `sigma`
    /// semitones, rounded and clamped to 0..=127.
    Gaussian { sigma: f32 },
}

/// Draw a fresh output key for every note-on: playing the same key twice
/// gives different notes. Note-offs and poly pressure follow whatever key
/// their note-on drew, and are dropped when nothing is sounding, since
/// there is no way to know a target.
pub struct LooseKeys {
    draw: Draw,
    rng: Prng,
    router: NoteRouter,
}

/// Precompiled form of `KeyDist`, so `process` never validates or unwraps.
#[derive(Clone, Copy)]
enum Draw {
    Uniform { lo: u8, hi: u8 },
    Gaussian(Normal<f32>),
}

impl LooseKeys {
    /// Panics if a Gaussian sigma is negative or not finite.
    pub fn new(seed: u64, dist: KeyDist) -> Self {
        let draw = match dist {
            // Clamped so process() can never hit an empty or out-of-range
            // sample range.
            KeyDist::Uniform { lo, hi } => {
                let hi = hi.min(127);
                Draw::Uniform { lo: lo.min(hi), hi }
            }
            KeyDist::Gaussian { sigma } => Draw::Gaussian(
                Normal::new(0.0, sigma).expect("sigma must be finite and non-negative"),
            ),
        };
        Self {
            draw,
            rng: seeded(seed, 0),
            router: NoteRouter::new(),
        }
    }

    fn draw(&mut self, key: u8) -> u8 {
        match self.draw {
            Draw::Uniform { lo, hi } => self.rng.random_range(lo..=hi),
            Draw::Gaussian(normal) => {
                let key_out = key as f32 + normal.sample(&mut self.rng);
                key_out.round().clamp(0.0, 127.0) as u8
            }
        }
    }
}

impl Effect for LooseKeys {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                let mapped = Some(self.draw(key));
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

    fn on_key(out: &[EventKind]) -> u8 {
        match out {
            [EventKind::NoteOn { key, .. }] => *key,
            other => panic!("expected exactly one note-on, got {other:?}"),
        }
    }

    #[test]
    fn same_seed_same_output() {
        let dist = KeyDist::Uniform { lo: 40, hi: 80 };
        let mut a = LooseKeys::new(9, dist);
        let mut b = LooseKeys::new(9, dist);
        for key in [60, 61, 60, 40, 80, 61] {
            assert_eq!(run(&mut a, on(key)), run(&mut b, on(key)));
            assert_eq!(run(&mut a, off(key)), run(&mut b, off(key)));
        }
    }

    #[test]
    fn note_off_goes_to_the_drawn_key() {
        let mut fx = LooseKeys::new(1, KeyDist::Uniform { lo: 40, hi: 80 });
        let key_out = on_key(&run(&mut fx, on(60)));
        assert!((40..=80).contains(&key_out));
        assert_eq!(run(&mut fx, off(60)), vec![off(key_out)]);
    }

    #[test]
    fn retrigger_cuts_the_previous_draw() {
        let mut fx = LooseKeys::new(1, KeyDist::Uniform { lo: 0, hi: 127 });
        let first = on_key(&run(&mut fx, on(60)));
        let out = run(&mut fx, on(60));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], off(first));
        let second = match out[1] {
            EventKind::NoteOn { key, .. } => key,
            other => panic!("expected a note-on, got {other:?}"),
        };
        assert_eq!(run(&mut fx, off(60)), vec![off(second)]);
    }

    #[test]
    fn repeated_key_draws_fresh_notes() {
        let mut fx = LooseKeys::new(3, KeyDist::Uniform { lo: 0, hi: 127 });
        let draws: Vec<u8> = (0..16)
            .map(|_| {
                let key_out = on_key(&run(&mut fx, on(60)));
                run(&mut fx, off(60));
                key_out
            })
            .collect();
        assert!(draws.iter().any(|&k| k != draws[0]));
    }

    #[test]
    fn gaussian_clamps_at_the_edges() {
        let mut fx = LooseKeys::new(5, KeyDist::Gaussian { sigma: 40.0 });
        let mut lo_hits = 0;
        let mut hi_hits = 0;
        for _ in 0..200 {
            let key_out = on_key(&run(&mut fx, on(0)));
            lo_hits += (key_out == 0) as u32;
            run(&mut fx, off(0));
            let key_out = on_key(&run(&mut fx, on(127)));
            hi_hits += (key_out == 127) as u32;
            run(&mut fx, off(127));
        }
        // Roughly half the draws fall past the edge and must clamp onto it.
        assert!(lo_hits > 50, "lo clamp hits: {lo_hits}");
        assert!(hi_hits > 50, "hi clamp hits: {hi_hits}");
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = LooseKeys::new(1, KeyDist::Uniform { lo: 40, hi: 80 });
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_the_drawn_keys() {
        let mut fx = LooseKeys::new(1, KeyDist::Uniform { lo: 40, hi: 80 });
        let a = on_key(&run(&mut fx, on(60)));
        let b = on_key(&run(&mut fx, on(61)));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        let mut expected = vec![off(a), off(b)];
        expected.sort_by_key(|kind| kind.key());
        assert_eq!(released, expected);
    }
}
