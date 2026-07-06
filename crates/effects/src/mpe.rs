//! The shared MPE voice pool behind the microtonal effects.
//!
//! MIDI has no per-note pitch: a channel-wide pitch bend detunes every
//! note on the channel at once. MPE (MIDI Polyphonic Expression) works
//! around this by giving each sounding note a member channel of its own,
//! so the channel bend becomes a per-note microtonal offset. `MpeVoices`
//! owns an inclusive block of member channels and hands out one voice per
//! microtonal note: pitch bend first, then the note-on, stealing the
//! oldest voice when every member is busy. Callers keep the returned
//! generation-stamped `Voice` handles in their per-input-note records; a
//! handle whose voice was stolen in the meantime goes stale, and
//! releasing it is a silent no-op.
//!
//! Member channels should be kept clear of the channels that carry dry
//! notes, or a stolen voice's note-off can cut an unrelated dry note
//! sharing its (channel, key).

use miditool_core::{Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Voice-pool configuration shared by the microtonal effects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MpeParams {
    /// First member channel, 0-based inclusive.
    pub lo: u8,
    /// Last member channel, 0-based inclusive.
    pub hi: u8,
    /// The receiver's per-channel pitch-bend range in semitones (MPE
    /// member channels conventionally use 48).
    pub bend_range: f32,
}

/// A handle to an allocated pool voice: the member channel plus the
/// slot's generation at allocation time. Every allocation bumps the
/// slot's generation, so a handle from before a steal can never release
/// the note that replaced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Voice {
    pub ch: u8,
    pub generation: u32,
}

/// How one input note was routed by an effect that mixes dry passes with
/// pool voices. `Default` (`Silent`) marks an inactive slot.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum Route {
    #[default]
    Silent,
    Dry,
    Tuned(Voice),
}

#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    /// The sounding key, `None` while the voice is free.
    key: Option<u8>,
    /// Bumped on every allocation, so earlier handles go stale.
    generation: u32,
    /// Allocation order; the smallest active stamp is stolen first.
    stamp: u64,
}

/// A fixed pool of MPE member-channel voices. Realtime-safe: fixed
/// arrays only, no allocation anywhere.
pub(crate) struct MpeVoices {
    lo: u8,
    hi: u8,
    bend_range: f32,
    /// Indexed by channel; only `lo..=hi` is ever used.
    slots: [Slot; 16],
    /// Monotonic counter feeding the age stamps.
    clock: u64,
    /// Member channels that ever received a bend, one bit per channel.
    bent: u16,
}

impl MpeVoices {
    /// Channels are clamped to 15 and swapped if reversed. A bend range
    /// that is not a positive finite number falls back to 48 semitones,
    /// and larger ranges are capped at 96.
    pub(crate) fn new(params: MpeParams) -> Self {
        let (lo, hi) = (params.lo.min(15), params.hi.min(15));
        let bend_range = if params.bend_range.is_finite() && params.bend_range > 0.0 {
            params.bend_range.min(96.0)
        } else {
            48.0
        };
        Self {
            lo: lo.min(hi),
            hi: lo.max(hi),
            bend_range,
            slots: [Slot::default(); 16],
            clock: 0,
            bent: 0,
        }
    }

    /// The wire bend for a cents offset: `round(cents / 100 / bend_range
    /// * 8192)`, clamped to -8192..=8191.
    fn bend_value(&self, cents: f32) -> i16 {
        (cents / 100.0 / self.bend_range * 8192.0)
            .round()
            .clamp(-8192.0, 8191.0) as i16
    }

    /// Allocate a member channel for `key` detuned by `cents`, emitting
    /// its pitch bend and then its note-on. The lowest free member wins;
    /// with none free the oldest voice is stolen, its note-off emitted
    /// before the newcomer's events. Returns the handle the caller must
    /// present to `note_off` later.
    pub(crate) fn note_on(
        &mut self,
        time: u64,
        key: u8,
        cents: f32,
        vel: u8,
        out: &mut EventBuf,
        cx: &ProcCx,
    ) -> Voice {
        let mut free = None;
        let mut oldest = self.lo;
        let mut oldest_stamp = u64::MAX;
        for ch in self.lo..=self.hi {
            let slot = &self.slots[ch as usize];
            match slot.key {
                None => {
                    free = Some(ch);
                    break;
                }
                Some(_) if slot.stamp < oldest_stamp => {
                    oldest_stamp = slot.stamp;
                    oldest = ch;
                }
                Some(_) => {}
            }
        }
        let ch = free.unwrap_or(oldest);
        let value = self.bend_value(cents);
        let slot = &mut self.slots[ch as usize];
        if let Some(stolen) = slot.key.take() {
            let cut = EventKind::NoteOff {
                ch,
                key: stolen,
                vel: 0,
            };
            push(out, cx, Event::new(time, cut));
        }
        slot.generation = slot.generation.wrapping_add(1);
        slot.key = Some(key);
        slot.stamp = self.clock;
        self.clock += 1;
        self.bent |= 1 << ch;
        let bend = EventKind::PitchBend { ch, value };
        push(out, cx, Event::new(time, bend));
        push(
            out,
            cx,
            Event::new(time, EventKind::NoteOn { ch, key, vel }),
        );
        Voice {
            ch,
            generation: slot.generation,
        }
    }

    /// Emit the note-off for `voice` and free its slot. A stale handle
    /// (the slot's generation moved on, so the voice was stolen or
    /// already released and reused) is a silent no-op.
    pub(crate) fn note_off(&mut self, time: u64, voice: Voice, out: &mut EventBuf, cx: &ProcCx) {
        let slot = &mut self.slots[(voice.ch & 15) as usize];
        if slot.generation != voice.generation {
            return;
        }
        if let Some(key) = slot.key.take() {
            let kind = EventKind::NoteOff {
                ch: voice.ch,
                key,
                vel: 0,
            };
            push(out, cx, Event::new(time, kind));
        }
    }

    /// Release every active voice, then reset the pitch bend to 0 on
    /// every member channel that was ever bent, and clear the pool.
    pub(crate) fn flush(&mut self, time: u64, out: &mut EventBuf, cx: &ProcCx) {
        for ch in self.lo..=self.hi {
            if let Some(key) = self.slots[ch as usize].key.take() {
                let kind = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(time, kind));
            }
        }
        for ch in 0..16u8 {
            if self.bent & (1 << ch) != 0 {
                let kind = EventKind::PitchBend { ch, value: 0 };
                push(out, cx, Event::new(time, kind));
            }
        }
        self.bent = 0;
        self.clock = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(lo: u8, hi: u8, bend_range: f32) -> MpeVoices {
        MpeVoices::new(MpeParams { lo, hi, bend_range })
    }

    fn kinds(out: &EventBuf) -> Vec<EventKind> {
        out.iter().map(|e| e.kind).collect()
    }

    fn bend(ch: u8, value: i16) -> EventKind {
        EventKind::PitchBend { ch, value }
    }

    fn von(ch: u8, key: u8) -> EventKind {
        EventKind::NoteOn { ch, key, vel: 100 }
    }

    fn voff(ch: u8, key: u8) -> EventKind {
        EventKind::NoteOff { ch, key, vel: 0 }
    }

    #[test]
    fn voices_allocate_lowest_member_first() {
        let mut p = pool(1, 3, 48.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let a = p.note_on(0, 60, 0.0, 100, &mut out, &cx);
        let b = p.note_on(0, 61, 0.0, 100, &mut out, &cx);
        let c = p.note_on(0, 62, 0.0, 100, &mut out, &cx);
        assert_eq!((a.ch, b.ch, c.ch), (1, 2, 3));
        assert_eq!(
            kinds(&out),
            vec![
                bend(1, 0),
                von(1, 60),
                bend(2, 0),
                von(2, 61),
                bend(3, 0),
                von(3, 62),
            ]
        );
        // A freed member is preferred over stealing.
        out.clear();
        p.note_off(1, b, &mut out, &cx);
        let d = p.note_on(2, 63, 0.0, 100, &mut out, &cx);
        assert_eq!(d.ch, 2);
        assert_eq!(kinds(&out), vec![voff(2, 61), bend(2, 0), von(2, 63)]);
    }

    #[test]
    fn a_full_pool_steals_the_oldest_voice() {
        let mut p = pool(1, 2, 48.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let a = p.note_on(0, 60, 0.0, 100, &mut out, &cx);
        let _b = p.note_on(1, 61, 0.0, 100, &mut out, &cx);
        out.clear();
        // Both members busy: voice `a` on channel 1 is the oldest, so its
        // note-off comes first and its slot is reused.
        let c = p.note_on(2, 62, 0.0, 100, &mut out, &cx);
        assert_eq!(kinds(&out), vec![voff(1, 60), bend(1, 0), von(1, 62)]);
        assert_eq!(c.ch, a.ch);
        assert_ne!(c.generation, a.generation, "stealing bumps the generation");
    }

    #[test]
    fn a_stale_handle_is_a_silent_no_op() {
        let mut p = pool(1, 1, 48.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let a = p.note_on(0, 60, 0.0, 100, &mut out, &cx);
        let b = p.note_on(1, 61, 0.0, 100, &mut out, &cx);
        out.clear();
        // `a` was stolen: releasing it must not cut `b`.
        p.note_off(2, a, &mut out, &cx);
        assert_eq!(kinds(&out), vec![]);
        p.note_off(3, b, &mut out, &cx);
        assert_eq!(kinds(&out), vec![voff(1, 61)]);
        // A double release is equally silent.
        out.clear();
        p.note_off(4, b, &mut out, &cx);
        assert_eq!(kinds(&out), vec![]);
    }

    #[test]
    fn bend_values_map_cents_into_the_range() {
        let mut p = pool(0, 0, 48.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        // +50 cents at range 48: round(0.5 / 48 * 8192) = 85.
        p.note_on(0, 60, 50.0, 100, &mut out, &cx);
        assert_eq!(out[0].kind, bend(0, 85));
        let mut p = pool(0, 0, 48.0);
        out.clear();
        p.note_on(0, 60, -50.0, 100, &mut out, &cx);
        assert_eq!(out[0].kind, bend(0, -85));
        // Range 2: -100 cents is half the range, -4096.
        let mut p = pool(0, 0, 2.0);
        out.clear();
        p.note_on(0, 60, -100.0, 100, &mut out, &cx);
        assert_eq!(out[0].kind, bend(0, -4096));
    }

    #[test]
    fn bend_values_clamp_at_the_wire_limits() {
        let mut p = pool(0, 0, 2.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        // +250 cents at range 2 would be 10240: clamped to 8191.
        p.note_on(0, 60, 250.0, 100, &mut out, &cx);
        assert_eq!(out[0].kind, bend(0, 8191));
        let mut p = pool(0, 0, 2.0);
        out.clear();
        p.note_on(0, 60, -300.0, 100, &mut out, &cx);
        assert_eq!(out[0].kind, bend(0, -8192));
    }

    #[test]
    fn flush_releases_voices_and_resets_bent_channels() {
        let mut p = pool(0, 3, 48.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let a = p.note_on(0, 60, 25.0, 100, &mut out, &cx);
        let _b = p.note_on(1, 64, -25.0, 100, &mut out, &cx);
        // Channel 0's voice is released before the flush, but the channel
        // was bent, so it still gets its reset.
        p.note_off(2, a, &mut out, &cx);
        out.clear();
        p.flush(3, &mut out, &cx);
        assert_eq!(kinds(&out), vec![voff(1, 64), bend(0, 0), bend(1, 0)]);
        // Channels 2 and 3 were never touched: no resets for them, and a
        // second flush emits nothing at all.
        out.clear();
        p.flush(4, &mut out, &cx);
        assert_eq!(kinds(&out), vec![]);
    }

    #[test]
    fn params_normalize() {
        // Reversed channels swap; a bend range of 0 falls back to 48, so
        // +50 cents still maps to 85.
        let mut p = pool(3, 1, 0.0);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let a = p.note_on(0, 60, 50.0, 100, &mut out, &cx);
        assert_eq!(a.ch, 1);
        assert_eq!(out[0].kind, bend(1, 85));
    }
}
