//! Probabilistic reflection about an axis, contrary motion in a wedge.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::{NoteRouter, push};

/// Reflect note-ons about `axis`: out = 2 * axis - key, so lines above the
/// axis answer below it, the contrary-motion wedge of a Bartok mirror or a
/// Webern inversion canon. With `probability` below 1 each note-on decides
/// independently (seeded) whether to mirror or pass, and the router
/// remembers per note, so the matching note-off always lands on whichever
/// side the note-on chose. A reflection outside 0..=127 drops the note-on
/// and its note-off with it. Orphan note-offs and poly pressure are
/// dropped, since the coin they depend on was never tossed.
pub struct WedgeMirror {
    axis: u8,
    probability: f32,
    rng: Prng,
    router: NoteRouter,
}

impl WedgeMirror {
    /// `axis` is clamped to 127 and `probability` to 0.0..=1.0.
    pub fn new(axis: u8, probability: f32, seed: u64) -> Self {
        Self {
            axis: axis.min(127),
            probability: probability.clamp(0.0, 1.0),
            rng: seeded(seed, 0),
            router: NoteRouter::new(),
        }
    }

    fn reflect(&self, key: u8) -> Option<u8> {
        let out = 2 * self.axis as i16 - key as i16;
        (0..=127).contains(&out).then_some(out as u8)
    }
}

impl Effect for WedgeMirror {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                // Drawn on every note-on so the stream position depends
                // only on how many notes came before, not on their keys.
                let mirror = self.rng.random::<f32>() < self.probability;
                let mapped = if mirror { self.reflect(key) } else { Some(key) };
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
    use crate::testutil::{off, on, run};

    #[test]
    fn probability_one_reflects_about_the_axis() {
        let mut fx = WedgeMirror::new(60, 1.0, 1);
        assert_eq!(run(&mut fx, on(55)), vec![on(65)]);
        assert_eq!(run(&mut fx, off(55)), vec![off(65)]);
        assert_eq!(run(&mut fx, on(72)), vec![on(48)]);
        assert_eq!(run(&mut fx, off(72)), vec![off(48)]);
        // The axis maps to itself.
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn out_of_range_reflection_drops_the_pair() {
        // 2 * 100 - 50 = 150: off the keyboard, so on and off both drop.
        let mut fx = WedgeMirror::new(100, 1.0, 1);
        assert_eq!(run(&mut fx, on(50)), vec![]);
        assert_eq!(run(&mut fx, off(50)), vec![]);
        // 2 * 20 - 60 = -20 drops the same way at the low edge.
        let mut fx = WedgeMirror::new(20, 1.0, 1);
        assert_eq!(run(&mut fx, on(60)), vec![]);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn probability_zero_passes_everything() {
        let mut fx = WedgeMirror::new(60, 0.0, 1);
        for key in [10, 60, 120] {
            assert_eq!(run(&mut fx, on(key)), vec![on(key)]);
            assert_eq!(run(&mut fx, off(key)), vec![off(key)]);
        }
    }

    #[test]
    fn the_note_off_follows_the_coin_toss() {
        // At probability 0.5 both outcomes occur; each note-off must land
        // where its own note-on went.
        let mut fx = WedgeMirror::new(66, 0.5, 7);
        let mut mirrored = 0;
        let mut passed = 0;
        for _ in 0..100 {
            let out = run(&mut fx, on(60));
            let [EventKind::NoteOn { key, .. }] = out[..] else {
                panic!("expected exactly one note-on, got {out:?}");
            };
            match key {
                72 => mirrored += 1,
                60 => passed += 1,
                other => panic!("unexpected key {other}"),
            }
            assert_eq!(run(&mut fx, off(60)), vec![off(key)]);
        }
        assert!(mirrored > 20 && passed > 20, "{mirrored} vs {passed}");
    }

    #[test]
    fn same_seed_same_output() {
        let mut a = WedgeMirror::new(60, 0.5, 42);
        let mut b = WedgeMirror::new(60, 0.5, 42);
        for key in [60, 55, 72, 55, 60, 72] {
            assert_eq!(run(&mut a, on(key)), run(&mut b, on(key)));
            assert_eq!(run(&mut a, off(key)), run(&mut b, off(key)));
        }
    }

    #[test]
    fn retrigger_cuts_the_previous_choice() {
        let mut fx = WedgeMirror::new(60, 0.5, 3);
        let first = match run(&mut fx, on(50))[..] {
            [EventKind::NoteOn { key, .. }] => key,
            ref other => panic!("expected a note-on, got {other:?}"),
        };
        let out = run(&mut fx, on(50));
        assert_eq!(out[0], off(first));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = WedgeMirror::new(60, 0.5, 1);
        assert_eq!(run(&mut fx, off(50)), vec![]);
    }
}
