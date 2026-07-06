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
//! Effects can instead be grouped into named `scene` blocks, each carrying
//! its own chain; bare effects lower to a single scene named "main". The
//! two styles do not mix. An optional `remote` node opens the phone/tablet
//! web remote for switching scenes; it binds loopback unless `bind=` says
//! otherwise:
//!
//! ```kdl
//! remote port=8320 bind="0.0.0.0"
//!
//! scene "scrambled" {
//!     shuffle-lock seed=42
//! }
//! scene "echo storm" switch="kill" {
//!     echo repeats=6 time="300ms" decay=0.8
//! }
//! ```
//!
//! Besides the built-in effects, a chain can hold a `script` node
//! (`script "wedge.lua" seed=42`) that runs a Luau file on every event.
//! The path stays verbatim in the spec; the CLI resolves and loads it.
//!
//! Parsing produces a plain [`Config`] of [`SceneSpec`] and [`EffectSpec`]
//! values. This crate knows nothing about the runtime effect graph; the
//! CLI maps specs onto `miditool-core` nodes.
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
    /// Hide the raw input source from other apps while running, so a DAW
    /// that listens to every port (GarageBand) hears only the output.
    /// macOS only; ignored elsewhere.
    pub hide_input: bool,
    /// Where processed events go.
    pub output: OutputSpec,
    /// Beats per minute, from the top-level `tempo` node; resolves the
    /// `beats=` form of [`TimeSpec`]. Defaults to 120.
    pub tempo: f32,
    /// The phone/tablet web remote, from the top-level `remote` node;
    /// `None` leaves the remote off.
    pub remote: Option<RemoteSpec>,
    /// The scenes, in file order; always at least one. Bare top-level
    /// effects lower to a single scene named "main".
    pub scenes: Vec<SceneSpec>,
}

/// The web remote's listen address, from the `remote` node.
///
/// The default bind is loopback, so the remote is reachable only from
/// the machine running miditool; `bind="0.0.0.0"` opens it to the local
/// network for a phone or tablet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteSpec {
    /// TCP port to serve on, `1..=65535`.
    pub port: u16,
    /// Address to bind; defaults to `127.0.0.1`.
    pub bind: std::net::IpAddr,
}

/// One named scene: a chain of effects, and what happens to sounding
/// notes when the scene is switched away from.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpec {
    /// The scene's name, unique within the config, compared as written
    /// (case matters). Bare configs get a single scene named "main".
    pub name: String,
    /// Cut sounding notes when leaving the scene (`switch="kill"`) rather
    /// than letting them ring out (`switch="let-ring"`, the default).
    pub kill_on_exit: bool,
    /// The scene's effects, run in series. An empty chain passes events
    /// through.
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

/// A duration, absolute or tempo-relative.
///
/// Time-based effects write either a duration string (`time="250ms"`,
/// `time="1.5s"`) or a beat count (`beats=0.5`). The spec keeps the
/// distinction; [`TimeSpec::to_nanos`] resolves it against the config's
/// tempo when the graph is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeSpec {
    /// An absolute time in milliseconds.
    Millis(f64),
    /// A count of beats; one beat lasts `60 / bpm` seconds.
    Beats(f64),
}

impl TimeSpec {
    /// Resolve to nanoseconds at the given tempo. Only the `Beats` form
    /// consults the tempo.
    pub fn to_nanos(self, tempo_bpm: f32) -> u64 {
        match self {
            TimeSpec::Millis(ms) => (ms * 1e6).round() as u64,
            TimeSpec::Beats(beats) => (beats * 60e9 / tempo_bpm as f64).round() as u64,
        }
    }
}

/// The MPE zone shared by the microtonal effects (`spectral-halo`,
/// `just`, `scordatura`, `overtone-pedal`), from their `channels=` and
/// `bend-range=` properties.
///
/// These effects speak MPE: each note goes out alone on one of the
/// member channels `lo..=hi` with a per-note pitch bend, and the bend
/// only lands right when the receiving instrument's pitch-bend range is
/// set to `bend_range` semitones. Channels are stored 0-based (the wire
/// format) even though config files write them 1-based.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MpeSpec {
    /// Lowest member channel, inclusive.
    pub lo: u8,
    /// Highest member channel, inclusive.
    pub hi: u8,
    /// Pitch-bend range in semitones, `1..=96`.
    pub bend_range: f32,
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

/// How `row-snap` reads its tone row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowForm {
    /// The row as written.
    Prime,
    /// Intervals flipped around the first pitch class.
    Inversion,
    /// The row read backwards.
    Retrograde,
    /// The inversion read backwards.
    RetrogradeInversion,
}

/// Where `tintinnabuli` places the triad voice relative to the melody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TDirection {
    /// The nearest triad tone above the melody note.
    Superior,
    /// The nearest triad tone below.
    Inferior,
    /// Above and below by turns, note for note.
    Alternating,
}

/// One neo-Riemannian move in a `tonnetz` sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plr {
    /// Parallel: swap major and minor over the same root.
    P,
    /// Leittonwechsel: move the root by a semitone.
    L,
    /// Relative: exchange a triad for its relative major or minor.
    R,
}

/// How `sieve` and `mode-lock` handle a key that is off the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SieveSnap {
    /// Move to the closest sieve key.
    Nearest,
    /// Move to the next sieve key above.
    Up,
    /// Move to the next sieve key below.
    Down,
    /// Drop the note.
    Drop,
}

/// What keys a `cluster-fist` cluster is built from.
///
/// The `Sieve` form carries its expression as written; the CLI parses it
/// against `miditool-core`'s sieve grammar when it builds the graph, so
/// this crate only checks that it is non-empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterKind {
    /// Every key.
    Chromatic,
    /// White keys only.
    White,
    /// Black keys only.
    Black,
    /// Keys on a Xenakis sieve, kept as the written expression.
    Sieve(String),
}

/// Where a `cluster-fist` cluster sits relative to the played key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterAnchor {
    /// The played key is the cluster's lowest note.
    Bottom,
    /// The cluster spreads evenly around the played key.
    Center,
    /// The played key is the cluster's highest note.
    Top,
}

/// The order `continuum` cycles through the held keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuumOrder {
    /// Bottom to top, then around again.
    Up,
    /// Top to bottom.
    Down,
    /// The order the keys were pressed.
    Played,
    /// A seeded random pick for every slot.
    Random,
}

/// The shape of a `mass-crescendo` swell over one period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrescendoShape {
    /// Rise across the period, then reset.
    Ramp,
    /// Rise to the middle of the period, then fall back.
    Arch,
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
    /// Hold every event back by a fixed time.
    Delay { time: TimeSpec },
    /// Repeat each note `repeats` times, `time` apart, each repeat
    /// `decay` times softer and shifted by `transpose` semitones.
    Echo {
        repeats: u8,
        time: TimeSpec,
        decay: f32,
        transpose: i16,
    },
    /// Re-strike held notes on a jittered interval, each strike `decay`
    /// times softer, never below velocity `floor`, at most `max` times
    /// per note.
    Restrike {
        seed: u64,
        interval: TimeSpec,
        jitter: f32,
        decay: f32,
        floor: u8,
        max: u8,
    },
    /// Ratchet each note-on into a burst of `repeats` hits: the first
    /// gap lasts `first`, later gaps scale by `curve`.
    Stutter {
        repeats: u8,
        first: TimeSpec,
        curve: f32,
    },
    /// Keep each note's pitch class but move it to a seeded random
    /// octave within `lo..=hi`.
    RegistralScatter { seed: u64, lo: u8, hi: u8 },
    /// Reflect notes around the `axis` key, each note with the given
    /// probability (seeded; 1 mirrors everything).
    WedgeMirror {
        axis: u8,
        probability: f32,
        seed: u64,
    },
    /// Drop notes on the listed keys (sorted, deduplicated). With
    /// `by_class` the entries are pitch classes 0..=11 and every octave
    /// of them is blocked.
    BlockedKeys { keys: Vec<u8>, by_class: bool },
    /// Deal successive notes across channels (0-based, in the order
    /// written): round-robin, or a seeded random pick when `random`.
    Klangfarben {
        channels: Vec<u8>,
        random: bool,
        seed: u64,
    },
    /// Ring modulation for keys: each note becomes its sum and/or
    /// difference with the `carrier` key; `dry` keeps the original too.
    /// At least one of the three flags is true.
    RingMod {
        carrier: u8,
        sum: bool,
        diff: bool,
        dry: bool,
    },
    /// Scale each note's distance from the `reference` key by `factor`:
    /// above 1 stretches intervals, below 1 compresses them.
    Telescope { factor: f32, reference: u8 },
    /// Snap notes onto a twelve-tone row (a permutation of the pitch
    /// classes 0..=11), read in the given form and shifted by
    /// `transpose` semitones.
    RowSnap {
        row: [u8; 12],
        form: RowForm,
        transpose: i8,
    },
    /// Let each pitch class sound once per aggregate: repeats are gated
    /// until all twelve classes have arrived, except a seeded `leak`
    /// fraction that slips through early.
    AggregateGate { leak: f32, seed: u64 },
    /// Quantize keys onto a Xenakis sieve: `sieve "8@0|8@3" snap="up"`.
    /// The expression is kept as written; the CLI parses it against
    /// `miditool-core`'s sieve grammar when it builds the graph, so this
    /// crate only checks that it is non-empty.
    Sieve { expr: String, snap: SieveSnap },
    /// Pärt's tintinnabuli: each melody note brings a companion from
    /// the tonic triad, the `position`th triad tone above (`Superior`),
    /// below (`Inferior`), or by turns (`Alternating`), at `level` of
    /// the played velocity. `root` is a pitch class 0..=11.
    Tintinnabuli {
        root: u8,
        minor: bool,
        position: u8,
        direction: TDirection,
        level: f32,
    },
    /// Snap keys onto one of the seven church modes (1 Ionian through
    /// 7 Locrian), shifted up `transposition` semitones; off-mode keys
    /// snap like `sieve` does.
    ModeLock {
        mode: u8,
        transposition: u8,
        snap: SieveSnap,
    },
    /// Reflect notes through the negative-harmony axis of the `tonic`
    /// (a pitch class 0..=11): the mirror replaces the note, or joins
    /// it at `level` of the played velocity when `add`.
    NegativeHarmony { tonic: u8, add: bool, level: f32 },
    /// Walk the neo-Riemannian Tonnetz: from the `start` triad (major,
    /// or minor when `minor`), each note-on takes the next step in the
    /// `sequence` and sounds the arrived-at triad within `lo..=hi`;
    /// `include_played` keeps the played note too.
    Tonnetz {
        start: u8,
        minor: bool,
        sequence: Vec<Plr>,
        lo: u8,
        hi: u8,
        include_played: bool,
    },
    /// Sound what is not played: a quiet pad, velocity `vel`, of the
    /// pitch classes missing from the held notes, voiced within
    /// `lo..=hi` and revoiced as the harmony changes.
    ComplementPad { lo: u8, hi: u8, vel: u8 },
    /// Bloom each note into its first `partials` overtones, each one
    /// `rolloff` times softer than the last, the series stretched
    /// (`stretch` above 1) or squeezed (below 1) away from the pure
    /// harmonic ladder. The partials land between the keys, so the
    /// notes go out over the MPE zone with per-note pitch bends.
    SpectralHalo {
        partials: u8,
        rolloff: f32,
        stretch: f32,
        mpe: MpeSpec,
    },
    /// Bend every note onto five-limit just intonation around `root`
    /// (a pitch class 0..=11), so thirds and fifths lock pure; the
    /// retuning rides per-note pitch bends over the MPE zone.
    Just { root: u8, mpe: MpeSpec },
    /// Retune chosen pitch classes by cents: `cents[pc]` is the offset
    /// within -100..=100 for pitch class `pc`, 0 for the classes the
    /// config leaves alone. The detuning rides per-note pitch bends
    /// over the MPE zone.
    Scordatura { cents: [i16; 12], mpe: MpeSpec },
    /// A resonating pedal on the `fundamental` key: each note snaps to
    /// the nearest partial of the fundamental, up to `max_partial`,
    /// tuned exactly with a per-note pitch bend over the MPE zone.
    OvertonePedal {
        fundamental: u8,
        max_partial: u8,
        mpe: MpeSpec,
    },
    /// Spray a seeded Poisson cloud of grains from each note-on:
    /// `density` grains per second for `duration`, pitches spread
    /// `sigma` semitones and velocities `vel_sigma` steps (both
    /// Gaussian) around the played note, at most `max` grains.
    PoissonCloud {
        seed: u64,
        density: f32,
        duration: TimeSpec,
        sigma: f32,
        vel_sigma: f32,
        max: u8,
    },
    /// A seeded roulette per note: pass it through with probability
    /// `pass`, replace it with a uniform key from `lo..=hi` with
    /// probability `replace`, and drop it otherwise. The two
    /// probabilities sum to at most 1.
    NoteRoulette {
        seed: u64,
        pass: f32,
        replace: f32,
        lo: u8,
        hi: u8,
    },
    /// Reroll each note-on velocity as a uniform draw from `lo..=hi`.
    VelocityDiceUniform { seed: u64, lo: u8, hi: u8 },
    /// Reroll each note-on velocity as a gaussian draw around the
    /// played velocity.
    VelocityDiceGaussian { seed: u64, sigma: f32 },
    /// Draw each note's length from a seeded lottery around `mean`,
    /// clamped to `min..=max`: exponential by default, flat over the
    /// clamp range when `uniform`.
    ///
    /// Only the mean follows the usual duration pair (`mean="500ms"` or
    /// the node's single `beats=`); `min=` and `max=` are plain duration
    /// strings, so the node keeps the one-`beats=`-per-node convention
    /// the other timed effects use. `min <= mean <= max` is checked here
    /// when the mean is absolute, and by the CLI after the tempo
    /// resolves a `beats=` mean.
    DurationLottery {
        seed: u64,
        mean: TimeSpec,
        min: TimeSpec,
        max: TimeSpec,
        uniform: bool,
    },
    /// Thin the note stream toward `target` notes per second, measured
    /// over a sliding `window`; the excess is dropped by seeded lottery.
    DensityGovernor {
        seed: u64,
        target: f32,
        window: TimeSpec,
    },
    /// Turn each note into a Cowell-style cluster of `width` keys drawn
    /// from `kind`, anchored to the played key, edge velocities scaled
    /// by `rolloff`.
    ClusterFist {
        kind: ClusterKind,
        width: u8,
        anchor: ClusterAnchor,
        rolloff: f32,
    },
    /// Add a ghost halo of `width` neighbors around each note at
    /// `level` of its velocity, fading over `decay`; a sieve expression
    /// (kept as written, parsed by the CLI) confines the halo to sieve
    /// keys.
    ResonanceHalo {
        width: u8,
        level: f32,
        decay: TimeSpec,
        sieve: Option<String>,
    },
    /// Gate note-ons through a Euclidean rhythm: `k` pulses spread as
    /// evenly as possible over `n` steps of `pulse` length, the pattern
    /// rotated by `rotation` steps. Notes landing on a silent step wait
    /// for the next sounding one when `defer`, or are dropped.
    EuclideanGate {
        k: u8,
        n: u8,
        rotation: u8,
        pulse: TimeSpec,
        defer: bool,
    },
    /// Pull events onto a time grid: `strength` 1 snaps them exactly
    /// onto the nearest `grid` line, lower values move them only part
    /// of the way.
    Quantize { grid: TimeSpec, strength: f32 },
    /// Lock note-ons to a repeating duration cycle, the medieval talea:
    /// `talea 250 500 250 1000` in milliseconds, or `talea 1 0.5 0.5 2
    /// beats=true` in beats against the tempo. Between 1 and 32 entries;
    /// each must resolve to 1ms..=60s (millisecond entries are checked
    /// here, beat entries by the CLI once the tempo resolves them).
    Talea { durations: Vec<TimeSpec> },
    /// Messiaen's added values: a seeded share of note-ons is stretched
    /// by one `unit` (probability `extend`) or held back by one `unit`
    /// (probability `defer`), unsettling the meter.
    AddedValue {
        seed: u64,
        unit: TimeSpec,
        extend: f32,
        defer: f32,
    },
    /// Additive accent groups (`accent-groups 3 5` is 3+5): the first
    /// note-on of each group takes the `accent` velocity, the rest of
    /// the group takes `rest`.
    AccentGroups {
        groups: Vec<u8>,
        accent: u8,
        rest: u8,
    },
    /// Feldman's quiet field: every note-on velocity sinks into
    /// `floor..=ceiling` with a seeded jitter of up to `jitter` steps,
    /// so nothing rises above a whisper.
    FeldmanField {
        seed: u64,
        floor: u8,
        ceiling: u8,
        jitter: u8,
    },
    /// Mirror note-on velocities around the `pivot`: soft playing comes
    /// out loud and loud playing comes out soft.
    VelocityInvert { pivot: u8 },
    /// Route notes by touch: velocities below `low` go to the `soft`
    /// channel, above `high` to the `loud` one, the rest to `mid`.
    /// Channels are stored 0-based.
    VelocityRouter {
        low: u8,
        high: u8,
        soft_ch: u8,
        mid_ch: u8,
        loud_ch: u8,
    },
    /// Seeded anti-accents: roughly once per `every`, one note-on is
    /// pressed down to the `level` velocity, denting any accent that
    /// tries to form.
    AntiAccent {
        seed: u64,
        level: u8,
        every: TimeSpec,
    },
    /// A slow tide under the dynamics: velocities swell and recede over
    /// each `period`, scaled by `depth`, shaped as a ramp or an arch.
    MassCrescendo {
        period: TimeSpec,
        depth: f32,
        shape: CrescendoShape,
    },
    /// Ligeti's Continuum machine: while keys are held, the held set
    /// cycles as a stream of `rate` notes per second in the given
    /// order, each note sounding for `gate` of its slot. The played
    /// notes are consumed; the blur replaces them.
    Continuum {
        rate: f32,
        order: ContinuumOrder,
        gate: f32,
        seed: u64,
    },
    /// The Poeme symphonique: each note-on winds up an independent
    /// metronome ticking that key at a seeded tempo within
    /// `bpm_lo..=bpm_hi`, every tick `fade` times softer, running
    /// down after at most `max` repeats.
    MetronomeSwarm {
        seed: u64,
        bpm_lo: f32,
        bpm_hi: f32,
        max: u8,
        fade: f32,
    },
    /// Xenakis's Mists: each note-on releases a walker that steps a
    /// Gaussian `sigma` semitones every `interval` and sounds where it
    /// lands, fenced into `lo..=hi`, until the note-off calls it home.
    BrownianWalker {
        seed: u64,
        interval: TimeSpec,
        sigma: f32,
        lo: u8,
        hi: u8,
    },
    /// Ligeti's mechanico textures: played notes latch onto a
    /// relentless `pulse` grid and are restruck up to `repeats` times,
    /// except a seeded `jam` fraction of pulses that stick or drop.
    Mechanico {
        pulse: TimeSpec,
        repeats: u8,
        jam: f32,
        seed: u64,
    },
    /// The Continuator: listens while you play, and once you fall
    /// silent for `idle` it answers with a seeded walk over what it
    /// heard, at most `max` notes or until you play again.
    Continuator { seed: u64, idle: TimeSpec, max: u16 },
    /// Run a Luau script on every event: `script "wedge.lua" seed=42`.
    /// The path is kept exactly as written; the CLI resolves it against
    /// the config file's directory when it builds the graph, so parsing
    /// never touches the filesystem.
    Script { path: String, seed: u64 },
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
