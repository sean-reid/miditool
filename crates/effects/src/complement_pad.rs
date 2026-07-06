//! The aggregate's other half: a pad sounding the unplayed pitch classes.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::push;

/// Sustain the chromatic complement of whatever the player holds, the
/// total-chromatic bookkeeping of Carter and Schoenberg turned into a
/// drone: while at least one input note is held, a soft pad (velocity
/// `vel`, on channel 0) sounds the lowest key in `lo..=hi` for every
/// pitch class absent from the held set, so player plus pad always
/// complete the aggregate. Pitch classes with no key in the range are
/// skipped. When the player holds nothing the pad is silent; sounding the
/// complement of the empty set would drone all twelve classes forever.
///
/// The player's events pass through unchanged (any channel); held input
/// notes are counted per (channel, key), so retriggers and overlapping
/// channels holding the same pitch class resolve correctly. On every
/// note-on and note-off the wanted pad set is recomputed and diffed
/// against the sounding one: pad note-offs are emitted before the
/// pass-through event and pad note-ons after it, so a pad note that
/// collides with a player key (or replaces one just released) never gets
/// its on and off reordered. `flush` releases the whole pad plus one
/// note-off per held pass-through note-on, leaving nothing sounding.
///
/// Fanout bound: 1 pass-through plus at most 11 pad changes per input
/// event (the pad never holds more than 11 notes, since at least one
/// pitch class is always held while it sounds), well under `MAX_FANOUT`.
pub struct ComplementPad {
    lo: u8,
    hi: u8,
    vel: u8,
    /// Held note-on count per input (channel, key).
    held: PerNote<u8>,
    /// How many held (channel, key) slots sound each pitch class.
    pc_count: [u16; 12],
    /// Total held (channel, key) slots.
    total: u16,
    /// The sounding pad key per pitch class, all on channel 0.
    pad: [Option<u8>; 12],
}

impl ComplementPad {
    /// `lo` and `hi` are clamped to 127 and swapped if reversed; `vel` is
    /// clamped to 1..=127.
    pub fn new(lo: u8, hi: u8, vel: u8) -> Self {
        let (lo, hi) = (lo.min(127), hi.min(127));
        Self {
            lo: lo.min(hi),
            hi: lo.max(hi),
            vel: vel.clamp(1, 127),
            held: PerNote::new(),
            pc_count: [0; 12],
            total: 0,
            pad: [None; 12],
        }
    }

    /// The lowest key in `lo..=hi` with pitch class `pc`, if any.
    fn pad_key(&self, pc: u8) -> Option<u8> {
        let base = self.lo as u16 + (pc as u16 + 12 - self.lo as u16 % 12) % 12;
        (base <= self.hi as u16).then_some(base as u8)
    }

    /// The pad key that should currently sound for `pc`: none while the
    /// player holds nothing or holds the class themselves.
    fn wanted(&self, pc: u8) -> Option<u8> {
        if self.total == 0 || self.pc_count[pc as usize] > 0 {
            return None;
        }
        self.pad_key(pc)
    }

    /// Release pad notes no longer wanted.
    fn retire(&mut self, time: u64, out: &mut EventBuf, cx: &ProcCx) {
        for pc in 0..12u8 {
            let slot = pc as usize;
            if let Some(key) = self.pad[slot]
                && self.wanted(pc) != Some(key)
            {
                self.pad[slot] = None;
                let kind = EventKind::NoteOff { ch: 0, key, vel: 0 };
                push(out, cx, Event::new(time, kind));
            }
        }
    }

    /// Start pad notes newly wanted.
    fn awaken(&mut self, time: u64, out: &mut EventBuf, cx: &ProcCx) {
        for pc in 0..12u8 {
            let slot = pc as usize;
            if self.pad[slot].is_none()
                && let Some(key) = self.wanted(pc)
            {
                self.pad[slot] = Some(key);
                let kind = EventKind::NoteOn {
                    ch: 0,
                    key,
                    vel: self.vel,
                };
                push(out, cx, Event::new(time, kind));
            }
        }
    }
}

impl Effect for ComplementPad {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, .. } => {
                let n = self.held.get(ch, key);
                self.held.set(ch, key, n.saturating_add(1));
                if n == 0 {
                    self.pc_count[(key % 12) as usize] += 1;
                    self.total += 1;
                }
                self.retire(ev.time, out, cx);
                push(out, cx, *ev);
                self.awaken(ev.time, out, cx);
            }
            EventKind::NoteOff { ch, key, .. } => {
                let n = self.held.get(ch, key);
                if n > 0 {
                    self.held.set(ch, key, n - 1);
                    if n == 1 {
                        self.pc_count[(key % 12) as usize] -= 1;
                        self.total -= 1;
                    }
                }
                self.retire(ev.time, out, cx);
                push(out, cx, *ev);
                self.awaken(ev.time, out, cx);
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        for slot in self.pad.iter_mut() {
            if let Some(key) = slot.take() {
                let kind = EventKind::NoteOff { ch: 0, key, vel: 0 };
                push(out, cx, Event::new(cx.now, kind));
            }
        }
        // The player's note-ons passed through unchanged, so wind them
        // down too: one note-off per outstanding pass-through note-on.
        let held = std::mem::take(&mut self.held);
        held.for_each(|ch, key, n| {
            for _ in 0..n {
                let kind = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(cx.now, kind));
            }
        });
        self.pc_count = [0; 12];
        self.total = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flush, off, on, run};

    fn pad_on(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    #[test]
    fn the_first_held_note_wakes_the_complement() {
        let mut fx = ComplementPad::new(72, 83, 40);
        // Holding C (pc 0): the pad sounds pcs 1..=11 at keys 73..=83.
        let mut expected = vec![on(60)];
        expected.extend((73..=83).map(|key| pad_on(key, 40)));
        assert_eq!(run(&mut fx, on(60)), expected);
    }

    #[test]
    fn releasing_the_last_note_silences_the_pad() {
        let mut fx = ComplementPad::new(72, 83, 40);
        run(&mut fx, on(60));
        // The pad retires before the pass-through off.
        let mut expected: Vec<EventKind> = (73..=83).map(off).collect();
        expected.push(off(60));
        assert_eq!(run(&mut fx, off(60)), expected);
        // And nothing sounds while nothing is held.
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn a_chord_change_diffs_the_pad() {
        let mut fx = ComplementPad::new(72, 83, 40);
        run(&mut fx, on(60));
        // Adding E (pc 4) only retires the pad's pc 4 (key 76), before
        // the pass-through.
        assert_eq!(run(&mut fx, on(64)), vec![off(76), on(64)]);
        // Releasing C frees pc 0: the pad key 72 wakes after the
        // pass-through.
        assert_eq!(run(&mut fx, off(60)), vec![off(60), pad_on(72, 40)]);
        // Releasing E empties the held set: the whole pad retires, then
        // the pass-through off.
        let out = run(&mut fx, off(64));
        assert_eq!(out.last(), Some(&off(64)));
        let mut pad = out[..out.len() - 1].to_vec();
        pad.sort_by_key(|kind| kind.key());
        let expected: Vec<EventKind> = (72..=83).filter(|&k| k != 76).map(off).collect();
        assert_eq!(pad, expected);
    }

    #[test]
    fn two_keys_sharing_a_pitch_class_keep_it_out_of_the_pad() {
        let mut fx = ComplementPad::new(0, 11, 40);
        run(&mut fx, on(60));
        // A second C (pc 0) an octave down changes nothing.
        assert_eq!(run(&mut fx, on(48)), vec![on(48)]);
        // Releasing one of them still leaves pc 0 held.
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        // Releasing the other silences the pad.
        let out = run(&mut fx, off(48));
        assert_eq!(out.len(), 1 + 11);
    }

    #[test]
    fn retriggers_are_counted() {
        let mut fx = ComplementPad::new(0, 11, 40);
        run(&mut fx, on(60));
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        // The first off leaves one hold outstanding.
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        // The second empties the held set and the pad follows.
        assert_eq!(run(&mut fx, off(60)).len(), 1 + 11);
    }

    #[test]
    fn a_narrow_range_skips_unavailable_pitch_classes() {
        let mut fx = ComplementPad::new(60, 63, 40);
        // Holding F (pc 5): only pcs 0..=3 have keys in 60..=63.
        assert_eq!(
            run(&mut fx, on(65)),
            vec![
                on(65),
                pad_on(60, 40),
                pad_on(61, 40),
                pad_on(62, 40),
                pad_on(63, 40)
            ]
        );
    }

    #[test]
    fn the_pad_takes_the_lowest_key_per_pitch_class() {
        let mut fx = ComplementPad::new(59, 83, 40);
        let out = run(&mut fx, on(48));
        // pc 11 sits at 59 itself; pc 1 at 61, not 73.
        assert!(out.contains(&pad_on(59, 40)));
        assert!(out.contains(&pad_on(61, 40)));
        assert!(!out.contains(&pad_on(73, 40)));
    }

    #[test]
    fn a_pad_note_colliding_with_a_player_key_is_retired_first() {
        let mut fx = ComplementPad::new(60, 71, 40);
        run(&mut fx, on(60));
        // The pad sounds 64 (pc 4); the player now presses it. The pad
        // off precedes the pass-through on, so the key ends up sounding.
        assert_eq!(run(&mut fx, on(64)), vec![off(64), on(64)]);
        // Releasing it: the pass-through off precedes the pad's re-on.
        assert_eq!(run(&mut fx, off(64)), vec![off(64), pad_on(64, 40)]);
    }

    #[test]
    fn pass_through_keeps_channel_and_velocity() {
        let mut fx = ComplementPad::new(72, 83, 40);
        let played = EventKind::NoteOn {
            ch: 3,
            key: 60,
            vel: 9,
        };
        let out = run(&mut fx, played);
        assert_eq!(out[0], played);
        // The pad itself sounds on channel 0.
        for kind in &out[1..] {
            assert!(
                matches!(kind, EventKind::NoteOn { ch: 0, vel: 40, .. }),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn velocity_clamps_into_one_to_127() {
        let mut fx = ComplementPad::new(72, 83, 0);
        let out = run(&mut fx, on(60));
        assert!(matches!(out[1], EventKind::NoteOn { vel: 1, .. }));
    }

    #[test]
    fn orphan_note_off_passes_through_without_pad_changes() {
        let mut fx = ComplementPad::new(72, 83, 40);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn flush_releases_the_pad_and_the_held_notes() {
        let mut fx = ComplementPad::new(72, 83, 40);
        run(&mut fx, on(60));
        run(&mut fx, on(60));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        // Two pass-through offs for the doubled C, then the 11 pad keys.
        let mut expected = vec![off(60), off(60)];
        expected.extend((73..=83).map(off));
        assert_eq!(released, expected);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn every_event_stays_within_the_fanout_bound() {
        let mut fx = ComplementPad::new(0, 127, 40);
        for kind in [on(60), on(64), off(60), off(64), on(61), off(61)] {
            assert!(run(&mut fx, kind).len() <= 12);
        }
    }

    #[test]
    fn other_events_pass() {
        let mut fx = ComplementPad::new(72, 83, 40);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
