//! KDL configuration for miditool.
//!
//! A config file is a KDL 2 document: an optional `input` node, an optional
//! `output` node, and a sequence of effect nodes forming an implicit chain:
//!
//! ```kdl
//! input "Roland"
//! output virtual="miditool Out"
//!
//! shuffle-lock seed=42 lo=21 hi=108 mode="free"
//! velocity-curve gamma=0.8
//! ```
//!
//! Parsing produces a plain [`Config`] of [`EffectSpec`] values. This crate
//! knows nothing about the runtime effect graph; the CLI maps specs onto
//! `miditool-core` nodes.
//!
//! Channels are 1-16 in config files, matching what keyboards print on
//! their panels, and 0-15 in the parsed spec, matching the wire format.

mod ast;
mod lower;

use std::path::{Path, PathBuf};

/// A fully validated configuration: where to read, where to write, and
/// what to do in between.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Substring to match against input port names; `None` means prompt
    /// or pick a default.
    pub input: Option<String>,
    /// Where processed events go.
    pub output: OutputSpec,
    /// Top-level effects, run in series.
    pub chain: Vec<EffectSpec>,
}

/// Output port selection.
///
/// Defaults to `Virtual("miditool Out")` when the config has no `output`
/// node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSpec {
    /// Create a virtual port with this exact name.
    Virtual(String),
    /// Connect to an existing port whose name contains this substring.
    Device(String),
}

/// How `shuffle-lock` is allowed to permute keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleMode {
    /// Any key may map to any other key in the range.
    Free,
    /// Keys stay within their octave.
    WithinOctave,
    /// Keys keep their pitch class and move between octaves.
    WithinPitchClass,
}

/// One effect node, validated and ready to compile into the effect graph.
///
/// Ranges are inclusive throughout. Channels are stored 0-based (the wire
/// format) even though config files write them 1-based.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectSpec {
    /// Run children in series.
    Chain(Vec<EffectSpec>),
    /// Run children in parallel and merge their outputs.
    Fork(Vec<EffectSpec>),
    /// The identity effect.
    Pass,
    /// Drop every event.
    Discard,
    /// Shift notes by a fixed number of semitones.
    Transpose { semis: i16 },
    /// Deterministically permute the keyboard.
    ShuffleLock {
        seed: u64,
        lo: u8,
        hi: u8,
        mode: ShuffleMode,
    },
    /// Replace each played key with a uniform draw from `lo..=hi`.
    LooseKeysUniform { seed: u64, lo: u8, hi: u8 },
    /// Replace each played key with a gaussian draw around the played key.
    LooseKeysGaussian { seed: u64, sigma: f32 },
    /// Reshape note-on velocities: `v -> (v/127)^gamma * 127`, clamped to
    /// `floor..=ceiling`.
    VelocityCurve { gamma: f32, floor: u8, ceiling: u8 },
    /// Rewrite every event onto one channel (0-based).
    Channelize { ch: u8 },
    /// Pass only events on these channels (0-based, sorted, deduplicated).
    OnlyChannels(Vec<u8>),
    /// Pass only notes with keys in `lo..=hi`; non-note events flow through.
    KeyRange { lo: u8, hi: u8 },
    /// Pass only note-ons with velocities in `lo..=hi`; everything else
    /// flows through.
    VelocityRange { lo: u8, hi: u8 },
    /// Pass only note and poly-pressure events.
    NotesOnly,
    /// Pass only controller events.
    ControllersOnly,
}

/// Everything that can go wrong between a path and a [`Config`].
///
/// Parse and decode failures wrap [`knus::Error`]; their `Display` output
/// is the full miette report, source snippets included, so the message is
/// useful even without a fancy error hook installed. The [`miette::Diagnostic`]
/// impl forwards to knus for callers that render reports themselves.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum ConfigError {
    /// The KDL text failed to parse or decode.
    #[error("{}", render_report(.0))]
    #[diagnostic(transparent)]
    Parse(#[from] knus::Error),

    /// The document parsed, but a value violates a constraint.
    #[error("{node}: {message}")]
    Invalid {
        /// The offending node's name.
        node: &'static str,
        /// What the constraint is and what was found instead.
        message: String,
    },

    /// The config file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ConfigError {
    fn invalid(node: &'static str, message: impl Into<String>) -> Self {
        ConfigError::Invalid {
            node,
            message: message.into(),
        }
    }
}

/// Render a knus error as miette's graphical report, without colors so the
/// text is clean wherever `Display` ends up.
fn render_report(err: &knus::Error) -> String {
    let handler =
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor());
    let mut out = String::new();
    match handler.render_report(&mut out, err) {
        Ok(()) => out,
        Err(_) => err.to_string(),
    }
}

/// Parse a config from KDL text. `source_name` labels the source in error
/// reports; use the file path or something like `"<inline>"`.
pub fn parse_str(source_name: &str, text: &str) -> Result<Config, ConfigError> {
    let doc: ast::Document = knus::parse(source_name, text)?;
    lower::document(doc)
}

/// Read and parse a config file.
pub fn parse_file(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_str(&path.display().to_string(), &text)
}
