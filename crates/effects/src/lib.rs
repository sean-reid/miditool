//! Built-in effect library for miditool: pitch remappers, velocity shaping,
//! channel routing, and time-based effects.
//!
//! Every effect here is realtime-safe (`process` and `flush` never allocate)
//! and note-off correct: an effect that rewrites keys remembers what each
//! note-on became, so the matching note-off always lands on the note that is
//! actually sounding and `flush` can silence everything it started.
//!
//! Time-based effects never sleep or schedule. `Event.time` is the intended
//! send moment in engine-monotonic nanoseconds, so delaying an event means
//! adding a delta to its time, and repeats are all computed up front at note
//! time (emit-ahead); a downstream scheduler delivers them. Each such effect
//! documents its worst-case fanout, kept well under `MAX_FANOUT` so a single
//! input event never overflows the buffer.

mod defer;
mod mpe;
mod router;

pub mod accent_groups;
pub mod added_value;
pub mod aggregate_gate;
pub mod anti_accent;
pub mod blocked_keys;
pub mod brownian_walker;
pub mod channelize;
pub mod cluster_fist;
pub mod complement_pad;
pub mod continuator;
pub mod continuum;
pub mod delay;
pub mod density_governor;
pub mod duration_lottery;
pub mod echo;
pub mod euclidean_gate;
pub mod feldman_field;
pub mod just;
pub mod klangfarben;
pub mod loose_keys;
pub mod mass_crescendo;
pub mod mechanico;
pub mod metronome_swarm;
pub mod mode_lock;
pub mod negative_harmony;
pub mod note_roulette;
pub mod overtone_pedal;
pub mod poisson_cloud;
pub mod quantize;
pub mod registral_scatter;
pub mod resonance_halo;
pub mod restrike;
pub mod ring_mod;
pub mod row_snap;
pub mod scordatura;
pub mod shuffle_lock;
pub mod sieve_quantizer;
pub mod spectral_halo;
pub mod stutter;
pub mod talea;
pub mod telescope;
pub mod tintinnabuli;
pub mod tonnetz;
pub mod transpose;
pub mod velocity_curve;
pub mod velocity_dice;
pub mod velocity_invert;
pub mod velocity_router;
pub mod wedge_mirror;

pub use accent_groups::AccentGroups;
pub use added_value::AddedValue;
pub use aggregate_gate::AggregateGate;
pub use anti_accent::AntiAccent;
pub use blocked_keys::BlockedKeys;
pub use brownian_walker::BrownianWalker;
pub use channelize::Channelize;
pub use cluster_fist::{ClusterAnchor, ClusterFist, ClusterKind};
pub use complement_pad::ComplementPad;
pub use continuator::Continuator;
pub use continuum::{Continuum, ContinuumOrder};
pub use delay::Delay;
pub use density_governor::DensityGovernor;
pub use duration_lottery::DurationLottery;
pub use echo::Echo;
pub use euclidean_gate::EuclideanGate;
pub use feldman_field::FeldmanField;
pub use just::Just;
pub use klangfarben::Klangfarben;
pub use loose_keys::{KeyDist, LooseKeys};
pub use mass_crescendo::{CrescendoShape, MassCrescendo};
pub use mechanico::Mechanico;
pub use metronome_swarm::MetronomeSwarm;
pub use mode_lock::ModeLock;
pub use mpe::MpeParams;
pub use negative_harmony::NegativeHarmony;
pub use note_roulette::NoteRoulette;
pub use overtone_pedal::OvertonePedal;
pub use poisson_cloud::PoissonCloud;
pub use quantize::Quantize;
pub use registral_scatter::RegistralScatter;
pub use resonance_halo::ResonanceHalo;
pub use restrike::Restrike;
pub use ring_mod::RingMod;
pub use row_snap::{RowForm, RowSnap};
pub use scordatura::Scordatura;
pub use shuffle_lock::{ShuffleLock, ShuffleMode};
pub use sieve_quantizer::{SieveQuantizer, SieveSnap};
pub use spectral_halo::SpectralHalo;
pub use stutter::Stutter;
pub use talea::Talea;
pub use telescope::Telescope;
pub use tintinnabuli::{TDirection, Tintinnabuli};
pub use tonnetz::{Plr, Tonnetz};
pub use transpose::Transpose;
pub use velocity_curve::VelocityCurve;
pub use velocity_dice::{VelDist, VelocityDice};
pub use velocity_invert::VelocityInvert;
pub use velocity_router::VelocityRouter;
pub use wedge_mirror::WedgeMirror;

#[cfg(test)]
pub(crate) mod testutil {
    use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

    /// Run one event through an effect at time 0 and collect the outputs.
    pub(crate) fn run(fx: &mut impl Effect, kind: EventKind) -> Vec<EventKind> {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&Event::new(0, kind), &mut out, &cx);
        out.iter().map(|e| e.kind).collect()
    }

    /// Run one event through an effect and collect the outputs, timestamps
    /// included.
    pub(crate) fn run_timed(fx: &mut impl Effect, time: u64, kind: EventKind) -> Vec<Event> {
        let cx = ProcCx::at(time);
        let mut out = EventBuf::new();
        fx.process(&Event::new(time, kind), &mut out, &cx);
        out.iter().copied().collect()
    }

    /// Advance an effect's clock to `now` and collect the outputs,
    /// timestamps included.
    pub(crate) fn tick(fx: &mut impl Effect, now: u64) -> Vec<Event> {
        let cx = ProcCx::at(now);
        let mut out = EventBuf::new();
        fx.tick(now, &mut out, &cx);
        out.iter().copied().collect()
    }

    /// Flush an effect at time 0 and collect the outputs.
    pub(crate) fn flush(fx: &mut impl Effect) -> Vec<EventKind> {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.flush(&mut out, &cx);
        out.iter().map(|e| e.kind).collect()
    }

    pub(crate) fn on(key: u8) -> EventKind {
        EventKind::NoteOn {
            ch: 0,
            key,
            vel: 100,
        }
    }

    pub(crate) fn off(key: u8) -> EventKind {
        EventKind::NoteOff { ch: 0, key, vel: 0 }
    }

    pub(crate) fn at(time: u64, kind: EventKind) -> Event {
        Event::new(time, kind)
    }
}
