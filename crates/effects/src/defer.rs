//! Shared bookkeeping for effects that move note-ons into the future.
//!
//! When an effect defers a note-on to a time past its arrival, the
//! player's note-off can arrive while the deferred on is still pending.
//! Emitted at its arrival time it would sort before the on at the
//! scheduler and orphan it. The tracker here remembers the time each
//! active note's on was emitted and holds every off back to at least
//! `MIN_GAP_NS` past it, so pairs always reach the destination in order.

use miditool_core::{Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::push;

/// Minimum gap between an emitted note-on and its note-off: 10ms, enough
/// for the scheduler's time ordering to keep the pair in order and for
/// the destination to voice the note at all.
pub(crate) const MIN_GAP_NS: u64 = 10_000_000;

/// Maps each active input (channel, key) to the time its note-on was
/// emitted. `None` means nothing is sounding for that slot, either
/// because no note-on arrived or because the effect dropped it.
#[derive(Default)]
pub(crate) struct DeferTracker {
    active: PerNote<Option<u64>>,
}

impl DeferTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Handle a note-on whose emitted time is `on_time` (`None` drops the
    /// note, and the tracker will swallow its future off). A retrigger of
    /// an already active input cuts first: a note-off at
    /// `max(arrival, previous on + MIN_GAP_NS)`, and the new on is raised
    /// to that cut time when needed so the cut can never land after the
    /// note it must precede. `ev` must be a note-on.
    pub(crate) fn note_on(
        &mut self,
        ev: &Event,
        on_time: Option<u64>,
        out: &mut EventBuf,
        cx: &ProcCx,
    ) {
        let EventKind::NoteOn { ch, key, vel } = ev.kind else {
            return;
        };
        let mut floor = ev.time;
        if let Some(prev) = self.active.take(ch, key) {
            let cut_time = ev.time.max(prev.saturating_add(MIN_GAP_NS));
            let cut = EventKind::NoteOff { ch, key, vel: 0 };
            push(out, cx, Event::new(cut_time, cut));
            floor = cut_time;
        }
        if let Some(on_time) = on_time {
            let on_time = on_time.max(floor);
            let kind = EventKind::NoteOn { ch, key, vel };
            push(out, cx, Event::new(on_time, kind));
            self.active.set(ch, key, Some(on_time));
        }
    }

    /// Route a note-off to the on it ends, `extra_ns` later than its
    /// arrival but never earlier than `MIN_GAP_NS` past the emitted on.
    /// With no active entry the off is swallowed: its on was dropped or
    /// never happened. `ev` must be a note-off.
    pub(crate) fn note_off(&mut self, ev: &Event, extra_ns: u64, out: &mut EventBuf, cx: &ProcCx) {
        let EventKind::NoteOff { ch, key, vel } = ev.kind else {
            return;
        };
        if let Some(on_time) = self.active.take(ch, key) {
            let time = ev
                .time
                .saturating_add(extra_ns)
                .max(on_time.saturating_add(MIN_GAP_NS));
            let kind = EventKind::NoteOff { ch, key, vel };
            push(out, cx, Event::new(time, kind));
        }
    }

    /// Route poly pressure to the sounding note, never earlier than its
    /// emitted on; dropped when nothing is active for the slot. `ev` must
    /// be a poly-pressure event.
    pub(crate) fn poly_pressure(&self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let EventKind::PolyPressure { ch, key, .. } = ev.kind else {
            return;
        };
        if let Some(on_time) = self.active.get(ch, key) {
            push(out, cx, Event::new(ev.time.max(on_time), ev.kind));
        }
    }

    /// Emit a note-off (velocity 0) for every active note at
    /// `max(now, on + MIN_GAP_NS)` and clear the map.
    pub(crate) fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        let active = std::mem::take(&mut self.active);
        active.for_each(|ch, key, entry| {
            if let Some(on_time) = entry {
                let kind = EventKind::NoteOff { ch, key, vel: 0 };
                let time = cx.now.max(on_time.saturating_add(MIN_GAP_NS));
                push(out, cx, Event::new(time, kind));
            }
        });
    }
}
