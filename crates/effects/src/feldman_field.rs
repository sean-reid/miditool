//! A dynamics field where the piano can only whisper.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::push;

/// Compress every note-on into a narrow dynamic band, the way a Feldman
/// page keeps the whole instrument at a murmur: the input velocity is
/// linearly mapped from 1..=127 into `floor..=ceiling`, then jittered by
/// a seeded uniform draw in `[-jitter, +jitter]` so the field still
/// breathes, and clamped to 1..=127. The player's shaping survives in
/// miniature; the room never gets loud.
///
/// The rng advances once per note-on, so the same seed replays the same
/// field. Note-offs and everything else pass untouched.
///
/// Fanout bound: exactly one output per input.
pub struct FeldmanField {
    rng: Prng,
    floor: u8,
    ceiling: u8,
    jitter: u8,
}

impl FeldmanField {
    /// `floor` is clamped to 1..=127, `ceiling` to `floor..=127`, and
    /// `jitter` to at most 20.
    pub fn new(seed: u64, floor: u8, ceiling: u8, jitter: u8) -> Self {
        let floor = floor.clamp(1, 127);
        Self {
            rng: seeded(seed, 0),
            floor,
            ceiling: ceiling.clamp(floor, 127),
            jitter: jitter.min(20),
        }
    }

    /// Map a velocity into the band and jitter it.
    fn shape(&mut self, vel: u8) -> u8 {
        let span = f32::from(self.ceiling) - f32::from(self.floor);
        let unit = (f32::from(vel.max(1)) - 1.0) / 126.0;
        let banded = f32::from(self.floor) + span * unit;
        let jitter = i32::from(self.jitter);
        let jittered = banded.round() as i32 + self.rng.random_range(-jitter..=jitter);
        jittered.clamp(1, 127) as u8
    }
}

impl Effect for FeldmanField {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                ch,
                key,
                vel: self.shape(vel),
            },
            other => other,
        };
        push(out, cx, Event::new(ev.time, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{off, run};

    fn on_vel(fx: &mut FeldmanField, vel: u8) -> u8 {
        let out = run(
            fx,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel,
            },
        );
        match out[..] {
            [EventKind::NoteOn { vel, .. }] => vel,
            ref other => panic!("expected one note-on, got {other:?}"),
        }
    }

    #[test]
    fn the_band_edges_map_exactly_without_jitter() {
        let mut fx = FeldmanField::new(1, 10, 30, 0);
        assert_eq!(on_vel(&mut fx, 1), 10);
        assert_eq!(on_vel(&mut fx, 127), 30);
        // The midpoint of the input range lands on the midpoint of the
        // band: 1 + 63 = 64 maps to 10 + 63 / 126 * 20 = 20.
        assert_eq!(on_vel(&mut fx, 64), 20);
    }

    #[test]
    fn a_pinned_band_flattens_everything() {
        let mut fx = FeldmanField::new(1, 15, 15, 0);
        for vel in 1..=127 {
            assert_eq!(on_vel(&mut fx, vel), 15);
        }
    }

    #[test]
    fn jitter_stays_inside_its_radius_and_the_valid_range() {
        let mut fx = FeldmanField::new(7, 5, 25, 10);
        let mut seen_off_band = false;
        for vel in 1..=127 {
            let banded = 5.0 + 20.0 * (f32::from(vel) - 1.0) / 126.0;
            let v = on_vel(&mut fx, vel);
            let lo = (banded.round() as i32 - 10).max(1);
            let hi = (banded.round() as i32 + 10).min(127);
            assert!((lo..=hi).contains(&i32::from(v)), "vel {vel} -> {v}");
            seen_off_band |= i32::from(v) != banded.round() as i32;
        }
        assert!(seen_off_band, "jitter must actually move something");
    }

    #[test]
    fn jitter_never_emits_velocity_zero() {
        let mut fx = FeldmanField::new(3, 1, 1, 20);
        for _ in 0..100 {
            assert!(on_vel(&mut fx, 1) >= 1);
        }
    }

    #[test]
    fn same_seed_same_field() {
        let mut a = FeldmanField::new(42, 5, 40, 8);
        let mut b = FeldmanField::new(42, 5, 40, 8);
        for vel in [1u8, 33, 64, 100, 127] {
            assert_eq!(on_vel(&mut a, vel), on_vel(&mut b, vel));
        }
    }

    #[test]
    fn constructor_clamps_the_band_and_jitter() {
        // floor 0 rises to 1; ceiling below floor rises to floor.
        let mut fx = FeldmanField::new(1, 0, 0, 0);
        assert_eq!(on_vel(&mut fx, 127), 1);
        let mut fx = FeldmanField::new(1, 50, 10, 0);
        assert_eq!(on_vel(&mut fx, 1), 50);
        assert_eq!(on_vel(&mut fx, 127), 50);
        // Jitter caps at 20: from the center of a pinned band at 64, the
        // output can never leave 44..=84.
        let mut fx = FeldmanField::new(1, 64, 64, 200);
        for _ in 0..200 {
            let v = on_vel(&mut fx, 64);
            assert!((44..=84).contains(&v), "jitter past 20: {v}");
        }
    }

    #[test]
    fn note_offs_and_other_events_pass_untouched() {
        let mut fx = FeldmanField::new(1, 10, 30, 5);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
