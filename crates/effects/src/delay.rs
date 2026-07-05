//! Shift every event later by a fixed amount.

use miditool_core::{Effect, Event, EventBuf, ProcCx};

use crate::router::push;

/// Move every event's intended send moment `delta_ns` nanoseconds into the
/// future; a downstream scheduler delivers it then. Note-ons and note-offs
/// shift by the same amount, so pairs stay matched. Stateless.
///
/// Fanout bound: exactly one output per input.
pub struct Delay {
    delta_ns: u64,
}

impl Delay {
    pub fn new(delta_ns: u64) -> Self {
        Self { delta_ns }
    }
}

impl Effect for Delay {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let time = ev.time.saturating_add(self.delta_ns);
        push(out, cx, Event::new(time, ev.kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};
    use miditool_core::EventKind;

    #[test]
    fn shifts_notes_by_delta() {
        let mut fx = Delay::new(250);
        assert_eq!(run_timed(&mut fx, 1_000, on(60)), vec![at(1_250, on(60))]);
        assert_eq!(run_timed(&mut fx, 2_000, off(60)), vec![at(2_250, off(60))]);
    }

    #[test]
    fn shifts_non_note_events_too() {
        let mut fx = Delay::new(7);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 3, pedal), vec![at(10, pedal)]);
    }

    #[test]
    fn zero_delta_is_identity() {
        let mut fx = Delay::new(0);
        assert_eq!(run_timed(&mut fx, 42, on(60)), vec![at(42, on(60))]);
    }
}
