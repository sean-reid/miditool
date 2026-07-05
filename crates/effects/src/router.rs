//! Shared note-on/note-off bookkeeping for pitch-remapping effects.
//!
//! An effect that rewrites keys must remember, per input (channel, key),
//! which output key its note-on produced. Only then can it route the
//! matching note-off to the note that is actually sounding, cut cleanly on
//! retrigger, and silence everything on flush.

use std::sync::atomic::Ordering;

use miditool_core::{Event, EventBuf, EventKind, PerNote, ProcCx};

/// Append an event, counting it as dropped if the buffer is full.
pub(crate) fn push(out: &mut EventBuf, cx: &ProcCx, ev: Event) {
    if out.try_push(ev).is_err() {
        cx.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Maps each active input (channel, key) to the output key its note-on
/// produced. `None` means nothing is sounding for that slot, either because
/// no note-on arrived or because the note-on was dropped.
#[derive(Default)]
pub(crate) struct NoteRouter {
    active: PerNote<Option<u8>>,
}

impl NoteRouter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Handle a note-on whose mapped output key is `mapped` (`None` drops
    /// the note). A retrigger of an already active input cuts: the previous
    /// output key gets a note-off before the new note-on. `ev` must be a
    /// note-on.
    pub(crate) fn note_on(
        &mut self,
        ev: &Event,
        mapped: Option<u8>,
        out: &mut EventBuf,
        cx: &ProcCx,
    ) {
        let EventKind::NoteOn { ch, key, vel } = ev.kind else {
            return;
        };
        if let Some(prev) = self.active.take(ch, key) {
            let cut = EventKind::NoteOff {
                ch,
                key: prev,
                vel: 0,
            };
            push(out, cx, Event::new(ev.time, cut));
        }
        if let Some(key_out) = mapped {
            let kind = EventKind::NoteOn {
                ch,
                key: key_out,
                vel,
            };
            push(out, cx, Event::new(ev.time, kind));
            self.active.set(ch, key, Some(key_out));
        }
    }

    /// Route a note-off to the key its note-on produced. With no active
    /// entry, fall back to `fallback` (`None` drops the note-off). `ev`
    /// must be a note-off.
    pub(crate) fn note_off(
        &mut self,
        ev: &Event,
        fallback: Option<u8>,
        out: &mut EventBuf,
        cx: &ProcCx,
    ) {
        let EventKind::NoteOff { ch, key, vel } = ev.kind else {
            return;
        };
        if let Some(key_out) = self.active.take(ch, key).or(fallback) {
            let kind = EventKind::NoteOff {
                ch,
                key: key_out,
                vel,
            };
            push(out, cx, Event::new(ev.time, kind));
        }
    }

    /// Route poly pressure to the sounding note when one exists, falling
    /// back to `fallback` otherwise (`None` drops the event). `ev` must be
    /// a poly-pressure event.
    pub(crate) fn poly_pressure(
        &self,
        ev: &Event,
        fallback: Option<u8>,
        out: &mut EventBuf,
        cx: &ProcCx,
    ) {
        let EventKind::PolyPressure { ch, key, value } = ev.kind else {
            return;
        };
        if let Some(key_out) = self.active.get(ch, key).or(fallback) {
            let kind = EventKind::PolyPressure {
                ch,
                key: key_out,
                value,
            };
            push(out, cx, Event::new(ev.time, kind));
        }
    }

    /// Emit a note-off (velocity 0) for every active output note and clear
    /// the map.
    pub(crate) fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        let active = std::mem::take(&mut self.active);
        active.for_each(|ch, _key, mapped| {
            if let Some(key_out) = mapped {
                let kind = EventKind::NoteOff {
                    ch,
                    key: key_out,
                    vel: 0,
                };
                push(out, cx, Event::new(cx.now, kind));
            }
        });
    }
}
