//! Lowering from the raw [`ast`](crate::ast) shapes into the public spec
//! types: defaults filled in, ranges checked, channels rebased from the
//! human 1-16 to the wire 0-15.

use std::net::IpAddr;

use crate::ast;
use crate::{
    Config, ConfigError, EffectSpec, OutputSpec, RemoteSpec, SceneSpec, ShuffleMode, TimeSpec,
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
            Some(sigma) => {
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
            let gamma = gamma.unwrap_or(1.0);
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
                decay: decay_factor("echo", decay.unwrap_or(0.6), OneIs::Allowed)?,
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
            let jitter = jitter.unwrap_or(0.15);
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
                decay: decay_factor("restrike", decay.unwrap_or(0.7), OneIs::Excluded)?,
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
            let curve = curve.unwrap_or(1.0);
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
