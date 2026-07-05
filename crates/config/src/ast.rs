//! The knus derive layer: KDL nodes as they appear on disk.
//!
//! These types mirror the document shape, not the public API. Everything
//! optional stays `Option` and every number stays wide here; defaults and
//! range checks live in [`crate::lower`], where they can produce errors
//! that name the node and the constraint.

/// A whole config document. `input` and `output` are matched by name
/// wherever they appear; every other top-level node must be an effect.
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
    /// The implicit top-level chain.
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
        sigma: Option<f64>,
    },
    VelocityCurve {
        #[knus(property)]
        gamma: Option<f64>,
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
        decay: Option<f64>,
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
        jitter: Option<f64>,
        #[knus(property)]
        decay: Option<f64>,
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
        curve: Option<f64>,
    },
}
