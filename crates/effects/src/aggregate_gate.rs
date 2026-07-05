//! Schoenberg aggregate discipline: no pitch class returns until all
//! twelve have sounded.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::{NoteRouter, push};

/// Drop any note-on whose pitch class has already sounded since the last
/// reset, enforcing the aggregate: only when all twelve classes have
/// sounded does the slate wipe clean (the note completing the aggregate
/// passes, and the count restarts from zero). A seeded draw below `leak`
/// lets a repeat through anyway, loosening the discipline from strict
/// serialism toward free chromaticism as `leak` rises. Dropped note-ons
/// take their note-offs with them through the router; orphan note-offs and
/// poly pressure are dropped, since the gate's decision for them was never
/// made.
pub struct AggregateGate {
    /// Bit per pitch class sounded since the last reset.
    sounded: u16,
    leak: f32,
    rng: Prng,
    router: NoteRouter,
}

impl AggregateGate {
    /// `leak` is clamped to 0.0..=1.0.
    pub fn new(leak: f32, seed: u64) -> Self {
        Self {
            sounded: 0,
            leak: leak.clamp(0.0, 1.0),
            rng: seeded(seed, 0),
            router: NoteRouter::new(),
        }
    }

    /// Whether a note-on on `key` may sound. The rng advances only on
    /// repeats, so the leak sequence depends only on how many repeats came
    /// before.
    fn admit(&mut self, key: u8) -> bool {
        let bit = 1u16 << (key % 12);
        if self.sounded & bit != 0 {
            return self.rng.random::<f32>() < self.leak;
        }
        self.sounded |= bit;
        if self.sounded == 0x0FFF {
            self.sounded = 0;
        }
        true
    }
}

impl Effect for AggregateGate {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                let mapped = self.admit(key).then_some(key);
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

    /// Play a note-on and its note-off; true when the note sounded.
    fn play(fx: &mut AggregateGate, key: u8) -> bool {
        let sounded = match run(fx, on(key))[..] {
            [EventKind::NoteOn { .. }] => true,
            [] => false,
            ref other => panic!("unexpected output {other:?}"),
        };
        assert_eq!(!run(fx, off(key)).is_empty(), sounded, "off must match on");
        sounded
    }

    #[test]
    fn a_repeated_class_is_dropped_with_its_off() {
        let mut fx = AggregateGate::new(0.0, 1);
        assert!(play(&mut fx, 60));
        assert!(!play(&mut fx, 60));
        // The same class in another octave is just as spent.
        assert!(!play(&mut fx, 72));
        assert!(play(&mut fx, 61));
    }

    #[test]
    fn the_twelfth_class_passes_and_resets_the_slate() {
        let mut fx = AggregateGate::new(0.0, 1);
        for key in 60..71 {
            assert!(play(&mut fx, key), "key {key}");
        }
        // Eleven classes down: a repeat still drops.
        assert!(!play(&mut fx, 60));
        // The twelfth class completes the aggregate and passes.
        assert!(play(&mut fx, 71));
        // The slate is fresh: every class may sound again, and those
        // twelve complete (and reset) a second aggregate.
        for key in 60..72 {
            assert!(play(&mut fx, key), "key {key} after reset");
        }
        // Midway through the third aggregate the discipline holds again.
        assert!(play(&mut fx, 60));
        assert!(!play(&mut fx, 60));
    }

    #[test]
    fn leak_one_lets_every_repeat_through() {
        let mut fx = AggregateGate::new(1.0, 1);
        for _ in 0..20 {
            assert!(play(&mut fx, 60));
        }
    }

    #[test]
    fn leak_lets_repeats_through_at_seed_determined_moments() {
        let mut fx = AggregateGate::new(0.5, 42);
        assert!(play(&mut fx, 60));
        let leaked: Vec<bool> = (0..40).map(|_| play(&mut fx, 60)).collect();
        assert!(leaked.iter().any(|&b| b), "some repeats must leak");
        assert!(!leaked.iter().all(|&b| b), "some repeats must drop");
        // The same seed replays the exact same leak pattern.
        let mut again = AggregateGate::new(0.5, 42);
        assert!(play(&mut again, 60));
        let replay: Vec<bool> = (0..40).map(|_| play(&mut again, 60)).collect();
        assert_eq!(leaked, replay);
    }

    #[test]
    fn retrigger_of_a_leaked_repeat_cuts_first() {
        let mut fx = AggregateGate::new(1.0, 1);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, on(60)), vec![off(60), on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = AggregateGate::new(0.0, 1);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = AggregateGate::new(0.0, 1);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
