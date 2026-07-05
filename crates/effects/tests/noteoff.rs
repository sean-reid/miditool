//! No effect may orphan a note: after an arbitrary sequence of note-ons and
//! note-offs plus a flush, nothing is left sounding at the output.

use miditool_core::{Effect, Event, EventBuf, EventKind, Node, NoteTracker, ProcCx};
use miditool_effects::{
    Channelize, KeyDist, LooseKeys, ShuffleLock, ShuffleMode, Transpose, VelocityCurve,
};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct Step {
    on: bool,
    ch: u8,
    key: u8,
    vel: u8,
}

/// Short sequences keep the worst-case flush (one note-off per active note
/// per stateful effect in a chain) inside one EventBuf.
fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        (any::<bool>(), 0u8..2, 0u8..128, 1u8..128).prop_map(|(on, ch, key, vel)| Step {
            on,
            ch,
            key,
            vel,
        }),
        0..32,
    )
}

fn modes() -> impl Strategy<Value = ShuffleMode> {
    prop_oneof![
        Just(ShuffleMode::Free),
        Just(ShuffleMode::WithinOctave),
        Just(ShuffleMode::WithinPitchClass),
    ]
}

/// Append a note-off per outstanding note-on. A stateless effect cannot end
/// notes the player never released, so its no-orphan property is stated
/// over balanced input; the stateful effects are tested on raw sequences,
/// where flush must do the cleanup.
fn balanced(steps: &[Step]) -> Vec<Step> {
    let mut counts = [[0u32; 128]; 16];
    let mut all = steps.to_vec();
    for step in steps {
        let slot = &mut counts[step.ch as usize][step.key as usize];
        *slot = if step.on {
            *slot + 1
        } else {
            slot.saturating_sub(1)
        };
    }
    for (ch, keys) in counts.iter().enumerate() {
        for (key, &count) in keys.iter().enumerate() {
            for _ in 0..count {
                all.push(Step {
                    on: false,
                    ch: ch as u8,
                    key: key as u8,
                    vel: 1,
                });
            }
        }
    }
    all
}

fn assert_no_orphans(node: &mut Node, steps: &[Step]) {
    let mut tracker = NoteTracker::new();
    let cx = ProcCx::at(0);
    for (i, step) in steps.iter().enumerate() {
        let kind = if step.on {
            EventKind::NoteOn {
                ch: step.ch,
                key: step.key,
                vel: step.vel,
            }
        } else {
            EventKind::NoteOff {
                ch: step.ch,
                key: step.key,
                vel: 0,
            }
        };
        let mut out = EventBuf::new();
        node.process(&Event::new(i as u64, kind), &mut out, &cx);
        for ev in &out {
            tracker.observe(&ev.kind);
        }
    }
    let mut out = EventBuf::new();
    node.flush(&mut out, &cx);
    for ev in &out {
        tracker.observe(&ev.kind);
    }
    assert_eq!(tracker.active(), 0, "orphaned notes at the output");
}

fn leaf(fx: impl Effect + 'static) -> Node {
    Node::Leaf(Box::new(fx))
}

proptest! {
    #[test]
    fn transpose_no_orphans(steps in steps(), semis in -140i16..=140) {
        assert_no_orphans(&mut leaf(Transpose::new(semis)), &steps);
    }

    #[test]
    fn shuffle_lock_no_orphans(
        steps in steps(),
        seed: u64,
        lo in 0u8..128,
        hi in 0u8..128,
        mode in modes(),
    ) {
        assert_no_orphans(&mut leaf(ShuffleLock::new(seed, lo, hi, mode)), &steps);
    }

    #[test]
    fn loose_keys_uniform_no_orphans(
        steps in steps(),
        seed: u64,
        lo in 0u8..128,
        hi in 0u8..128,
    ) {
        let fx = LooseKeys::new(seed, KeyDist::Uniform { lo, hi });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn loose_keys_gaussian_no_orphans(steps in steps(), seed: u64, sigma in 0.0f32..60.0) {
        let fx = LooseKeys::new(seed, KeyDist::Gaussian { sigma });
        assert_no_orphans(&mut leaf(fx), &steps);
    }

    #[test]
    fn velocity_curve_no_orphans(steps in steps(), gamma in 0.1f32..4.0) {
        let fx = VelocityCurve { gamma, floor: 10, ceiling: 120 };
        assert_no_orphans(&mut leaf(fx), &balanced(&steps));
    }

    #[test]
    fn channelize_no_orphans(steps in steps(), ch in 0u8..16) {
        assert_no_orphans(&mut leaf(Channelize { ch }), &balanced(&steps));
    }

    #[test]
    fn chain_no_orphans(steps in steps(), seed: u64, semis in -24i16..=24, mode in modes()) {
        let mut node = Node::Chain(vec![
            leaf(Channelize { ch: 3 }),
            leaf(Transpose::new(semis)),
            leaf(ShuffleLock::new(seed, 24, 96, mode)),
            leaf(LooseKeys::new(seed, KeyDist::Gaussian { sigma: 12.0 })),
            leaf(VelocityCurve { gamma: 1.5, floor: 5, ceiling: 127 }),
        ]);
        assert_no_orphans(&mut node, &steps);
    }
}
