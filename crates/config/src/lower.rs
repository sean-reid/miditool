//! Lowering from the raw [`ast`](crate::ast) shapes into the public spec
//! types: defaults filled in, ranges checked, channels rebased from the
//! human 1-16 to the wire 0-15.

use crate::ast;
use crate::{Config, ConfigError, EffectSpec, OutputSpec, ShuffleMode};

/// Output port name used when the config has no `output` node.
const DEFAULT_OUTPUT: &str = "miditool Out";

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
        chain: effects(doc.effects)?,
    })
}

fn effects(nodes: Vec<ast::Effect>) -> Result<Vec<EffectSpec>, ConfigError> {
    nodes.into_iter().map(effect).collect()
}

fn effect(node: ast::Effect) -> Result<EffectSpec, ConfigError> {
    use ast::Effect as E;
    Ok(match node {
        E::Chain { children } => EffectSpec::Chain(effects(children)?),
        E::Fork { children } => EffectSpec::Fork(effects(children)?),
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
    })
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
