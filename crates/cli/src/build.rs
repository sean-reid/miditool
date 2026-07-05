//! Compile parsed config specs into a runnable effect graph.

use miditool_config::{EffectSpec, OutputSpec, ShuffleMode};
use miditool_core::graph::{Discard, Filter, Node, Pass};
use miditool_effects::{
    Channelize, Delay, Echo, KeyDist, LooseKeys, Restrike, ShuffleLock, Stutter, Transpose,
    VelocityCurve,
};
use miditool_io::OutputTarget;

/// Build the root node from the config's implicit top-level chain.
/// `tempo` resolves `beats=` times to absolute nanoseconds.
pub fn build_graph(chain: Vec<EffectSpec>, tempo: f32) -> Node {
    Node::Chain(chain.into_iter().map(|s| build_node(s, tempo)).collect())
}

fn build_node(spec: EffectSpec, tempo: f32) -> Node {
    match spec {
        EffectSpec::Chain(children) => {
            Node::Chain(children.into_iter().map(|s| build_node(s, tempo)).collect())
        }
        EffectSpec::Fork(children) => {
            Node::Fork(children.into_iter().map(|s| build_node(s, tempo)).collect())
        }
        EffectSpec::Pass => Node::Leaf(Box::new(Pass)),
        EffectSpec::Discard => Node::Leaf(Box::new(Discard)),
        EffectSpec::Transpose { semis } => Node::Leaf(Box::new(Transpose::new(semis))),
        EffectSpec::ShuffleLock { seed, lo, hi, mode } => {
            Node::Leaf(Box::new(ShuffleLock::new(seed, lo, hi, shuffle_mode(mode))))
        }
        EffectSpec::LooseKeysUniform { seed, lo, hi } => {
            Node::Leaf(Box::new(LooseKeys::new(seed, KeyDist::Uniform { lo, hi })))
        }
        EffectSpec::LooseKeysGaussian { seed, sigma } => {
            Node::Leaf(Box::new(LooseKeys::new(seed, KeyDist::Gaussian { sigma })))
        }
        EffectSpec::VelocityCurve {
            gamma,
            floor,
            ceiling,
        } => Node::Leaf(Box::new(VelocityCurve {
            gamma,
            floor,
            ceiling,
        })),
        EffectSpec::Channelize { ch } => Node::Leaf(Box::new(Channelize { ch })),
        EffectSpec::OnlyChannels(channels) => {
            let mask = channels.iter().fold(0u16, |m, ch| m | 1 << ch);
            Node::Filter(Filter::Channels(mask))
        }
        EffectSpec::KeyRange { lo, hi } => Node::Filter(Filter::KeyRange { lo, hi }),
        EffectSpec::VelocityRange { lo, hi } => Node::Filter(Filter::VelocityRange { lo, hi }),
        EffectSpec::NotesOnly => Node::Filter(Filter::NotesOnly),
        EffectSpec::ControllersOnly => Node::Filter(Filter::ControllersOnly),
        EffectSpec::Delay { time } => Node::Leaf(Box::new(Delay::new(time.to_nanos(tempo)))),
        EffectSpec::Echo {
            repeats,
            time,
            decay,
            transpose,
        } => Node::Leaf(Box::new(Echo::new(
            repeats,
            time.to_nanos(tempo),
            decay,
            transpose,
        ))),
        EffectSpec::Restrike {
            seed,
            interval,
            jitter,
            decay,
            floor,
            max,
        } => Node::Leaf(Box::new(Restrike::new(
            seed,
            interval.to_nanos(tempo),
            jitter,
            decay,
            floor,
            max,
        ))),
        EffectSpec::Stutter {
            repeats,
            first,
            curve,
        } => Node::Leaf(Box::new(Stutter::new(
            repeats,
            first.to_nanos(tempo),
            curve,
        ))),
    }
}

fn shuffle_mode(mode: ShuffleMode) -> miditool_effects::ShuffleMode {
    match mode {
        ShuffleMode::Free => miditool_effects::ShuffleMode::Free,
        ShuffleMode::WithinOctave => miditool_effects::ShuffleMode::WithinOctave,
        ShuffleMode::WithinPitchClass => miditool_effects::ShuffleMode::WithinPitchClass,
    }
}

pub fn output_target(spec: OutputSpec) -> OutputTarget {
    match spec {
        OutputSpec::Virtual(name) => OutputTarget::Virtual(name),
        OutputSpec::Device(name) => OutputTarget::Device(name),
    }
}
