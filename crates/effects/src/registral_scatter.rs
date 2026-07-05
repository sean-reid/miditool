//! Webern pointillism: pitch classes stay put, octaves scatter.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::{NoteRouter, push};

/// Keep every note-on's pitch class but land it in a fresh, uniformly
/// drawn octave whose key lies in lo..=hi, the way a Webern line sprays a
/// stepwise melody across the registers. Each note-on draws anew, so a
/// repeated key wanders; a pitch class with no candidate octave inside the
/// range passes unchanged. Note-offs and poly pressure follow whatever
/// octave their note-on drew, and are dropped when nothing is sounding,
/// since there is no way to know a target.
pub struct RegistralScatter {
    lo: u8,
    hi: u8,
    rng: Prng,
    router: NoteRouter,
}

impl RegistralScatter {
    /// `hi` is clamped to 127 and `lo` to at most `hi`.
    pub fn new(seed: u64, lo: u8, hi: u8) -> Self {
        let hi = hi.min(127);
        Self {
            lo: lo.min(hi),
            hi,
            rng: seeded(seed, 0),
            router: NoteRouter::new(),
        }
    }

    fn draw(&mut self, key: u8) -> u8 {
        let pc = key % 12;
        // The lowest key at or above `lo` with this pitch class; the
        // candidates then step up by octaves while they stay under `hi`.
        let first = self.lo + (pc + 12 - self.lo % 12) % 12;
        if first > self.hi {
            return key;
        }
        let count = (self.hi - first) / 12 + 1;
        first + 12 * self.rng.random_range(0..count)
    }
}

impl Effect for RegistralScatter {
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
    use crate::testutil::{off, on, run};

    fn on_key(out: &[EventKind]) -> u8 {
        match out {
            [EventKind::NoteOn { key, .. }] => *key,
            other => panic!("expected exactly one note-on, got {other:?}"),
        }
    }

    #[test]
    fn keeps_the_pitch_class_and_stays_in_range() {
        let mut fx = RegistralScatter::new(1, 24, 96);
        for _ in 0..100 {
            let key_out = on_key(&run(&mut fx, on(61)));
            assert_eq!(key_out % 12, 1);
            assert!((24..=96).contains(&key_out));
            run(&mut fx, off(61));
        }
    }

    #[test]
    fn draws_cover_every_candidate_octave() {
        let mut fx = RegistralScatter::new(2, 0, 127);
        let mut seen = [false; 11];
        for _ in 0..300 {
            let key_out = on_key(&run(&mut fx, on(60)));
            seen[(key_out / 12) as usize] = true;
            run(&mut fx, off(60));
        }
        // Pitch class 0 fits octaves 0..=10; a uniform draw hits them all.
        assert!(seen.iter().all(|&s| s), "octaves seen: {seen:?}");
    }

    #[test]
    fn no_candidate_passes_unchanged() {
        // Pitch class 10 has no key in 60..=64, from inside or outside the
        // range.
        let mut fx = RegistralScatter::new(1, 60, 64);
        assert_eq!(run(&mut fx, on(70)), vec![on(70)]);
        assert_eq!(run(&mut fx, off(70)), vec![off(70)]);
        assert_eq!(run(&mut fx, on(46)), vec![on(46)]);
        assert_eq!(run(&mut fx, off(46)), vec![off(46)]);
    }

    #[test]
    fn same_seed_same_output() {
        let mut a = RegistralScatter::new(9, 24, 96);
        let mut b = RegistralScatter::new(9, 24, 96);
        for key in [60, 61, 60, 24, 96, 61] {
            assert_eq!(run(&mut a, on(key)), run(&mut b, on(key)));
            assert_eq!(run(&mut a, off(key)), run(&mut b, off(key)));
        }
    }

    #[test]
    fn note_off_follows_the_drawn_octave() {
        let mut fx = RegistralScatter::new(3, 0, 127);
        let key_out = on_key(&run(&mut fx, on(60)));
        assert_eq!(run(&mut fx, off(60)), vec![off(key_out)]);
    }

    #[test]
    fn retrigger_cuts_the_previous_draw() {
        let mut fx = RegistralScatter::new(3, 0, 127);
        let first = on_key(&run(&mut fx, on(60)));
        let out = run(&mut fx, on(60));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], off(first));
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = RegistralScatter::new(1, 0, 127);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }
}
