//! Dynamics turned upside down around a pivot.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Invert note-on velocity around `pivot`: the output is
/// `clamp(2 * pivot - vel, 1, 127)`, so what the player hammers comes out
/// whispered and what they brush comes out hammered, with the pivot
/// itself the fixed point. Note-offs and everything else pass untouched.
///
/// Fanout bound: exactly one output per input.
pub struct VelocityInvert {
    pivot: u8,
}

impl VelocityInvert {
    /// `pivot` is clamped to 1..=127.
    pub fn new(pivot: u8) -> Self {
        Self {
            pivot: pivot.clamp(1, 127),
        }
    }
}

impl Effect for VelocityInvert {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let inverted = 2 * i16::from(self.pivot) - i16::from(vel);
                EventKind::NoteOn {
                    ch,
                    key,
                    vel: inverted.clamp(1, 127) as u8,
                }
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

    fn on_vel(fx: &mut VelocityInvert, vel: u8) -> u8 {
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
    fn the_pivot_is_the_fixed_point() {
        let mut fx = VelocityInvert::new(64);
        assert_eq!(on_vel(&mut fx, 64), 64);
    }

    #[test]
    fn loud_and_soft_swap_around_the_pivot() {
        let mut fx = VelocityInvert::new(64);
        assert_eq!(on_vel(&mut fx, 1), 127);
        assert_eq!(on_vel(&mut fx, 127), 1);
        assert_eq!(on_vel(&mut fx, 100), 28);
        assert_eq!(on_vel(&mut fx, 28), 100);
    }

    #[test]
    fn results_clamp_into_one_to_127() {
        // 2 * 20 - 127 = -87 clamps up to 1.
        let mut fx = VelocityInvert::new(20);
        assert_eq!(on_vel(&mut fx, 127), 1);
        // 2 * 120 - 1 = 239 clamps down to 127.
        let mut fx = VelocityInvert::new(120);
        assert_eq!(on_vel(&mut fx, 1), 127);
    }

    #[test]
    fn pivot_zero_clamps_to_one() {
        let mut fx = VelocityInvert::new(0);
        assert_eq!(on_vel(&mut fx, 1), 1);
        assert_eq!(on_vel(&mut fx, 127), 1);
    }

    #[test]
    fn note_offs_and_other_events_pass_untouched() {
        let mut fx = VelocityInvert::new(64);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
