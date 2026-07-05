//! Decaying repeats, optionally transposed into a canon cascade.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Repeat every note `repeats` times at `delta_ns` spacing, each copy
/// quieter by a factor of `decay` and shifted by `transpose` semitones per
/// step. With `transpose == 0` this is a plain echo; with a nonzero
/// transpose each voice enters later and higher (or lower), a canon cascade
/// in the spirit of Webern's mirror canons or Ligeti's self-shadowing
/// lattices.
///
/// The original note passes unchanged. Copy `k` (1..=repeats) lands at
/// `time + k * delta_ns` on key `key + k * transpose`, with note-on
/// velocity `vel * decay^k` rounded and clamped to 1..=127. A copy whose
/// key leaves 0..=127 is dropped, and because note-offs get the identical
/// `(k, transpose)` treatment, the matching off drops with it, so nothing
/// orphans. Note-off copies keep their release velocity unscaled. Non-note
/// events pass through once, undelayed.
///
/// Needs no per-note state: every output depends only on the input event,
/// so on and off transform identically by construction.
///
/// Fanout bound: at most `1 + repeats` outputs per input, and `repeats`
/// is clamped to 16, so 17 events, well under `MAX_FANOUT`.
///
/// Overflow caveat: a copy's note-on and note-off come from different
/// input events, so unlike restrike and stutter they cannot be pushed as
/// an all-or-nothing pair. If an upstream fanout fills the buffer, a
/// copy's on or off can drop alone; a lone echo never gets close to that
/// limit by itself.
pub struct Echo {
    repeats: u8,
    delta_ns: u64,
    decay: f32,
    transpose: i16,
}

impl Echo {
    /// `repeats` is clamped to 1..=16.
    pub fn new(repeats: u8, delta_ns: u64, decay: f32, transpose: i16) -> Self {
        Self {
            repeats: repeats.clamp(1, 16),
            delta_ns,
            decay,
            transpose,
        }
    }

    /// Key of the k-th copy, or `None` when it leaves the keyboard.
    fn shift(&self, key: u8, k: u8) -> Option<u8> {
        let shifted = key as i32 + k as i32 * self.transpose as i32;
        (0..=127).contains(&shifted).then_some(shifted as u8)
    }

    /// Note-on velocity of the k-th copy. Never zero: velocity 0 would
    /// read as a note-off on the wire.
    fn scaled(&self, vel: u8, k: u8) -> u8 {
        let v = vel as f32 * self.decay.powi(k as i32);
        v.round().clamp(1.0, 127.0) as u8
    }
}

impl Effect for Echo {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        push(out, cx, *ev);
        let (ch, key, vel, on) = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => (ch, key, vel, true),
            EventKind::NoteOff { ch, key, vel } => (ch, key, vel, false),
            _ => return,
        };
        for k in 1..=self.repeats {
            let Some(key) = self.shift(key, k) else {
                continue;
            };
            let kind = if on {
                let vel = self.scaled(vel, k);
                EventKind::NoteOn { ch, key, vel }
            } else {
                EventKind::NoteOff { ch, key, vel }
            };
            let time = ev.time.saturating_add(k as u64 * self.delta_ns);
            push(out, cx, Event::new(time, kind));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run, run_timed};

    fn on_vel(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    #[test]
    fn copies_land_at_multiples_of_delta_with_decayed_velocity() {
        let mut fx = Echo::new(3, 1_000, 0.5, 0);
        assert_eq!(
            run_timed(&mut fx, 10_000, on_vel(60, 100)),
            vec![
                at(10_000, on_vel(60, 100)),
                at(11_000, on_vel(60, 50)),
                at(12_000, on_vel(60, 25)),
                at(13_000, on_vel(60, 13)),
            ]
        );
    }

    #[test]
    fn note_off_copies_keep_release_velocity() {
        let mut fx = Echo::new(2, 500, 0.5, 0);
        let release = EventKind::NoteOff {
            ch: 0,
            key: 60,
            vel: 90,
        };
        assert_eq!(
            run_timed(&mut fx, 0, release),
            vec![at(0, release), at(500, release), at(1_000, release)]
        );
    }

    #[test]
    fn transpose_shifts_each_copy_further() {
        let mut fx = Echo::new(2, 100, 1.0, 7);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(100, on(67)), at(200, on(74))]
        );
    }

    #[test]
    fn out_of_range_copies_drop_the_same_way_for_on_and_off() {
        // 110 + 12 = 122 stays; 110 + 24 = 134 leaves the keyboard. The
        // same k must drop for the note-on and its note-off.
        let mut fx = Echo::new(4, 100, 1.0, 12);
        assert_eq!(
            run_timed(&mut fx, 0, on(110)),
            vec![at(0, on(110)), at(100, on(122))]
        );
        assert_eq!(
            run_timed(&mut fx, 50, off(110)),
            vec![at(50, off(110)), at(150, off(122))]
        );
    }

    #[test]
    fn velocity_never_falls_below_one() {
        let mut fx = Echo::new(4, 1, 0.01, 0);
        let out = run_timed(&mut fx, 0, on_vel(60, 100));
        for ev in &out[1..] {
            match ev.kind {
                EventKind::NoteOn { vel, .. } => assert!(vel >= 1),
                other => panic!("expected a note-on, got {other:?}"),
            }
        }
    }

    #[test]
    fn repeats_clamp_to_one_and_sixteen() {
        let mut fx = Echo::new(0, 1, 1.0, 0);
        assert_eq!(run(&mut fx, on(60)).len(), 2);
        let mut fx = Echo::new(u8::MAX, 1, 1.0, 0);
        assert_eq!(run(&mut fx, on(60)).len(), 17);
    }

    #[test]
    fn non_note_events_pass_once_undelayed() {
        let mut fx = Echo::new(8, 1_000, 0.5, 0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 77, pedal), vec![at(77, pedal)]);
    }
}
