//! Built-in effect library for miditool: pitch remappers, velocity shaping,
//! and channel routing.
//!
//! Every effect here is realtime-safe (`process` and `flush` never allocate)
//! and note-off correct: an effect that rewrites keys remembers what each
//! note-on became, so the matching note-off always lands on the note that is
//! actually sounding and `flush` can silence everything it started.

mod router;

pub mod channelize;
pub mod loose_keys;
pub mod shuffle_lock;
pub mod transpose;
pub mod velocity_curve;

pub use channelize::Channelize;
pub use loose_keys::{KeyDist, LooseKeys};
pub use shuffle_lock::{ShuffleLock, ShuffleMode};
pub use transpose::Transpose;
pub use velocity_curve::VelocityCurve;

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
}
