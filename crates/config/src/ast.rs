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
}
