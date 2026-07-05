//! A slow architectural envelope over the whole mass of notes.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// The shape of the envelope over one period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrescendoShape {
    /// Rises from 0 to 1 across the period, then resets: a sawtooth
    /// crescendo.
    Ramp,
    /// Rises to 1 at the period's midpoint and falls back: a swell.
    Arch,
}

/// Scale note-on velocities by a slow periodic envelope anchored at the
/// first note-on, the long architectural crescendo of a Xenakis mass or a
/// Ligeti swell: with `phase = ((t - anchor) % period) / period`, the
/// envelope is `phase` for `Ramp` and `1 - |2 * phase - 1|` for `Arch`,
/// and velocity scales by `(1 - depth) + depth * envelope`, clamped to
/// 1..=127. Depth 0 leaves the playing alone; depth 1 fades to a whisper
/// at the envelope's floor.
///
/// Keys are unchanged, so note-offs pass untouched, as does everything
/// else.
///
/// Fanout bound: exactly one output per input.
pub struct MassCrescendo {
    period_ns: u64,
    depth: f32,
    shape: CrescendoShape,
    /// Envelope origin, fixed by the first note-on.
    anchor: Option<u64>,
}

impl MassCrescendo {
    /// `period_ns` is raised to at least one second and `depth` clamped
    /// to 0.0..=1.0.
    pub fn new(period_ns: u64, depth: f32, shape: CrescendoShape) -> Self {
        Self {
            period_ns: period_ns.max(1_000_000_000),
            depth: depth.clamp(0.0, 1.0),
            shape,
            anchor: None,
        }
    }

    /// The velocity scale at time `t`.
    fn scale(&mut self, t: u64) -> f64 {
        let anchor = *self.anchor.get_or_insert(t);
        let phase = (t.saturating_sub(anchor) % self.period_ns) as f64 / self.period_ns as f64;
        let envelope = match self.shape {
            CrescendoShape::Ramp => phase,
            CrescendoShape::Arch => 1.0 - (2.0 * phase - 1.0).abs(),
        };
        let depth = f64::from(self.depth);
        (1.0 - depth) + depth * envelope
    }
}

impl Effect for MassCrescendo {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let scaled = f64::from(vel) * self.scale(ev.time);
                EventKind::NoteOn {
                    ch,
                    key,
                    vel: scaled.round().clamp(1.0, 127.0) as u8,
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
    use crate::testutil::{off, run_timed};

    const PERIOD: u64 = 1_000_000_000;

    fn on_vel(fx: &mut MassCrescendo, time: u64, vel: u8) -> u8 {
        let out = run_timed(
            fx,
            time,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel,
            },
        );
        match out[..] {
            [
                Event {
                    kind: EventKind::NoteOn { vel, .. },
                    ..
                },
            ] => vel,
            ref other => panic!("expected one note-on, got {other:?}"),
        }
    }

    #[test]
    fn ramp_phase_math_at_anchor_mid_and_wrap() {
        let mut fx = MassCrescendo::new(PERIOD, 0.5, CrescendoShape::Ramp);
        // Anchor: phase 0, scale 1 - 0.5 = 0.5.
        assert_eq!(on_vel(&mut fx, 10_000, 100), 50);
        // Midway: phase 0.5, scale 0.75.
        assert_eq!(on_vel(&mut fx, 10_000 + PERIOD / 2, 100), 75);
        // One full period later the ramp has reset.
        assert_eq!(on_vel(&mut fx, 10_000 + PERIOD, 100), 50);
        // Late in the period the ramp approaches full strength.
        assert_eq!(on_vel(&mut fx, 10_000 + PERIOD / 4 * 7, 100), 88);
    }

    #[test]
    fn arch_phase_math_at_anchor_quarter_mid_and_wrap() {
        let mut fx = MassCrescendo::new(PERIOD, 0.5, CrescendoShape::Arch);
        // Anchor: envelope 1 - |0 - 1| = 0, scale 0.5.
        assert_eq!(on_vel(&mut fx, 0, 100), 50);
        // Quarter: envelope 0.5, scale 0.75.
        assert_eq!(on_vel(&mut fx, PERIOD / 4, 100), 75);
        // Midpoint: envelope 1, full strength.
        assert_eq!(on_vel(&mut fx, PERIOD / 2, 100), 100);
        // Three quarters: back down the far side.
        assert_eq!(on_vel(&mut fx, PERIOD / 4 * 3, 100), 75);
        // The wrap lands back at the floor.
        assert_eq!(on_vel(&mut fx, PERIOD, 100), 50);
    }

    #[test]
    fn depth_zero_is_identity() {
        let mut fx = MassCrescendo::new(PERIOD, 0.0, CrescendoShape::Ramp);
        for i in 0..8u64 {
            assert_eq!(on_vel(&mut fx, i * PERIOD / 3, 100), 100);
        }
    }

    #[test]
    fn full_depth_at_the_floor_never_emits_zero() {
        let mut fx = MassCrescendo::new(PERIOD, 1.0, CrescendoShape::Ramp);
        assert_eq!(on_vel(&mut fx, 0, 127), 1);
    }

    #[test]
    fn constructor_clamps_period_and_depth() {
        // Period 0 rises to one second; depth 7 clamps to 1.
        let mut fx = MassCrescendo::new(0, 7.0, CrescendoShape::Ramp);
        assert_eq!(on_vel(&mut fx, 0, 100), 1);
        assert_eq!(on_vel(&mut fx, PERIOD / 2, 100), 50);
    }

    #[test]
    fn note_offs_and_other_events_pass_untouched() {
        let mut fx = MassCrescendo::new(PERIOD, 1.0, CrescendoShape::Arch);
        assert_eq!(run_timed(&mut fx, 0, off(60)), vec![Event::new(0, off(60))]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 1, pedal), vec![Event::new(1, pedal)]);
    }
}
