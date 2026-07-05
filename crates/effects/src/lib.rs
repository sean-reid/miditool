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

pub mod channelize;
pub mod delay;
pub mod echo;
pub mod loose_keys;
pub mod restrike;
pub mod shuffle_lock;
pub mod stutter;
pub mod transpose;
pub mod velocity_curve;

pub use channelize::Channelize;
pub use delay::Delay;
pub use echo::Echo;
pub use loose_keys::{KeyDist, LooseKeys};
pub use restrike::Restrike;
pub use shuffle_lock::{ShuffleLock, ShuffleMode};
pub use stutter::Stutter;
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
