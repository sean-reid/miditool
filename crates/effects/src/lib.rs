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

mod router;

pub mod aggregate_gate;
pub mod blocked_keys;
pub mod channelize;
pub mod cluster_fist;
pub mod delay;
pub mod density_governor;
pub mod duration_lottery;
pub mod echo;
pub mod klangfarben;
pub mod loose_keys;
pub mod note_roulette;
pub mod poisson_cloud;
pub mod registral_scatter;
pub mod resonance_halo;
pub mod restrike;
pub mod ring_mod;
pub mod row_snap;
pub mod shuffle_lock;
pub mod sieve_quantizer;
pub mod stutter;
pub mod telescope;
pub mod transpose;
pub mod velocity_curve;
pub mod velocity_dice;
pub mod wedge_mirror;

pub use aggregate_gate::AggregateGate;
pub use blocked_keys::BlockedKeys;
pub use channelize::Channelize;
pub use cluster_fist::{ClusterAnchor, ClusterFist, ClusterKind};
pub use delay::Delay;
pub use density_governor::DensityGovernor;
pub use duration_lottery::DurationLottery;
pub use echo::Echo;
pub use klangfarben::Klangfarben;
pub use loose_keys::{KeyDist, LooseKeys};
pub use note_roulette::NoteRoulette;
pub use poisson_cloud::PoissonCloud;
pub use registral_scatter::RegistralScatter;
pub use resonance_halo::ResonanceHalo;
pub use restrike::Restrike;
pub use ring_mod::RingMod;
pub use row_snap::{RowForm, RowSnap};
pub use shuffle_lock::{ShuffleLock, ShuffleMode};
pub use sieve_quantizer::{SieveQuantizer, SieveSnap};
pub use stutter::Stutter;
pub use telescope::Telescope;
pub use transpose::Transpose;
pub use velocity_curve::VelocityCurve;
pub use velocity_dice::{VelDist, VelocityDice};
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
