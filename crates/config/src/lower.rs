//! Lowering from the raw [`ast`](crate::ast) shapes into the public spec
//! types: defaults filled in, ranges checked, channels rebased from the
//! human 1-16 to the wire 0-15.

use std::net::IpAddr;

use crate::ast;
use crate::{
    ClusterAnchor, ClusterKind, Config, ConfigError, CrescendoShape, EffectSpec, OutputSpec, Plr,
    RemoteSpec, RowForm, SceneSpec, ShuffleMode, SieveSnap, TDirection, TimeSpec,
};

/// Output port name used when the config has no `output` node.
const DEFAULT_OUTPUT: &str = "miditool Out";

/// Scene name given to the implicit chain of a bare-style config.
const MAIN_SCENE: &str = "main";

/// Tempo used when the config has no `tempo` node.
const DEFAULT_TEMPO: f32 = 120.0;

/// Address the web remote binds when the `remote` node has no `bind=`:
/// loopback, so turning the remote on does not expose it to the network.
const DEFAULT_BIND: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

/// How deep `chain`/`fork` blocks may nest. The graph is compiled and
/// walked recursively, so unbounded nesting could overflow the stack;
/// real configs stay in single digits.
const MAX_EFFECT_DEPTH: usize = 64;

/// Default key range for the randomizing effects: A0..=C8, the 88 keys of
/// a piano.
const DEFAULT_LO: u8 = 21;
const DEFAULT_HI: u8 = 108;

pub(crate) fn document(doc: ast::Document) -> Result<Config, ConfigError> {
    let output = match doc.output {
        None => OutputSpec::Virtual(DEFAULT_OUTPUT.to_owned()),
        Some(out) => match (out.virtual_, out.device) {
            (Some(name), None) => OutputSpec::Virtual(name),
            (None, Some(name)) => OutputSpec::Device(name),
            (None, None) => {
                return Err(ConfigError::invalid(
                    "output",
                    "expected either virtual=\"Name\" or device=\"substring\"",
                ));
            }
            (Some(_), Some(_)) => {
                return Err(ConfigError::invalid(
                    "output",
                    "virtual= and device= are mutually exclusive; give one",
                ));
            }
        },
    };
    Ok(Config {
        input: doc.input.as_ref().map(|i| i.name.clone()),
        hide_input: doc.input.as_ref().and_then(|i| i.hide).unwrap_or(false),
        output,
        tempo: tempo(doc.tempo)?,
        remote: remote(doc.remote)?,
        scenes: scenes(doc.scenes, doc.effects)?,
    })
}

fn remote(node: Option<ast::Remote>) -> Result<Option<RemoteSpec>, ConfigError> {
    let Some(ast::Remote { port, bind }) = node else {
        return Ok(None);
    };
    if !(1..=65535).contains(&port) {
        return Err(ConfigError::invalid(
            "remote",
            format!("port must be within 1..=65535, got {port}"),
        ));
    }
    let bind = match bind {
        None => DEFAULT_BIND,
        Some(text) => text.parse().map_err(|_| {
            ConfigError::invalid(
                "remote",
                format!(
                    "bind must be an IP address like \"127.0.0.1\" or \"0.0.0.0\", got \"{text}\""
                ),
            )
        })?,
    };
    Ok(Some(RemoteSpec {
        port: port as u16,
        bind,
    }))
}

fn scenes(
    scene_nodes: Vec<ast::Scene>,
    loose: Vec<ast::Effect>,
) -> Result<Vec<SceneSpec>, ConfigError> {
    if scene_nodes.is_empty() {
        // The bare style: the whole document is one implicit chain. This
        // covers the empty document too; an empty chain passes events
        // through, so a pure pass-through config stays valid.
        return Ok(vec![SceneSpec {
            name: MAIN_SCENE.to_owned(),
            kill_on_exit: false,
            chain: effects(loose)?,
        }]);
    }
    if !loose.is_empty() {
        return Err(ConfigError::invalid(
            "scene",
            "bare effects and scene blocks do not mix; \
             put the loose effects in a scene block",
        ));
    }
    let mut scenes = Vec::with_capacity(scene_nodes.len());
    for node in scene_nodes {
        let scene = scene(node)?;
        if scenes.iter().any(|s: &SceneSpec| s.name == scene.name) {
            return Err(ConfigError::invalid(
                "scene",
                format!("duplicate scene name \"{}\"", scene.name),
            ));
        }
        scenes.push(scene);
    }
    Ok(scenes)
}

fn scene(node: ast::Scene) -> Result<SceneSpec, ConfigError> {
    if node.name.is_empty() {
        return Err(ConfigError::invalid(
            "scene",
            "the scene name must not be empty",
        ));
    }
    let kill_on_exit = match node.switch.as_deref() {
        None | Some("let-ring") => false,
        Some("kill") => true,
        Some(other) => {
            return Err(ConfigError::invalid(
                "scene",
                format!("switch must be \"kill\" or \"let-ring\", got \"{other}\""),
            ));
        }
    };
    if node.effects.is_empty() {
        return Err(ConfigError::invalid(
            "scene",
            format!(
                "scene \"{}\" is empty; give it at least one effect",
                node.name
            ),
        ));
    }
    Ok(SceneSpec {
        name: node.name,
        kill_on_exit,
        chain: effects(node.effects)?,
    })
}

fn tempo(node: Option<ast::Tempo>) -> Result<f32, ConfigError> {
    let Some(ast::Tempo {
        bpm: ast::Number(bpm),
    }) = node
    else {
        return Ok(DEFAULT_TEMPO);
    };
    if bpm.is_finite() && (20.0..=400.0).contains(&bpm) {
        Ok(bpm as f32)
    } else {
        Err(ConfigError::invalid(
            "tempo",
            format!("beats per minute must be within 20..=400, got {bpm}"),
        ))
    }
}

fn effects(nodes: Vec<ast::Effect>) -> Result<Vec<EffectSpec>, ConfigError> {
    effects_at(nodes, 0)
}

fn effects_at(nodes: Vec<ast::Effect>, depth: usize) -> Result<Vec<EffectSpec>, ConfigError> {
    nodes.into_iter().map(|node| effect(node, depth)).collect()
}

fn effect(node: ast::Effect, depth: usize) -> Result<EffectSpec, ConfigError> {
    use ast::Effect as E;
    Ok(match node {
        E::Chain { children } => {
            nesting("chain", depth)?;
            EffectSpec::Chain(effects_at(children, depth + 1)?)
        }
        E::Fork { children } => {
            nesting("fork", depth)?;
            EffectSpec::Fork(effects_at(children, depth + 1)?)
        }
        E::Pass => EffectSpec::Pass,
        E::Discard => EffectSpec::Discard,
        E::Transpose { semis } => {
            if !(-127..=127).contains(&semis) {
                return Err(ConfigError::invalid(
                    "transpose",
                    format!("semitones must be within -127..=127, got {semis}"),
                ));
            }
            EffectSpec::Transpose {
                semis: semis as i16,
            }
        }
        E::ShuffleLock { seed, lo, hi, mode } => {
            let (lo, hi) = key_range("shuffle-lock", lo, hi, DEFAULT_LO, DEFAULT_HI)?;
            EffectSpec::ShuffleLock {
                seed,
                lo,
                hi,
                mode: shuffle_mode(mode)?,
            }
        }
        E::LooseKeys {
            seed,
            lo,
            hi,
            sigma,
        } => match sigma {
            // sigma wins over lo/hi when both are given.
            Some(ast::Number(sigma)) => {
                if !(sigma.is_finite() && sigma > 0.0) {
                    return Err(ConfigError::invalid(
                        "loose-keys",
                        format!("sigma must be finite and greater than 0, got {sigma}"),
                    ));
                }
                EffectSpec::LooseKeysGaussian {
                    seed,
                    sigma: sigma as f32,
                }
            }
            None => {
                let (lo, hi) = key_range("loose-keys", lo, hi, DEFAULT_LO, DEFAULT_HI)?;
                EffectSpec::LooseKeysUniform { seed, lo, hi }
            }
        },
        E::VelocityCurve {
            gamma,
            floor,
            ceiling,
        } => {
            let gamma = gamma.map_or(1.0, |ast::Number(v)| v);
            if !(gamma.is_finite() && gamma > 0.0) {
                return Err(ConfigError::invalid(
                    "velocity-curve",
                    format!("gamma must be finite and greater than 0, got {gamma}"),
                ));
            }
            let floor = velocity("velocity-curve", "floor", floor.unwrap_or(1))?;
            let ceiling = velocity("velocity-curve", "ceiling", ceiling.unwrap_or(127))?;
            ordered("velocity-curve", "floor", floor, "ceiling", ceiling)?;
            EffectSpec::VelocityCurve {
                gamma: gamma as f32,
                floor,
                ceiling,
            }
        }
        E::Channelize { ch } => EffectSpec::Channelize {
            ch: channel("channelize", ch)?,
        },
        E::OnlyChannels { channels } => {
            if channels.is_empty() {
                return Err(ConfigError::invalid(
                    "only-channels",
                    "at least one channel is required",
                ));
            }
            let mut chans = channels
                .into_iter()
                .map(|ch| channel("only-channels", ch))
                .collect::<Result<Vec<u8>, _>>()?;
            chans.sort_unstable();
            chans.dedup();
            EffectSpec::OnlyChannels(chans)
        }
        E::KeyRange { lo, hi } => {
            let (lo, hi) = key_range("key-range", lo, hi, 0, 127)?;
            EffectSpec::KeyRange { lo, hi }
        }
        E::VelocityRange { lo, hi } => {
            let lo = velocity("velocity-range", "lo", lo.unwrap_or(1))?;
            let hi = velocity("velocity-range", "hi", hi.unwrap_or(127))?;
            ordered("velocity-range", "lo", lo, "hi", hi)?;
            EffectSpec::VelocityRange { lo, hi }
        }
        E::NotesOnly => EffectSpec::NotesOnly,
        E::ControllersOnly => EffectSpec::ControllersOnly,
        E::Delay { time, beats } => EffectSpec::Delay {
            time: time_spec("delay", "time", time, beats)?,
        },
        E::Echo {
            repeats,
            time,
            beats,
            decay,
            transpose,
        } => {
            let time = time_spec("echo", "time", time, beats)?;
            let transpose = transpose.unwrap_or(0);
            if !(-24..=24).contains(&transpose) {
                return Err(ConfigError::invalid(
                    "echo",
                    format!("transpose must be within -24..=24 semitones, got {transpose}"),
                ));
            }
            EffectSpec::Echo {
                repeats: bounded("echo", "repeats", repeats.unwrap_or(3), 1, 16)?,
                time,
                decay: decay_factor(
                    "echo",
                    decay.map_or(0.6, |ast::Number(v)| v),
                    OneIs::Allowed,
                )?,
                transpose: transpose as i16,
            }
        }
        E::Restrike {
            seed,
            interval,
            beats,
            jitter,
            decay,
            floor,
            max,
        } => {
            let interval = time_spec("restrike", "interval", interval, beats)?;
            let jitter = jitter.map_or(0.15, |ast::Number(v)| v);
            if !(jitter.is_finite() && (0.0..=0.9).contains(&jitter)) {
                return Err(ConfigError::invalid(
                    "restrike",
                    format!("jitter must be within 0..=0.9, got {jitter}"),
                ));
            }
            EffectSpec::Restrike {
                seed,
                interval,
                jitter: jitter as f32,
                decay: decay_factor(
                    "restrike",
                    decay.map_or(0.7, |ast::Number(v)| v),
                    OneIs::Excluded,
                )?,
                floor: velocity("restrike", "floor", floor.unwrap_or(8))?,
                max: bounded("restrike", "max", max.unwrap_or(12), 1, 24)?,
            }
        }
        E::Stutter {
            repeats,
            first,
            beats,
            curve,
        } => {
            let first = time_spec("stutter", "first", first, beats)?;
            let curve = curve.map_or(1.0, |ast::Number(v)| v);
            if !(curve.is_finite() && (0.25..=4.0).contains(&curve)) {
                return Err(ConfigError::invalid(
                    "stutter",
                    format!("curve must be within 0.25..=4.0, got {curve}"),
                ));
            }
            EffectSpec::Stutter {
                repeats: bounded("stutter", "repeats", repeats.unwrap_or(6), 1, 24)?,
                first,
                curve: curve as f32,
            }
        }
        E::RegistralScatter { seed, lo, hi } => {
            let (lo, hi) = key_range("registral-scatter", lo, hi, DEFAULT_LO, DEFAULT_HI)?;
            EffectSpec::RegistralScatter { seed, lo, hi }
        }
        E::WedgeMirror {
            axis,
            probability,
            seed,
        } => EffectSpec::WedgeMirror {
            axis: key("wedge-mirror", "axis", axis.unwrap_or(60))?,
            probability: fraction(
                "wedge-mirror",
                "probability",
                probability.map_or(1.0, |ast::Number(v)| v),
            )?,
            seed: seed.unwrap_or(0),
        },
        E::BlockedKeys { keys, by_class } => {
            if keys.is_empty() {
                return Err(ConfigError::invalid(
                    "blocked-keys",
                    "at least one key is required",
                ));
            }
            let by_class = by_class.unwrap_or(false);
            let mut items = Vec::with_capacity(keys.len());
            for value in keys {
                if by_class && !(0..=11).contains(&value) {
                    return Err(ConfigError::invalid(
                        "blocked-keys",
                        format!(
                            "with by-class=true entries are pitch classes within 0..=11, \
                             got {value}"
                        ),
                    ));
                }
                items.push(if by_class {
                    value as u8
                } else {
                    key("blocked-keys", "key", value)?
                });
            }
            items.sort_unstable();
            items.dedup();
            EffectSpec::BlockedKeys {
                keys: items,
                by_class,
            }
        }
        E::Klangfarben {
            channels,
            mode,
            seed,
        } => {
            if channels.is_empty() {
                return Err(ConfigError::invalid(
                    "klangfarben",
                    "at least one channel is required",
                ));
            }
            // The list is the cycle, so order is kept as written and a
            // repeated channel is an error rather than being quietly
            // dropped the way only-channels dedupes.
            let mut chans: Vec<u8> = Vec::with_capacity(channels.len());
            for raw in channels {
                let ch = channel("klangfarben", raw)?;
                if chans.contains(&ch) {
                    return Err(ConfigError::invalid(
                        "klangfarben",
                        format!("channel {raw} is listed more than once"),
                    ));
                }
                chans.push(ch);
            }
            EffectSpec::Klangfarben {
                channels: chans,
                random: klangfarben_random(mode)?,
                seed: seed.unwrap_or(0),
            }
        }
        E::RingMod {
            carrier,
            sum,
            diff,
            dry,
        } => {
            let sum = sum.unwrap_or(true);
            let diff = diff.unwrap_or(true);
            let dry = dry.unwrap_or(false);
            if !(sum || diff || dry) {
                return Err(ConfigError::invalid(
                    "ring-mod",
                    "at least one of sum=, diff=, and dry= must be true, \
                     or every note is dropped",
                ));
            }
            EffectSpec::RingMod {
                carrier: key("ring-mod", "carrier", carrier)?,
                sum,
                diff,
                dry,
            }
        }
        E::Telescope {
            factor: ast::Number(factor),
            reference,
        } => {
            if !(factor.is_finite() && (0.1..=8.0).contains(&factor)) {
                return Err(ConfigError::invalid(
                    "telescope",
                    format!("factor must be within 0.1..=8.0, got {factor}"),
                ));
            }
            EffectSpec::Telescope {
                factor: factor as f32,
                reference: key("telescope", "reference", reference.unwrap_or(60))?,
            }
        }
        E::RowSnap {
            row,
            form,
            transpose,
        } => {
            let transpose = transpose.unwrap_or(0);
            if !(-24..=24).contains(&transpose) {
                return Err(ConfigError::invalid(
                    "row-snap",
                    format!("transpose must be within -24..=24 semitones, got {transpose}"),
                ));
            }
            EffectSpec::RowSnap {
                row: tone_row(row)?,
                form: row_form(form)?,
                transpose: transpose as i8,
            }
        }
        E::AggregateGate { leak, seed } => EffectSpec::AggregateGate {
            leak: fraction(
                "aggregate-gate",
                "leak",
                leak.map_or(0.0, |ast::Number(v)| v),
            )?,
            seed: seed.unwrap_or(0),
        },
        E::Sieve { expr, snap } => {
            if expr.is_empty() {
                return Err(ConfigError::invalid(
                    "sieve",
                    "the sieve expression must not be empty",
                ));
            }
            EffectSpec::Sieve {
                expr,
                snap: sieve_snap("sieve", snap)?,
            }
        }
        E::Tintinnabuli {
            root,
            minor,
            position,
            direction,
            level,
        } => EffectSpec::Tintinnabuli {
            root: pitch_class("tintinnabuli", "root", &root)?,
            minor: minor.unwrap_or(true),
            position: bounded("tintinnabuli", "position", position.unwrap_or(1), 1, 2)?,
            direction: t_direction(direction)?,
            level: fraction(
                "tintinnabuli",
                "level",
                level.map_or(0.8, |ast::Number(v)| v),
            )?,
        },
        E::ModeLock {
            mode,
            transposition,
            snap,
        } => EffectSpec::ModeLock {
            mode: bounded("mode-lock", "mode", mode, 1, 7)?,
            transposition: bounded(
                "mode-lock",
                "transposition",
                transposition.unwrap_or(0),
                0,
                11,
            )?,
            snap: sieve_snap("mode-lock", snap)?,
        },
        E::NegativeHarmony { tonic, mode, level } => EffectSpec::NegativeHarmony {
            tonic: pitch_class("negative-harmony", "tonic", &tonic)?,
            add: negative_harmony_add(mode)?,
            level: fraction(
                "negative-harmony",
                "level",
                level.map_or(0.8, |ast::Number(v)| v),
            )?,
        },
        E::Tonnetz {
            start,
            minor,
            sequence,
            lo,
            hi,
            include_played,
        } => {
            let (lo, hi) = key_range("tonnetz", lo, hi, 48, 79)?;
            EffectSpec::Tonnetz {
                start: pitch_class("tonnetz", "start", &start)?,
                minor: minor.unwrap_or(false),
                sequence: plr_sequence(sequence.as_deref().unwrap_or("rl"))?,
                lo,
                hi,
                include_played: include_played.unwrap_or(false),
            }
        }
        E::ComplementPad { lo, hi, vel } => {
            let (lo, hi) = key_range("complement-pad", lo, hi, 60, 84)?;
            EffectSpec::ComplementPad {
                lo,
                hi,
                vel: velocity("complement-pad", "vel", vel.unwrap_or(18))?,
            }
        }
        E::PoissonCloud {
            seed,
            density,
            duration,
            beats,
            sigma,
            vel_sigma,
            max,
        } => EffectSpec::PoissonCloud {
            seed,
            density: float_range(
                "poisson-cloud",
                "density",
                density.map_or(8.0, |ast::Number(d)| d),
                0.1,
                50.0,
            )?,
            duration: time_spec_or("poisson-cloud", "duration", duration, beats, 2000.0)?,
            sigma: float_range(
                "poisson-cloud",
                "sigma",
                sigma.map_or(7.0, |ast::Number(v)| v),
                0.0,
                24.0,
            )?,
            vel_sigma: float_range(
                "poisson-cloud",
                "vel-sigma",
                vel_sigma.map_or(10.0, |ast::Number(v)| v),
                0.0,
                40.0,
            )?,
            max: bounded("poisson-cloud", "max", max.unwrap_or(16), 1, 24)?,
        },
        E::NoteRoulette {
            seed,
            pass,
            replace,
            lo,
            hi,
        } => {
            let pass = fraction(
                "note-roulette",
                "pass",
                pass.map_or(0.6, |ast::Number(v)| v),
            )?;
            let replace = fraction(
                "note-roulette",
                "replace",
                replace.map_or(0.3, |ast::Number(v)| v),
            )?;
            if pass + replace > 1.0 {
                return Err(ConfigError::invalid(
                    "note-roulette",
                    format!(
                        "pass={pass} and replace={replace} must sum to at most 1, \
                         got {}",
                        pass + replace
                    ),
                ));
            }
            let (lo, hi) = key_range("note-roulette", lo, hi, DEFAULT_LO, DEFAULT_HI)?;
            EffectSpec::NoteRoulette {
                seed,
                pass,
                replace,
                lo,
                hi,
            }
        }
        E::VelocityDice {
            seed,
            lo,
            hi,
            sigma,
        } => match sigma {
            // sigma wins over lo/hi when both are given, like loose-keys.
            Some(ast::Number(sigma)) => EffectSpec::VelocityDiceGaussian {
                seed,
                sigma: float_range("velocity-dice", "sigma", sigma, 0.1, 40.0)?,
            },
            None => {
                let lo = velocity("velocity-dice", "lo", lo.unwrap_or(1))?;
                let hi = velocity("velocity-dice", "hi", hi.unwrap_or(127))?;
                ordered("velocity-dice", "lo", lo, "hi", hi)?;
                EffectSpec::VelocityDiceUniform { seed, lo, hi }
            }
        },
        E::DurationLottery {
            seed,
            mean,
            beats,
            min,
            max,
            spread,
        } => {
            let mean = time_spec_or("duration-lottery", "mean", mean, beats, 500.0)?;
            let min = match min {
                None => TimeSpec::Millis(30.0),
                Some(text) => duration("duration-lottery", "min", &text)?,
            };
            let max = match max {
                None => TimeSpec::Millis(4000.0),
                Some(text) => duration("duration-lottery", "max", &text)?,
            };
            // min and max are always absolute; the mean is comparable
            // here unless it was given in beats, in which case the CLI
            // finishes the check once the tempo resolves it.
            let (TimeSpec::Millis(min_ms), TimeSpec::Millis(max_ms)) = (min, max) else {
                unreachable!("min and max lower from duration strings");
            };
            if let TimeSpec::Millis(mean_ms) = mean {
                if min_ms > mean_ms {
                    return Err(ConfigError::invalid(
                        "duration-lottery",
                        format!("min={min_ms}ms must not exceed mean={mean_ms}ms"),
                    ));
                }
                if mean_ms > max_ms {
                    return Err(ConfigError::invalid(
                        "duration-lottery",
                        format!("mean={mean_ms}ms must not exceed max={max_ms}ms"),
                    ));
                }
            } else if min_ms > max_ms {
                return Err(ConfigError::invalid(
                    "duration-lottery",
                    format!("min={min_ms}ms must not exceed max={max_ms}ms"),
                ));
            }
            EffectSpec::DurationLottery {
                seed,
                mean,
                min,
                max,
                uniform: lottery_uniform(spread)?,
            }
        }
        E::DensityGovernor {
            target: ast::Number(target),
            window,
            beats,
            seed,
        } => EffectSpec::DensityGovernor {
            seed: seed.unwrap_or(0),
            target: float_range("density-governor", "target", target, 0.1, 100.0)?,
            window: time_spec_or("density-governor", "window", window, beats, 2000.0)?,
        },
        E::ClusterFist {
            width,
            kind,
            anchor,
            rolloff,
            sieve,
        } => EffectSpec::ClusterFist {
            kind: cluster_kind(kind, sieve)?,
            width: bounded("cluster-fist", "width", width.unwrap_or(4), 2, 12)?,
            anchor: cluster_anchor(anchor)?,
            rolloff: fraction(
                "cluster-fist",
                "rolloff",
                rolloff.map_or(0.8, |ast::Number(v)| v),
            )?,
        },
        E::ResonanceHalo {
            width,
            level,
            decay,
            beats,
            sieve,
        } => {
            if sieve.as_deref() == Some("") {
                return Err(ConfigError::invalid(
                    "resonance-halo",
                    "the sieve expression must not be empty",
                ));
            }
            EffectSpec::ResonanceHalo {
                width: bounded("resonance-halo", "width", width.unwrap_or(3), 1, 6)?,
                level: fraction(
                    "resonance-halo",
                    "level",
                    level.map_or(0.25, |ast::Number(v)| v),
                )?,
                decay: time_spec_or("resonance-halo", "decay", decay, beats, 3000.0)?,
                sieve,
            }
        }
        E::EuclideanGate {
            k,
            n,
            rotation,
            pulse,
            beats,
            mode,
        } => {
            let n = bounded("euclidean-gate", "n", n, 1, 64)?;
            let k = bounded("euclidean-gate", "k", k, 1, 64)?;
            ordered("euclidean-gate", "k", k, "n", n)?;
            let rotation = rotation.unwrap_or(0);
            if !(0..n as i64).contains(&rotation) {
                return Err(ConfigError::invalid(
                    "euclidean-gate",
                    format!("rotation must be less than n={n}, got {rotation}"),
                ));
            }
            EffectSpec::EuclideanGate {
                k,
                n,
                rotation: rotation as u8,
                pulse: time_spec_or_beats("euclidean-gate", "pulse", pulse, beats, 0.25)?,
                defer: gate_defer(mode)?,
            }
        }
        E::Quantize {
            grid,
            beats,
            strength,
        } => EffectSpec::Quantize {
            grid: time_spec_or_beats("quantize", "grid", grid, beats, 0.25)?,
            strength: fraction(
                "quantize",
                "strength",
                strength.map_or(1.0, |ast::Number(v)| v),
            )?,
        },
        E::Talea { durations, beats } => {
            if durations.is_empty() || durations.len() > 32 {
                return Err(ConfigError::invalid(
                    "talea",
                    format!(
                        "between 1 and 32 durations are required, got {}",
                        durations.len()
                    ),
                ));
            }
            // Entries are milliseconds unless beats=true reads them as
            // beat counts. Millisecond entries are range-checked here;
            // beat entries only once the CLI resolves them against the
            // tempo, since 1ms..=60s is an absolute constraint.
            let in_beats = beats.unwrap_or(false);
            let mut items = Vec::with_capacity(durations.len());
            for ast::Number(value) in durations {
                if in_beats {
                    if !(value.is_finite() && value > 0.0) {
                        return Err(ConfigError::invalid(
                            "talea",
                            format!(
                                "with beats=true entries must be finite and greater than 0, \
                                 got {value}"
                            ),
                        ));
                    }
                    items.push(TimeSpec::Beats(value));
                } else {
                    if !(value.is_finite() && (1.0..=60_000.0).contains(&value)) {
                        return Err(ConfigError::invalid(
                            "talea",
                            format!("each duration must be within 1ms..=60s, got {value}ms"),
                        ));
                    }
                    items.push(TimeSpec::Millis(value));
                }
            }
            EffectSpec::Talea { durations: items }
        }
        E::AddedValue {
            seed,
            unit,
            beats,
            extend,
            defer,
        } => EffectSpec::AddedValue {
            seed,
            unit: time_spec_or("added-value", "unit", unit, beats, 60.0)?,
            extend: fraction(
                "added-value",
                "extend",
                extend.map_or(0.3, |ast::Number(v)| v),
            )?,
            defer: fraction(
                "added-value",
                "defer",
                defer.map_or(0.0, |ast::Number(v)| v),
            )?,
        },
        E::AccentGroups {
            groups,
            accent,
            rest,
        } => {
            if groups.is_empty() {
                return Err(ConfigError::invalid(
                    "accent-groups",
                    "at least one group is required",
                ));
            }
            let groups = groups
                .into_iter()
                .map(|g| bounded("accent-groups", "group", g, 1, 16))
                .collect::<Result<Vec<u8>, _>>()?;
            EffectSpec::AccentGroups {
                groups,
                accent: velocity("accent-groups", "accent", accent.unwrap_or(112))?,
                rest: velocity("accent-groups", "rest", rest.unwrap_or(64))?,
            }
        }
        E::FeldmanField {
            seed,
            floor,
            ceiling,
            jitter,
        } => {
            let floor = velocity("feldman-field", "floor", floor.unwrap_or(8))?;
            let ceiling = velocity("feldman-field", "ceiling", ceiling.unwrap_or(28))?;
            ordered("feldman-field", "floor", floor, "ceiling", ceiling)?;
            EffectSpec::FeldmanField {
                seed: seed.unwrap_or(0),
                floor,
                ceiling,
                jitter: bounded("feldman-field", "jitter", jitter.unwrap_or(4), 0, 20)?,
            }
        }
        E::VelocityInvert { pivot } => EffectSpec::VelocityInvert {
            pivot: velocity("velocity-invert", "pivot", pivot.unwrap_or(64))?,
        },
        E::VelocityRouter {
            low,
            high,
            soft,
            medium,
            loud,
        } => {
            let low = velocity("velocity-router", "low", low.unwrap_or(64))?;
            let high = velocity("velocity-router", "high", high.unwrap_or(96))?;
            if low >= high {
                return Err(ConfigError::invalid(
                    "velocity-router",
                    format!("low={low} must be less than high={high}"),
                ));
            }
            EffectSpec::VelocityRouter {
                low,
                high,
                soft_ch: channel("velocity-router", soft)?,
                mid_ch: channel("velocity-router", medium)?,
                loud_ch: channel("velocity-router", loud)?,
            }
        }
        E::AntiAccent {
            seed,
            level,
            every,
            beats,
        } => {
            let every = time_spec_or("anti-accent", "every", every, beats, 30_000.0)?;
            at_least_a_second("anti-accent", "every", every)?;
            EffectSpec::AntiAccent {
                seed: seed.unwrap_or(0),
                level: velocity("anti-accent", "level", level.unwrap_or(30))?,
                every,
            }
        }
        E::MassCrescendo {
            period,
            beats,
            depth,
            shape,
        } => {
            let period = time_spec_or("mass-crescendo", "period", period, beats, 120_000.0)?;
            at_least_a_second("mass-crescendo", "period", period)?;
            EffectSpec::MassCrescendo {
                period,
                depth: fraction(
                    "mass-crescendo",
                    "depth",
                    depth.map_or(0.6, |ast::Number(v)| v),
                )?,
                shape: crescendo_shape(shape)?,
            }
        }
        E::Script { path, seed } => {
            if path.is_empty() {
                return Err(ConfigError::invalid(
                    "script",
                    "the script path must not be empty",
                ));
            }
            EffectSpec::Script {
                path,
                seed: seed.unwrap_or(0),
            }
        }
    })
}

/// Reject a `chain`/`fork` block nested past [`MAX_EFFECT_DEPTH`].
fn nesting(node: &'static str, depth: usize) -> Result<(), ConfigError> {
    if depth < MAX_EFFECT_DEPTH {
        Ok(())
    } else {
        Err(ConfigError::invalid(
            node,
            format!("effects nest deeper than the limit of {MAX_EFFECT_DEPTH} levels"),
        ))
    }
}

/// Resolve a time-valued parameter given as either a duration string
/// (`time="250ms"`) or a beat count (`beats=0.5`). Exactly one of the
/// two must be present.
fn time_spec(
    node: &'static str,
    prop: &str,
    time: Option<String>,
    beats: Option<ast::Number>,
) -> Result<TimeSpec, ConfigError> {
    match (time, beats.map(|ast::Number(b)| b)) {
        (Some(_), Some(_)) => Err(ConfigError::invalid(
            node,
            format!("{prop}= and beats= are mutually exclusive; give one"),
        )),
        (Some(text), None) => duration(node, prop, &text),
        (None, Some(beats)) => {
            if beats.is_finite() && beats > 0.0 {
                Ok(TimeSpec::Beats(beats))
            } else {
                Err(ConfigError::invalid(
                    node,
                    format!("beats must be finite and greater than 0, got {beats}"),
                ))
            }
        }
        (None, None) => Err(ConfigError::invalid(
            node,
            format!("expected either {prop}=\"250ms\" or beats=0.5"),
        )),
    }
}

/// Like [`time_spec`], but the property is optional: when neither the
/// duration string nor `beats=` is present, fall back to `default_ms`.
fn time_spec_or(
    node: &'static str,
    prop: &str,
    time: Option<String>,
    beats: Option<ast::Number>,
    default_ms: f64,
) -> Result<TimeSpec, ConfigError> {
    match (time, beats) {
        (None, None) => Ok(TimeSpec::Millis(default_ms)),
        (time, beats) => time_spec(node, prop, time, beats),
    }
}

/// Like [`time_spec_or`], but the fallback is a beat count rather than
/// an absolute time.
fn time_spec_or_beats(
    node: &'static str,
    prop: &str,
    time: Option<String>,
    beats: Option<ast::Number>,
    default_beats: f64,
) -> Result<TimeSpec, ConfigError> {
    match (time, beats) {
        (None, None) => Ok(TimeSpec::Beats(default_beats)),
        (time, beats) => time_spec(node, prop, time, beats),
    }
}

/// A duration that must come to at least one second. Only the absolute
/// form is checkable here; the CLI re-checks once the tempo resolves a
/// `beats=` value.
fn at_least_a_second(node: &'static str, prop: &str, time: TimeSpec) -> Result<(), ConfigError> {
    match time {
        TimeSpec::Millis(ms) if ms < 1000.0 => Err(ConfigError::invalid(
            node,
            format!("{prop} must be at least 1s, got {ms}ms"),
        )),
        _ => Ok(()),
    }
}

/// Parse a duration string: digits with an optional decimal point,
/// suffixed `ms` or `s`.
fn duration(node: &'static str, prop: &str, text: &str) -> Result<TimeSpec, ConfigError> {
    let bad = || {
        ConfigError::invalid(
            node,
            format!("{prop} must be a duration like \"250ms\" or \"1.5s\", got \"{text}\""),
        )
    };
    let (number, scale) = if let Some(number) = text.strip_suffix("ms") {
        (number, 1.0)
    } else if let Some(number) = text.strip_suffix('s') {
        (number, 1000.0)
    } else {
        return Err(bad());
    };
    // f64's grammar is wider than a duration's: no signs, exponents, or
    // named specials here, just digits and at most one point.
    if !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(bad());
    }
    let value: f64 = number.parse().map_err(|_| bad())?;
    if value > 0.0 {
        Ok(TimeSpec::Millis(value * scale))
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{prop} must be a positive duration, got \"{text}\""),
        ))
    }
}

/// Whether a decay of exactly 1 is acceptable.
#[derive(PartialEq)]
enum OneIs {
    /// Echo may repeat at constant volume.
    Allowed,
    /// Restrike must fade toward its floor.
    Excluded,
}

/// A per-repeat velocity decay factor in (0, 1] or (0, 1).
fn decay_factor(node: &'static str, value: f64, one: OneIs) -> Result<f32, ConfigError> {
    let below_top = value < 1.0 || (one == OneIs::Allowed && value == 1.0);
    if value.is_finite() && value > 0.0 && below_top {
        Ok(value as f32)
    } else {
        let top = match one {
            OneIs::Allowed => "at most 1",
            OneIs::Excluded => "less than 1",
        };
        Err(ConfigError::invalid(
            node,
            format!("decay must be greater than 0 and {top}, got {value}"),
        ))
    }
}

/// An integer property confined to `lo..=hi`.
fn bounded(node: &'static str, prop: &str, value: i64, lo: u8, hi: u8) -> Result<u8, ConfigError> {
    if (lo as i64..=hi as i64).contains(&value) {
        Ok(value as u8)
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{prop} must be within {lo}..={hi}, got {value}"),
        ))
    }
}

/// Resolve a `lo=`/`hi=` pair of key properties: apply defaults, check each
/// key is a valid MIDI key, and check the pair is ordered.
fn key_range(
    node: &'static str,
    lo: Option<i64>,
    hi: Option<i64>,
    default_lo: u8,
    default_hi: u8,
) -> Result<(u8, u8), ConfigError> {
    let lo = key(node, "lo", lo.unwrap_or(default_lo as i64))?;
    let hi = key(node, "hi", hi.unwrap_or(default_hi as i64))?;
    ordered(node, "lo", lo, "hi", hi)?;
    Ok((lo, hi))
}

/// A MIDI key number, 0..=127.
fn key(node: &'static str, prop: &str, value: i64) -> Result<u8, ConfigError> {
    if (0..=127).contains(&value) {
        Ok(value as u8)
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{prop} must be a key within 0..=127, got {value}"),
        ))
    }
}

/// A note-on velocity, 1..=127 (0 means note-off on the wire).
fn velocity(node: &'static str, prop: &str, value: i64) -> Result<u8, ConfigError> {
    if (1..=127).contains(&value) {
        Ok(value as u8)
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{prop} must be a velocity within 1..=127, got {value}"),
        ))
    }
}

/// A channel as humans write it, 1..=16, rebased to the wire's 0..=15.
fn channel(node: &'static str, value: i64) -> Result<u8, ConfigError> {
    if (1..=16).contains(&value) {
        Ok((value - 1) as u8)
    } else {
        Err(ConfigError::invalid(
            node,
            format!("channels are 1..=16, got {value}"),
        ))
    }
}

fn ordered(
    node: &'static str,
    lo_name: &str,
    lo: u8,
    hi_name: &str,
    hi: u8,
) -> Result<(), ConfigError> {
    if lo <= hi {
        Ok(())
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{lo_name}={lo} must not exceed {hi_name}={hi}"),
        ))
    }
}

/// A fraction property, 0..=1 and finite.
fn fraction(node: &'static str, prop: &str, value: f64) -> Result<f32, ConfigError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value as f32)
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{prop} must be within 0..=1, got {value}"),
        ))
    }
}

/// A float property confined to `lo..=hi` and finite.
fn float_range(
    node: &'static str,
    prop: &str,
    value: f64,
    lo: f64,
    hi: f64,
) -> Result<f32, ConfigError> {
    if value.is_finite() && (lo..=hi).contains(&value) {
        Ok(value as f32)
    } else {
        Err(ConfigError::invalid(
            node,
            format!("{prop} must be within {lo}..={hi}, got {value}"),
        ))
    }
}

/// A `row-snap` row: exactly 12 arguments, together a permutation of the
/// pitch classes 0..=11. The error for a broken permutation names what is
/// duplicated and what is missing.
fn tone_row(row: Vec<i64>) -> Result<[u8; 12], ConfigError> {
    if row.len() != 12 {
        return Err(ConfigError::invalid(
            "row-snap",
            format!("a row is exactly 12 pitch classes, got {}", row.len()),
        ));
    }
    let mut counts = [0u8; 12];
    let mut fixed = [0u8; 12];
    for (slot, &value) in fixed.iter_mut().zip(&row) {
        if !(0..=11).contains(&value) {
            return Err(ConfigError::invalid(
                "row-snap",
                format!("row entries are pitch classes within 0..=11, got {value}"),
            ));
        }
        counts[value as usize] += 1;
        *slot = value as u8;
    }
    let list = |pred: fn(u8) -> bool| {
        counts
            .iter()
            .enumerate()
            .filter(|&(_, &n)| pred(n))
            .map(|(pc, _)| pc.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if counts.iter().any(|&n| n != 1) {
        return Err(ConfigError::invalid(
            "row-snap",
            format!(
                "the row must use every pitch class exactly once; \
                 duplicated: {}; missing: {}",
                list(|n| n > 1),
                list(|n| n == 0),
            ),
        ));
    }
    Ok(fixed)
}

fn row_form(form: Option<String>) -> Result<RowForm, ConfigError> {
    match form.as_deref() {
        None | Some("p") => Ok(RowForm::Prime),
        Some("i") => Ok(RowForm::Inversion),
        Some("r") => Ok(RowForm::Retrograde),
        Some("ri") => Ok(RowForm::RetrogradeInversion),
        Some(other) => Err(ConfigError::invalid(
            "row-snap",
            format!("form must be \"p\", \"i\", \"r\", or \"ri\", got \"{other}\""),
        )),
    }
}

/// The `snap=` property shared by `sieve` and `mode-lock`: what to do
/// with a key that is off the grid.
fn sieve_snap(node: &'static str, snap: Option<String>) -> Result<SieveSnap, ConfigError> {
    match snap.as_deref() {
        None | Some("nearest") => Ok(SieveSnap::Nearest),
        Some("up") => Ok(SieveSnap::Up),
        Some("down") => Ok(SieveSnap::Down),
        Some("drop") => Ok(SieveSnap::Drop),
        Some(other) => Err(ConfigError::invalid(
            node,
            format!("snap must be \"nearest\", \"up\", \"down\", or \"drop\", got \"{other}\""),
        )),
    }
}

/// A pitch class written as a note name or a number: a letter a-g with
/// an optional `#` or `b` accidental (`"c"`, `"f#"`, `"bb"`, case does
/// not matter), or a numeric string `"0"`..`"11"`.
fn pitch_class(node: &'static str, prop: &str, text: &str) -> Result<u8, ConfigError> {
    let bad = || {
        ConfigError::invalid(
            node,
            format!(
                "{prop} must be a note name like \"c\", \"f#\", or \"bb\", \
                 or a pitch class \"0\"..\"11\", got \"{text}\""
            ),
        )
    };
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) {
        return match text.parse::<u8>() {
            Ok(pc) if pc <= 11 => Ok(pc),
            _ => Err(bad()),
        };
    }
    let lower = text.to_ascii_lowercase();
    let mut chars = lower.chars();
    let base: i8 = match chars.next() {
        Some('c') => 0,
        Some('d') => 2,
        Some('e') => 4,
        Some('f') => 5,
        Some('g') => 7,
        Some('a') => 9,
        Some('b') => 11,
        _ => return Err(bad()),
    };
    let shift: i8 = match chars.as_str() {
        "" => 0,
        "#" => 1,
        "b" => -1,
        _ => return Err(bad()),
    };
    Ok((base + shift).rem_euclid(12) as u8)
}

fn t_direction(direction: Option<String>) -> Result<TDirection, ConfigError> {
    match direction.as_deref() {
        None | Some("superior") => Ok(TDirection::Superior),
        Some("inferior") => Ok(TDirection::Inferior),
        Some("alternating") => Ok(TDirection::Alternating),
        Some(other) => Err(ConfigError::invalid(
            "tintinnabuli",
            format!(
                "direction must be \"superior\", \"inferior\", or \"alternating\", \
                 got \"{other}\""
            ),
        )),
    }
}

/// `negative-harmony`'s mode: `"replace"` (the default) swaps the note
/// for its mirror, `"add"` sounds both.
fn negative_harmony_add(mode: Option<String>) -> Result<bool, ConfigError> {
    match mode.as_deref() {
        None | Some("replace") => Ok(false),
        Some("add") => Ok(true),
        Some(other) => Err(ConfigError::invalid(
            "negative-harmony",
            format!("mode must be \"replace\" or \"add\", got \"{other}\""),
        )),
    }
}

/// A `tonnetz` move sequence: a non-empty string of the letters p, l,
/// and r, case-insensitive, read left to right.
fn plr_sequence(text: &str) -> Result<Vec<Plr>, ConfigError> {
    if text.is_empty() {
        return Err(ConfigError::invalid(
            "tonnetz",
            "the sequence must not be empty",
        ));
    }
    text.chars()
        .map(|c| match c.to_ascii_lowercase() {
            'p' => Ok(Plr::P),
            'l' => Ok(Plr::L),
            'r' => Ok(Plr::R),
            other => Err(ConfigError::invalid(
                "tonnetz",
                format!("the sequence uses only the letters p, l, and r, got '{other}'"),
            )),
        })
        .collect()
}

/// Resolve a `cluster-fist` kind and its `sieve=` companion: the sieve
/// expression is required exactly when the kind is `"sieve"`, and only
/// checked for non-emptiness here (the CLI parses it at build).
fn cluster_kind(kind: Option<String>, sieve: Option<String>) -> Result<ClusterKind, ConfigError> {
    let kind = match kind.as_deref() {
        None | Some("chromatic") => ClusterKind::Chromatic,
        Some("white") => ClusterKind::White,
        Some("black") => ClusterKind::Black,
        Some("sieve") => match sieve {
            Some(expr) if !expr.is_empty() => return Ok(ClusterKind::Sieve(expr)),
            Some(_) => {
                return Err(ConfigError::invalid(
                    "cluster-fist",
                    "the sieve expression must not be empty",
                ));
            }
            None => {
                return Err(ConfigError::invalid(
                    "cluster-fist",
                    "kind=\"sieve\" requires a sieve=\"...\" expression",
                ));
            }
        },
        Some(other) => {
            return Err(ConfigError::invalid(
                "cluster-fist",
                format!(
                    "kind must be \"chromatic\", \"white\", \"black\", or \"sieve\", \
                     got \"{other}\""
                ),
            ));
        }
    };
    if sieve.is_some() {
        return Err(ConfigError::invalid(
            "cluster-fist",
            "sieve= only applies with kind=\"sieve\"",
        ));
    }
    Ok(kind)
}

fn cluster_anchor(anchor: Option<String>) -> Result<ClusterAnchor, ConfigError> {
    match anchor.as_deref() {
        None | Some("center") => Ok(ClusterAnchor::Center),
        Some("bottom") => Ok(ClusterAnchor::Bottom),
        Some("top") => Ok(ClusterAnchor::Top),
        Some(other) => Err(ConfigError::invalid(
            "cluster-fist",
            format!("anchor must be \"bottom\", \"center\", or \"top\", got \"{other}\""),
        )),
    }
}

/// `duration-lottery`'s spread: `"exp"` (the default) or `"uniform"`.
fn lottery_uniform(spread: Option<String>) -> Result<bool, ConfigError> {
    match spread.as_deref() {
        None | Some("exp") => Ok(false),
        Some("uniform") => Ok(true),
        Some(other) => Err(ConfigError::invalid(
            "duration-lottery",
            format!("spread must be \"exp\" or \"uniform\", got \"{other}\""),
        )),
    }
}

/// `euclidean-gate`'s off-pulse handling: `"defer"` (the default) holds
/// the note for the next sounding step, `"drop"` discards it.
fn gate_defer(mode: Option<String>) -> Result<bool, ConfigError> {
    match mode.as_deref() {
        None | Some("defer") => Ok(true),
        Some("drop") => Ok(false),
        Some(other) => Err(ConfigError::invalid(
            "euclidean-gate",
            format!("mode must be \"defer\" or \"drop\", got \"{other}\""),
        )),
    }
}

fn crescendo_shape(shape: Option<String>) -> Result<CrescendoShape, ConfigError> {
    match shape.as_deref() {
        None | Some("arch") => Ok(CrescendoShape::Arch),
        Some("ramp") => Ok(CrescendoShape::Ramp),
        Some(other) => Err(ConfigError::invalid(
            "mass-crescendo",
            format!("shape must be \"ramp\" or \"arch\", got \"{other}\""),
        )),
    }
}

fn klangfarben_random(mode: Option<String>) -> Result<bool, ConfigError> {
    match mode.as_deref() {
        None | Some("cycle") => Ok(false),
        Some("random") => Ok(true),
        Some(other) => Err(ConfigError::invalid(
            "klangfarben",
            format!("mode must be \"cycle\" or \"random\", got \"{other}\""),
        )),
    }
}

fn shuffle_mode(mode: Option<String>) -> Result<ShuffleMode, ConfigError> {
    match mode.as_deref() {
        None | Some("free") => Ok(ShuffleMode::Free),
        Some("within-octave") => Ok(ShuffleMode::WithinOctave),
        Some("within-pitch-class") => Ok(ShuffleMode::WithinPitchClass),
        Some(other) => Err(ConfigError::invalid(
            "shuffle-lock",
            format!(
                "mode must be \"free\", \"within-octave\", or \"within-pitch-class\", \
                 got \"{other}\""
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::pitch_class;

    fn pc(text: &str) -> u8 {
        pitch_class("test-node", "root", text).expect("note name should parse")
    }

    fn pc_err(text: &str) -> String {
        pitch_class("test-node", "root", text)
            .expect_err("note name should not parse")
            .to_string()
    }

    #[test]
    fn every_natural_letter_maps_to_its_pitch_class() {
        assert_eq!(pc("c"), 0);
        assert_eq!(pc("d"), 2);
        assert_eq!(pc("e"), 4);
        assert_eq!(pc("f"), 5);
        assert_eq!(pc("g"), 7);
        assert_eq!(pc("a"), 9);
        assert_eq!(pc("b"), 11);
    }

    #[test]
    fn sharps_and_flats_are_enharmonic() {
        assert_eq!(pc("c#"), pc("db"));
        assert_eq!(pc("d#"), pc("eb"));
        assert_eq!(pc("f#"), pc("gb"));
        assert_eq!(pc("g#"), pc("ab"));
        assert_eq!(pc("a#"), pc("bb"));
        assert_eq!(pc("c#"), 1);
        assert_eq!(pc("bb"), 10);
        // The wrapping enharmonics too.
        assert_eq!(pc("cb"), 11);
        assert_eq!(pc("b#"), 0);
        assert_eq!(pc("e#"), 5);
        assert_eq!(pc("fb"), 4);
    }

    #[test]
    fn case_does_not_matter() {
        assert_eq!(pc("F#"), 6);
        assert_eq!(pc("Db"), 1);
        assert_eq!(pc("DB"), 1);
        assert_eq!(pc("A"), 9);
    }

    #[test]
    fn numeric_pitch_classes_parse() {
        for n in 0..=11u8 {
            assert_eq!(pc(&n.to_string()), n);
        }
    }

    #[test]
    fn bad_note_names_are_rejected_with_the_accepted_forms() {
        for bad in ["h", "cb#", "c##", "12", "99", "", "#", "b b", "-1", "1a"] {
            let msg = pc_err(bad);
            assert!(
                msg.contains("test-node")
                    && msg.contains("root")
                    && msg.contains("f#")
                    && msg.contains("0")
                    && msg.contains("11"),
                "error for {bad:?} should name the property and the accepted forms: {msg}"
            );
        }
        assert!(pc_err("h").contains("\"h\""), "the value is echoed back");
    }
}
