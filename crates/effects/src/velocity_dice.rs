//! Re-draw every note-on's velocity from a distribution.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::router::push;

/// Where a note-on's output velocity is drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VelDist {
    /// Uniform over the inclusive velocity range.
    Uniform { lo: u8, hi: u8 },
    /// Normal around the played velocity with standard deviation `sigma`,
    /// rounded and clamped to 1..=127.
    Gaussian { sigma: f32 },
}

/// Roll fresh dynamics for every note-on: the key stays put, the velocity
/// is re-drawn, decoupling loudness from touch the way chance procedures
/// decouple a parameter from the hand that plays it. Note-offs and
/// everything else pass untouched; no per-note state is needed, since the
/// key never changes. Stateless besides the rng.
///
/// Fanout bound: exactly 1 output per input.
pub struct VelocityDice {
    draw: Draw,
    rng: Prng,
}

/// Precompiled form of `VelDist`, so `process` never validates or unwraps.
#[derive(Clone, Copy)]
enum Draw {
    Uniform { lo: u8, hi: u8 },
    Gaussian(Normal<f32>),
}

impl VelocityDice {
    /// Uniform bounds are clamped into 1..=127 (velocity 0 would read as
    /// a note-off on the wire) with `lo` at most `hi`. Panics if a
    /// Gaussian sigma is negative or not finite.
    pub fn new(seed: u64, dist: VelDist) -> Self {
        let draw = match dist {
            VelDist::Uniform { lo, hi } => {
                let hi = hi.clamp(1, 127);
                Draw::Uniform {
                    lo: lo.clamp(1, hi),
                    hi,
                }
            }
            VelDist::Gaussian { sigma } => Draw::Gaussian(
                Normal::new(0.0, sigma).expect("sigma must be finite and non-negative"),
            ),
        };
        Self {
            draw,
            rng: seeded(seed, 0),
        }
    }

    fn draw(&mut self, vel: u8) -> u8 {
        match self.draw {
            Draw::Uniform { lo, hi } => self.rng.random_range(lo..=hi),
            Draw::Gaussian(normal) => {
                let vel_out = vel as f32 + normal.sample(&mut self.rng).round();
                vel_out.clamp(1.0, 127.0) as u8
            }
        }
    }
}

impl Effect for VelocityDice {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                ch,
                key,
                vel: self.draw(vel),
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

    fn on_vel(fx: &mut VelocityDice, vel: u8) -> u8 {
        let out = run(
            fx,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel,
            },
        );
        match out[..] {
            [EventKind::NoteOn { key: 60, vel, .. }] => vel,
            ref other => panic!("expected one note-on on key 60, got {other:?}"),
        }
    }

    #[test]
    fn uniform_draws_stay_in_range_and_vary() {
        let mut fx = VelocityDice::new(1, VelDist::Uniform { lo: 30, hi: 40 });
        let draws: Vec<u8> = (0..100).map(|_| on_vel(&mut fx, 100)).collect();
        assert!(draws.iter().all(|v| (30..=40).contains(v)));
        assert!(draws.iter().any(|&v| v != draws[0]));
    }

    #[test]
    fn uniform_zero_clamps_to_one() {
        // lo 0 would allow velocity 0, a note-off on the wire.
        let mut fx = VelocityDice::new(1, VelDist::Uniform { lo: 0, hi: 0 });
        for _ in 0..20 {
            assert_eq!(on_vel(&mut fx, 100), 1);
        }
    }

    #[test]
    fn gaussian_sigma_zero_is_identity() {
        let mut fx = VelocityDice::new(1, VelDist::Gaussian { sigma: 0.0 });
        for vel in [1, 64, 127] {
            assert_eq!(on_vel(&mut fx, vel), vel);
        }
    }

    #[test]
    fn gaussian_clamps_at_the_edges() {
        let mut fx = VelocityDice::new(5, VelDist::Gaussian { sigma: 40.0 });
        let mut lo_hits = 0;
        let mut hi_hits = 0;
        for _ in 0..200 {
            lo_hits += (on_vel(&mut fx, 1) == 1) as u32;
            hi_hits += (on_vel(&mut fx, 127) == 127) as u32;
        }
        // Roughly half the draws fall past the edge and must clamp onto it.
        assert!(lo_hits > 50, "lo clamp hits: {lo_hits}");
        assert!(hi_hits > 50, "hi clamp hits: {hi_hits}");
    }

    #[test]
    fn same_seed_same_output() {
        let dist = VelDist::Gaussian { sigma: 20.0 };
        let mut a = VelocityDice::new(9, dist);
        let mut b = VelocityDice::new(9, dist);
        for vel in [100, 1, 64, 127, 33] {
            assert_eq!(on_vel(&mut a, vel), on_vel(&mut b, vel));
        }
    }

    #[test]
    fn note_offs_and_other_events_pass() {
        let mut fx = VelocityDice::new(1, VelDist::Uniform { lo: 1, hi: 127 });
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
