//! A seeded scramble of the keyboard, fixed at construction.

use miditool_core::rng::seeded;
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::{NoteRouter, push};

/// Which keys may trade places when the shuffle is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleMode {
    /// Any key in the range may land on any other.
    Free,
    /// Keys trade places only within their octave (same key / 12).
    WithinOctave,
    /// Keys trade places only with keys of the same pitch class (key % 12).
    WithinPitchClass,
}

/// Remap keys in lo..=hi through a seeded permutation drawn once at
/// construction: the keyboard is scrambled, but each key keeps its
/// assignment for the life of the effect. Keys outside the range pass
/// unchanged.
pub struct ShuffleLock {
    map: [u8; 128],
    router: NoteRouter,
}

impl ShuffleLock {
    pub fn new(seed: u64, lo: u8, hi: u8, mode: ShuffleMode) -> Self {
        let hi = hi.min(127);
        let mut map: [u8; 128] = std::array::from_fn(|key| key as u8);
        let mut rng = seeded(seed, 0);
        let class = |key: u8| match mode {
            ShuffleMode::Free => 0,
            ShuffleMode::WithinOctave => key / 12,
            ShuffleMode::WithinPitchClass => key % 12,
        };
        // Partition the range into groups that may trade places and
        // Fisher-Yates each group in ascending class order, so the
        // permutation depends only on the seed and the config.
        for group in 0..12u8 {
            let keys: Vec<u8> = (lo..=hi).filter(|&key| class(key) == group).collect();
            let mut vals = keys.clone();
            for i in (1..vals.len()).rev() {
                let j = rng.random_range(0..=i);
                vals.swap(i, j);
            }
            for (&key, &val) in keys.iter().zip(&vals) {
                map[key as usize] = val;
            }
        }
        Self {
            map,
            router: NoteRouter::new(),
        }
    }

    fn map(&self, key: u8) -> u8 {
        self.map[key as usize & 127]
    }
}

impl Effect for ShuffleLock {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                self.router.note_on(ev, Some(self.map(key)), out, cx);
            }
            EventKind::NoteOff { key, .. } => {
                self.router.note_off(ev, Some(self.map(key)), out, cx);
            }
            EventKind::PolyPressure { key, .. } => {
                self.router.poly_pressure(ev, Some(self.map(key)), out, cx);
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

    /// Probe where a key lands, releasing it again to keep the router clean.
    fn mapped(fx: &mut ShuffleLock, key: u8) -> u8 {
        let out = run(fx, on(key));
        let EventKind::NoteOn { key: key_out, .. } = out[0] else {
            panic!("expected a note-on, got {out:?}");
        };
        run(fx, off(key));
        key_out
    }

    fn image(fx: &mut ShuffleLock, lo: u8, hi: u8) -> Vec<u8> {
        (lo..=hi).map(|key| mapped(fx, key)).collect()
    }

    #[test]
    fn same_seed_same_output() {
        let mut a = ShuffleLock::new(42, 36, 84, ShuffleMode::Free);
        let mut b = ShuffleLock::new(42, 36, 84, ShuffleMode::Free);
        for key in [60, 61, 40, 84, 36, 60] {
            assert_eq!(run(&mut a, on(key)), run(&mut b, on(key)));
            assert_eq!(run(&mut a, off(key)), run(&mut b, off(key)));
        }
    }

    #[test]
    fn free_mode_is_a_permutation_of_the_range() {
        let mut fx = ShuffleLock::new(7, 36, 84, ShuffleMode::Free);
        let mut keys = image(&mut fx, 36, 84);
        keys.sort_unstable();
        assert_eq!(keys, (36..=84).collect::<Vec<_>>());
    }

    #[test]
    fn within_octave_preserves_octave() {
        let mut fx = ShuffleLock::new(7, 24, 96, ShuffleMode::WithinOctave);
        let mut keys = image(&mut fx, 24, 96);
        for (key, &key_out) in (24..=96).zip(&keys) {
            assert_eq!(key_out / 12, key / 12);
        }
        keys.sort_unstable();
        assert_eq!(keys, (24..=96).collect::<Vec<_>>());
    }

    #[test]
    fn within_pitch_class_preserves_pitch_class() {
        let mut fx = ShuffleLock::new(7, 24, 96, ShuffleMode::WithinPitchClass);
        let mut keys = image(&mut fx, 24, 96);
        for (key, &key_out) in (24..=96).zip(&keys) {
            assert_eq!(key_out % 12, key % 12);
        }
        keys.sort_unstable();
        assert_eq!(keys, (24..=96).collect::<Vec<_>>());
    }

    #[test]
    fn keys_outside_range_pass_unchanged() {
        let mut fx = ShuffleLock::new(7, 36, 84, ShuffleMode::Free);
        assert_eq!(mapped(&mut fx, 20), 20);
        assert_eq!(mapped(&mut fx, 100), 100);
    }

    #[test]
    fn note_off_follows_the_shuffled_key() {
        let mut fx = ShuffleLock::new(7, 36, 84, ShuffleMode::Free);
        let key_out = mapped(&mut fx, 60);
        assert_eq!(run(&mut fx, on(60)), vec![on(key_out)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(key_out)]);
    }
}
