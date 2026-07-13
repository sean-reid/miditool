//! The knus derive layer: KDL nodes as they appear on disk.
//!
//! These types mirror the document shape, not the public API. Everything
//! optional stays `Option` and every number stays wide here; defaults and
//! range checks live in [`crate::lower`], where they can produce errors
//! that name the node and the constraint.

/// A whole config document. `input`, `output`, `tempo`, `remote`, and
/// `scene` are matched by name wherever they appear; every other
/// top-level node must be an effect.
#[derive(Debug, knus::Decode)]
pub(crate) struct Document {
    /// `input "<substring>" hide=#true`
    #[knus(child)]
    pub input: Option<Input>,
    /// `output virtual="Name"` or `output device="substring"`
    #[knus(child)]
    pub output: Option<Output>,
    /// `tempo 96`
    #[knus(child)]
    pub tempo: Option<Tempo>,
    /// `remote port=8320`
    #[knus(child)]
    pub remote: Option<Remote>,
    /// `control { ... }`: keyboard keys reserved as performance
    /// gestures, plus the optional moments clock.
    #[knus(child)]
    pub control: Option<Control>,
    /// `scene "name" { ... }` blocks. Mutually exclusive with bare
    /// effects; [`crate::lower`] enforces that.
    #[knus(children(name = "scene"))]
    pub scenes: Vec<Scene>,
    /// Bare effects: the implicit top-level chain.
    #[knus(children)]
    pub effects: Vec<Effect>,
}

/// The `input` node: a port name substring, and optionally `hide=#true`
/// to hide the raw source from other apps while miditool runs (macOS).
#[derive(Debug, knus::Decode)]
pub(crate) struct Input {
    #[knus(argument)]
    pub name: String,
    #[knus(property)]
    pub hide: Option<bool>,
}

/// The `output` node. Exactly one of the two properties must be present;
/// [`crate::lower`] enforces that.
#[derive(Debug, knus::Decode)]
pub(crate) struct Output {
    #[knus(property(name = "virtual"))]
    pub virtual_: Option<String>,
    #[knus(property)]
    pub device: Option<String>,
}

/// The `tempo` node: beats per minute for `beats=` times.
#[derive(Debug, knus::Decode)]
pub(crate) struct Tempo {
    #[knus(argument)]
    pub bpm: Number,
}

/// The `remote` node: a TCP port for the phone/tablet web remote, and
/// optionally the address to bind. The port stays wide and the bind stays
/// a string here; [`crate::lower`] range-checks the port into a `u16` and
/// parses the bind into an `IpAddr`.
#[derive(Debug, knus::Decode)]
pub(crate) struct Remote {
    #[knus(property)]
    pub port: i64,
    #[knus(property)]
    pub bind: Option<String>,
}

/// The `control` node: keyboard keys reserved as performance gestures
/// (`next-scene`, `prev-scene`, repeatable `goto`, `panic`), plus the
/// optional `moments` clock. Keys stay wide here; [`crate::lower`]
/// range-checks them and enforces that each key serves one role.
#[derive(Debug, knus::Decode)]
pub(crate) struct Control {
    #[knus(child)]
    pub next_scene: Option<ControlKey>,
    #[knus(child)]
    pub prev_scene: Option<ControlKey>,
    #[knus(children(name = "goto"))]
    pub gotos: Vec<Goto>,
    #[knus(child)]
    pub panic: Option<ControlKey>,
    #[knus(child)]
    pub moments: Option<Moments>,
}

/// A control gesture carrying just `key=<0..=127>`: `next-scene`,
/// `prev-scene`, and `panic` all look like this.
#[derive(Debug, knus::Decode)]
pub(crate) struct ControlKey {
    #[knus(property)]
    pub key: i64,
}

/// `goto key=21 scene="clouds"`: a jump straight to a named scene.
#[derive(Debug, knus::Decode)]
pub(crate) struct Goto {
    #[knus(property)]
    pub key: i64,
    #[knus(property)]
    pub scene: String,
}

/// `moments dwell-lo="20s" dwell-hi="90s" seed=7`. Both dwells are
/// plain duration strings: the `beats=` convention admits one
/// beat-valued property per node and the dwell pair needs two, so
/// neither takes it.
#[derive(Debug, knus::Decode)]
pub(crate) struct Moments {
    #[knus(property)]
    pub dwell_lo: String,
    #[knus(property)]
    pub dwell_hi: String,
    #[knus(property)]
    pub seed: Option<u64>,
}

/// A `scene` node: a name, an optional `switch=` behavior, and the
/// scene's chain of effects as children.
#[derive(Debug, knus::Decode)]
pub(crate) struct Scene {
    #[knus(argument)]
    pub name: String,
    #[knus(property)]
    pub switch: Option<String>,
    #[knus(children)]
    pub effects: Vec<Effect>,
}

/// A numeric scalar decoded from an integer or a decimal literal.
///
/// knus decodes `f64` from decimal literals only, which would make
/// `tempo 96` a type error while `tempo 96.0` parses. Values that are
/// naturally written either way go through this wrapper instead.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Number(pub f64);

impl<S: knus::traits::ErrorSpan> knus::traits::DecodeScalar<S> for Number {
    fn type_check(
        _type_name: &Option<knus::span::Spanned<knus::ast::TypeName, S>>,
        _ctx: &mut knus::decode::Context<S>,
    ) {
    }

    fn raw_decode(
        value: &knus::span::Spanned<knus::ast::Literal, S>,
        ctx: &mut knus::decode::Context<S>,
    ) -> Result<Self, knus::errors::DecodeError<S>> {
        use knus::ast::Literal;
        type DynError = Box<dyn std::error::Error + Send + Sync>;
        let parsed: Result<f64, DynError> = match &**value {
            Literal::Int(v) => i64::try_from(v).map(|v| v as f64).map_err(Into::into),
            Literal::Decimal(v) => f64::try_from(v).map_err(Into::into),
            _ => {
                ctx.emit_error(knus::errors::DecodeError::scalar_kind(
                    knus::decode::Kind::Decimal,
                    value,
                ));
                return Ok(Number(0.0));
            }
        };
        match parsed {
            Ok(v) => Ok(Number(v)),
            Err(e) => {
                ctx.emit_error(knus::errors::DecodeError::conversion(value, e));
                Ok(Number(0.0))
            }
        }
    }
}

/// One effect node. Variant names decode from kebab-case node names, so
/// `ShuffleLock` is written `shuffle-lock`.
#[derive(Debug, knus::Decode)]
pub(crate) enum Effect {
    Chain {
        #[knus(children)]
        children: Vec<Effect>,
    },
    Fork {
        #[knus(children)]
        children: Vec<Effect>,
    },
    Pass,
    Discard,
    Transpose {
        #[knus(argument)]
        semis: i64,
    },
    ShuffleLock {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
        #[knus(property)]
        mode: Option<String>,
    },
    LooseKeys {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
        #[knus(property)]
        sigma: Option<Number>,
    },
    VelocityCurve {
        #[knus(property)]
        gamma: Option<Number>,
        #[knus(property)]
        floor: Option<i64>,
        #[knus(property)]
        ceiling: Option<i64>,
    },
    Channelize {
        #[knus(argument)]
        ch: i64,
    },
    OnlyChannels {
        #[knus(arguments)]
        channels: Vec<i64>,
    },
    KeyRange {
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
    },
    VelocityRange {
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
    },
    NotesOnly,
    ControllersOnly,
    Delay {
        #[knus(property)]
        time: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
    },
    Echo {
        #[knus(property)]
        repeats: Option<i64>,
        #[knus(property)]
        time: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        decay: Option<Number>,
        #[knus(property)]
        transpose: Option<i64>,
    },
    Restrike {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        interval: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        jitter: Option<Number>,
        #[knus(property)]
        decay: Option<Number>,
        #[knus(property)]
        floor: Option<i64>,
        #[knus(property)]
        max: Option<i64>,
    },
    Stutter {
        #[knus(property)]
        repeats: Option<i64>,
        #[knus(property)]
        first: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        curve: Option<Number>,
    },
    RegistralScatter {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
    },
    WedgeMirror {
        #[knus(property)]
        axis: Option<i64>,
        #[knus(property)]
        probability: Option<Number>,
        #[knus(property)]
        seed: Option<u64>,
    },
    BlockedKeys {
        #[knus(arguments)]
        keys: Vec<i64>,
        #[knus(property)]
        by_class: Option<bool>,
    },
    Klangfarben {
        #[knus(arguments)]
        channels: Vec<i64>,
        #[knus(property)]
        mode: Option<String>,
        #[knus(property)]
        seed: Option<u64>,
    },
    RingMod {
        #[knus(property)]
        carrier: i64,
        #[knus(property)]
        sum: Option<bool>,
        #[knus(property)]
        diff: Option<bool>,
        #[knus(property)]
        dry: Option<bool>,
    },
    Telescope {
        #[knus(property)]
        factor: Number,
        #[knus(property)]
        reference: Option<i64>,
    },
    RowSnap {
        #[knus(arguments)]
        row: Vec<i64>,
        #[knus(property)]
        form: Option<String>,
        #[knus(property)]
        transpose: Option<i64>,
    },
    AggregateGate {
        #[knus(property)]
        leak: Option<Number>,
        #[knus(property)]
        seed: Option<u64>,
    },
    Sieve {
        #[knus(argument)]
        expr: String,
        #[knus(property)]
        snap: Option<String>,
    },
    Tintinnabuli {
        #[knus(property)]
        root: String,
        #[knus(property)]
        minor: Option<bool>,
        #[knus(property)]
        position: Option<i64>,
        #[knus(property)]
        direction: Option<String>,
        #[knus(property)]
        level: Option<Number>,
    },
    ModeLock {
        #[knus(property)]
        mode: i64,
        #[knus(property)]
        transposition: Option<i64>,
        #[knus(property)]
        snap: Option<String>,
    },
    NegativeHarmony {
        #[knus(property)]
        tonic: String,
        #[knus(property)]
        mode: Option<String>,
        #[knus(property)]
        level: Option<Number>,
    },
    Tonnetz {
        #[knus(property)]
        start: String,
        #[knus(property)]
        minor: Option<bool>,
        #[knus(property)]
        sequence: Option<String>,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
        #[knus(property)]
        include_played: Option<bool>,
    },
    ComplementPad {
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
        #[knus(property)]
        vel: Option<i64>,
    },
    SpectralHalo {
        #[knus(property)]
        partials: Option<i64>,
        #[knus(property)]
        rolloff: Option<Number>,
        #[knus(property)]
        stretch: Option<Number>,
        #[knus(property)]
        channels: Option<String>,
        #[knus(property)]
        bend_range: Option<Number>,
    },
    Just {
        #[knus(property)]
        root: String,
        #[knus(property)]
        channels: Option<String>,
        #[knus(property)]
        bend_range: Option<Number>,
    },
    Scordatura {
        #[knus(arguments)]
        pairs: Vec<String>,
        #[knus(property)]
        channels: Option<String>,
        #[knus(property)]
        bend_range: Option<Number>,
    },
    OvertonePedal {
        #[knus(property)]
        fundamental: i64,
        #[knus(property)]
        partials: Option<i64>,
        #[knus(property)]
        channels: Option<String>,
        #[knus(property)]
        bend_range: Option<Number>,
    },
    PoissonCloud {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        density: Option<Number>,
        #[knus(property)]
        duration: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        sigma: Option<Number>,
        #[knus(property)]
        vel_sigma: Option<Number>,
        #[knus(property)]
        max: Option<i64>,
    },
    NoteRoulette {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        pass: Option<Number>,
        #[knus(property)]
        replace: Option<Number>,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
    },
    VelocityDice {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
        #[knus(property)]
        sigma: Option<Number>,
    },
    DurationLottery {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        mean: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        min: Option<String>,
        #[knus(property)]
        max: Option<String>,
        #[knus(property)]
        spread: Option<String>,
    },
    DensityGovernor {
        #[knus(property)]
        target: Number,
        #[knus(property)]
        window: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        seed: Option<u64>,
    },
    ClusterFist {
        #[knus(property)]
        width: Option<i64>,
        #[knus(property)]
        kind: Option<String>,
        #[knus(property)]
        anchor: Option<String>,
        #[knus(property)]
        rolloff: Option<Number>,
        #[knus(property)]
        sieve: Option<String>,
    },
    ResonanceHalo {
        #[knus(property)]
        width: Option<i64>,
        #[knus(property)]
        level: Option<Number>,
        #[knus(property)]
        decay: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        sieve: Option<String>,
    },
    EuclideanGate {
        #[knus(property)]
        k: i64,
        #[knus(property)]
        n: i64,
        #[knus(property)]
        rotation: Option<i64>,
        #[knus(property)]
        pulse: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        mode: Option<String>,
    },
    Quantize {
        #[knus(property)]
        grid: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        strength: Option<Number>,
    },
    Snap {
        #[knus(property)]
        division: Option<i64>,
        #[knus(property)]
        strength: Option<Number>,
        #[knus(property)]
        follow: Option<Number>,
        #[knus(property(name = "bpm-lo"))]
        bpm_lo: Option<Number>,
        #[knus(property(name = "bpm-hi"))]
        bpm_hi: Option<Number>,
    },
    Talea {
        #[knus(arguments)]
        durations: Vec<Number>,
        #[knus(property)]
        beats: Option<bool>,
    },
    AddedValue {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        unit: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        extend: Option<Number>,
        #[knus(property)]
        defer: Option<Number>,
    },
    AccentGroups {
        #[knus(arguments)]
        groups: Vec<i64>,
        #[knus(property)]
        accent: Option<i64>,
        #[knus(property)]
        rest: Option<i64>,
    },
    FeldmanField {
        #[knus(property)]
        seed: Option<u64>,
        #[knus(property)]
        floor: Option<i64>,
        #[knus(property)]
        ceiling: Option<i64>,
        #[knus(property)]
        jitter: Option<i64>,
    },
    VelocityInvert {
        #[knus(property)]
        pivot: Option<i64>,
    },
    VelocityRouter {
        #[knus(property)]
        low: Option<i64>,
        #[knus(property)]
        high: Option<i64>,
        #[knus(property)]
        soft: i64,
        #[knus(property)]
        medium: i64,
        #[knus(property)]
        loud: i64,
    },
    AntiAccent {
        #[knus(property)]
        seed: Option<u64>,
        #[knus(property)]
        level: Option<i64>,
        #[knus(property)]
        every: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
    },
    MassCrescendo {
        #[knus(property)]
        period: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        depth: Option<Number>,
        #[knus(property)]
        shape: Option<String>,
    },
    Continuum {
        #[knus(property)]
        rate: Option<Number>,
        #[knus(property)]
        order: Option<String>,
        #[knus(property)]
        gate: Option<Number>,
        #[knus(property)]
        seed: Option<u64>,
    },
    MetronomeSwarm {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        bpm_lo: Option<Number>,
        #[knus(property)]
        bpm_hi: Option<Number>,
        #[knus(property)]
        max: Option<i64>,
        #[knus(property)]
        fade: Option<Number>,
    },
    BrownianWalker {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        interval: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        sigma: Option<Number>,
        #[knus(property)]
        lo: Option<i64>,
        #[knus(property)]
        hi: Option<i64>,
    },
    Mechanico {
        #[knus(property)]
        pulse: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        repeats: Option<i64>,
        #[knus(property)]
        jam: Option<Number>,
        #[knus(property)]
        seed: Option<u64>,
    },
    Continuator {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        idle: Option<String>,
        #[knus(property)]
        beats: Option<Number>,
        #[knus(property)]
        max: Option<i64>,
    },
    CrippledLooper {
        #[knus(property)]
        seed: u64,
        #[knus(property)]
        pedal: Option<i64>,
        #[knus(property)]
        max: Option<i64>,
    },
    Retrograde {
        #[knus(property)]
        pedal: Option<i64>,
        #[knus(property)]
        speed: Option<Number>,
    },
    Script {
        #[knus(argument)]
        path: String,
        #[knus(property)]
        seed: Option<u64>,
    },
}
