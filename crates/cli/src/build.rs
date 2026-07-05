//! Compile parsed config specs into a runnable effect graph.

use std::path::Path;

use miditool_config::{EffectSpec, OutputSpec, RowForm, ShuffleMode, SieveSnap};
use miditool_core::graph::{Discard, Filter, Node, Pass};
use miditool_effects::{
    AggregateGate, BlockedKeys, Channelize, Delay, Echo, KeyDist, Klangfarben, LooseKeys,
    RegistralScatter, Restrike, RingMod, RowSnap, ShuffleLock, SieveQuantizer, Stutter, Telescope,
    Transpose, VelocityCurve, WedgeMirror,
};
use miditool_io::OutputTarget;

/// Build the root node from the config's implicit top-level chain.
/// `tempo` resolves `beats=` times to absolute nanoseconds, and `base`
/// is the directory script paths resolve against (the config file's).
/// Fails when a `script` node's file cannot be loaded or compiled.
pub fn build_graph(chain: Vec<EffectSpec>, tempo: f32, base: &Path) -> Result<Node, String> {
    Ok(Node::Chain(
        chain
            .into_iter()
            .map(|s| build_node(s, tempo, base))
            .collect::<Result<_, _>>()?,
    ))
}

fn build_node(spec: EffectSpec, tempo: f32, base: &Path) -> Result<Node, String> {
    Ok(match spec {
        EffectSpec::Chain(children) => Node::Chain(
            children
                .into_iter()
                .map(|s| build_node(s, tempo, base))
                .collect::<Result<_, _>>()?,
        ),
        EffectSpec::Fork(children) => Node::Fork(
            children
                .into_iter()
                .map(|s| build_node(s, tempo, base))
                .collect::<Result<_, _>>()?,
        ),
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
        EffectSpec::RegistralScatter { seed, lo, hi } => {
            Node::Leaf(Box::new(RegistralScatter::new(seed, lo, hi)))
        }
        EffectSpec::WedgeMirror {
            axis,
            probability,
            seed,
        } => Node::Leaf(Box::new(WedgeMirror::new(axis, probability, seed))),
        EffectSpec::BlockedKeys { keys, by_class } => {
            Node::Leaf(Box::new(BlockedKeys::new(&keys, by_class)))
        }
        EffectSpec::Klangfarben {
            channels,
            random,
            seed,
        } => Node::Leaf(Box::new(Klangfarben::new(&channels, random, seed))),
        EffectSpec::RingMod {
            carrier,
            sum,
            diff,
            dry,
        } => Node::Leaf(Box::new(RingMod::new(carrier, sum, diff, dry))),
        EffectSpec::Telescope { factor, reference } => {
            Node::Leaf(Box::new(Telescope::new(factor, reference)))
        }
        EffectSpec::RowSnap {
            row,
            form,
            transpose,
        } => Node::Leaf(Box::new(RowSnap::new(row, row_form(form), transpose))),
        EffectSpec::AggregateGate { leak, seed } => {
            Node::Leaf(Box::new(AggregateGate::new(leak, seed)))
        }
        EffectSpec::Sieve { expr, snap } => {
            let sieve = miditool_core::sieve::Sieve::parse(&expr)
                .map_err(|e| format!("sieve \"{expr}\": {e}"))?;
            Node::Leaf(Box::new(SieveQuantizer::new(sieve, sieve_snap(snap))))
        }
        EffectSpec::Script { path, seed } => {
            let resolved = base.join(&path);
            let effect = miditool_script::ScriptEffect::from_file(&resolved, seed)
                .map_err(|e| format!("script {}: {e}", resolved.display()))?;
            Node::Leaf(Box::new(effect))
        }
    })
}

fn shuffle_mode(mode: ShuffleMode) -> miditool_effects::ShuffleMode {
    match mode {
        ShuffleMode::Free => miditool_effects::ShuffleMode::Free,
        ShuffleMode::WithinOctave => miditool_effects::ShuffleMode::WithinOctave,
        ShuffleMode::WithinPitchClass => miditool_effects::ShuffleMode::WithinPitchClass,
    }
}

fn row_form(form: RowForm) -> miditool_effects::RowForm {
    match form {
        RowForm::Prime => miditool_effects::RowForm::Prime,
        RowForm::Inversion => miditool_effects::RowForm::Inversion,
        RowForm::Retrograde => miditool_effects::RowForm::Retrograde,
        RowForm::RetrogradeInversion => miditool_effects::RowForm::RetrogradeInversion,
    }
}

fn sieve_snap(snap: SieveSnap) -> miditool_effects::SieveSnap {
    match snap {
        SieveSnap::Nearest => miditool_effects::SieveSnap::Nearest,
        SieveSnap::Up => miditool_effects::SieveSnap::Up,
        SieveSnap::Down => miditool_effects::SieveSnap::Down,
        SieveSnap::Drop => miditool_effects::SieveSnap::Drop,
    }
}

pub fn output_target(spec: OutputSpec) -> OutputTarget {
    match spec {
        OutputSpec::Virtual(name) => OutputTarget::Virtual(name),
        OutputSpec::Device(name) => OutputTarget::Device(name),
    }
}

/// End-to-end over real virtual ports: a temp-dir config whose `script`
/// node points at a transpose-by-12 Luau file, compiled by the real
/// [`build_graph`] with the temp dir as `base`, run through a live
/// engine. Proves path resolution and script execution together. macOS
/// only, like the loopback tests; Linux CI has no sequencer device.
#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use miditool_engine::{Engine, SceneDef};
    use miditool_io::{Output, OutputTarget, open_input, open_output};

    use super::build_graph;

    /// Block until the keyboard-to-capture path is live, the same probe
    /// loop the loopback tests use: CoreMIDI wires virtual ports up
    /// asynchronously.
    fn wait_until_live(keyboard: &mut Output, seen: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            keyboard.send(&[0x90, 0, 1]).expect("send probe note-on");
            keyboard.send(&[0x80, 0, 0]).expect("send probe note-off");
            let retry = Instant::now() + Duration::from_millis(200);
            while Instant::now() < retry {
                if seen() {
                    // Give the probe's partner message a moment to land
                    // so the caller's clear wipes both.
                    sleep(Duration::from_millis(250));
                    return;
                }
                sleep(Duration::from_millis(10));
            }
            assert!(
                Instant::now() < deadline,
                "the loopback never became live: no probe note arrived in 5s"
            );
        }
    }

    fn wait_for(mut pred: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if pred() {
                return true;
            }
            sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn script_node_transposes_end_to_end() {
        // A throwaway directory holding the config and the script it
        // names, so `script "up.lua"` exercises resolution against the
        // config file's directory.
        let dir = std::env::temp_dir().join(format!("miditool-script-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        std::fs::write(
            dir.join("up.lua"),
            r#"
function on_event(ev)
    if ev.kind == "note-on" or ev.kind == "note-off" then
        ev.key = ev.key + 12
        return ev
    end
end
"#,
        )
        .expect("write script");
        std::fs::write(dir.join("miditool.kdl"), "script \"up.lua\" seed=1\n")
            .expect("write config");

        let cfg = miditool_config::parse_file(&dir.join("miditool.kdl")).expect("parse config");
        let chain = cfg.scenes[0].chain.clone();
        let base = dir.clone();

        let mut keyboard = open_output(&OutputTarget::Virtual("miditool script kb".into()))
            .expect("create fake keyboard");

        let (engine, _handle) = Engine::run(
            Some("miditool script kb"),
            &OutputTarget::Virtual("miditool script out".into()),
            vec![SceneDef {
                name: "main".to_owned(),
                kill_on_exit: false,
            }],
            Box::new(move |_| build_graph(chain.clone(), 120.0, &base)),
            None,
        )
        .expect("start engine");

        let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let sink = Arc::clone(&received);
        let _capture = open_input(Some("miditool script out"), move |_stamp, bytes| {
            sink.lock().unwrap().push(bytes.to_vec());
        })
        .expect("open capture port");

        wait_until_live(&mut keyboard, || !received.lock().unwrap().is_empty());
        received.lock().unwrap().clear();
        keyboard.send(&[0x90, 60, 100]).unwrap();
        keyboard.send(&[0x80, 60, 0]).unwrap();

        assert!(
            wait_for(|| received.lock().unwrap().len() >= 2),
            "expected 2 messages, got {:?}",
            received.lock().unwrap()
        );
        let msgs = received.lock().unwrap();
        assert_eq!(
            msgs[0],
            vec![0x90, 72, 100],
            "the script transposes the note-on up an octave"
        );
        assert_eq!(
            msgs[1],
            vec![0x80, 72, 0],
            "the note-off matches the transposed note-on"
        );
        drop(msgs);
        engine.stop().expect("stop engine");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
