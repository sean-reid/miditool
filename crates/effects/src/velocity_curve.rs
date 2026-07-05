//! Velocity shaping with a gamma curve.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Reshape note-on velocity: input velocity v maps to
/// `floor + (ceiling - floor) * (v / 127) ^ gamma`, rounded and clamped to
/// 1..=127. Gamma below 1.0 lifts soft playing, above 1.0 tames it.
/// Note-offs and everything else pass untouched.
#[derive(Debug, Clone, Copy)]
pub struct VelocityCurve {
    pub gamma: f32,
    pub floor: u8,
    pub ceiling: u8,
}

impl Effect for VelocityCurve {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let span = self.ceiling as f32 - self.floor as f32;
                let curved = self.floor as f32 + span * (vel as f32 / 127.0).powf(self.gamma);
                // Velocity 0 would read as a note-off on the wire; never
                // emit it.
                let vel = curved.round().clamp(1.0, 127.0) as u8;
                EventKind::NoteOn { ch, key, vel }
            }
            other => other,
        };
        push(out, cx, Event::new(ev.time, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{off, run};

    fn on_vel(fx: &mut VelocityCurve, vel: u8) -> u8 {
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
    fn unit_gamma_full_range_is_identity() {
        let mut fx = VelocityCurve {
            gamma: 1.0,
            floor: 0,
            ceiling: 127,
        };
        for vel in 1..=127 {
            assert_eq!(on_vel(&mut fx, vel), vel);
        }
    }

    #[test]
    fn output_is_never_zero() {
        let mut fx = VelocityCurve {
            gamma: 4.0,
            floor: 0,
            ceiling: 127,
        };
        for vel in 1..=127 {
            assert!(on_vel(&mut fx, vel) >= 1);
        }
    }

    #[test]
    fn floor_and_ceiling_bound_the_curve() {
        let mut fx = VelocityCurve {
            gamma: 0.5,
            floor: 20,
            ceiling: 100,
        };
        assert_eq!(on_vel(&mut fx, 127), 100);
        for vel in 1..=127 {
            let v = on_vel(&mut fx, vel);
            assert!((20..=100).contains(&v));
        }
    }

    #[test]
    fn note_offs_pass_untouched() {
        let mut fx = VelocityCurve {
            gamma: 2.0,
            floor: 20,
            ceiling: 100,
        };
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }
}
