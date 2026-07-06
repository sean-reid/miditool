//! 5-limit just intonation: pure thirds and fifths against a root.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::mpe::{MpeParams, MpeVoices, Voice};
use crate::router::push;

/// Cents deviation from equal temperament per interval in semitones
/// above the root, the classic 5-limit chromatic scale. Each entry lists
/// the just ratio it tunes to.
const JUST_CENTS: [f32; 12] = [
    0.0,    // 0: unison, 1/1
    11.73,  // 1: minor second, 16/15
    3.91,   // 2: major second, 9/8
    15.64,  // 3: minor third, 6/5
    -13.69, // 4: major third, 5/4
    -1.96,  // 5: perfect fourth, 4/3
    -9.78,  // 6: tritone, 45/32
    1.96,   // 7: perfect fifth, 3/2
    13.69,  // 8: minor sixth, 8/5
    -15.64, // 9: major sixth, 5/3
    17.60,  // 10: minor seventh, 9/5
    -11.73, // 11: major seventh, 15/8
];

/// Retune the keyboard to 5-limit just intonation relative to `root_pc`:
/// every note-on is re-emitted through an MPE voice pool at its own key,
/// detuned by the `JUST_CENTS` entry for its interval above the root, so
/// thirds and fifths against the root ring pure. Notes whose offset is
/// 0.0 (only the root pitch class) still go through the pool: uniform
/// behavior is simpler to reason about than a special dry path.
///
/// The matching note-off releases the pool voice; a retrigger cuts it
/// first; `flush` releases everything and resets the pool's bends. An
/// orphan note-off is dropped: pool channels are assigned dynamically,
/// so there is nothing stateless to map it to. Non-note events pass
/// unchanged; in particular a pitch bend from the player passes through
/// on its original channel and never reaches the pool's member channels,
/// where the notes actually sound.
///
/// Fanout bound: at most 1 retrigger cut, 1 steal-off, and the voice's
/// bend and note-on per input event, 4 events, well under `MAX_FANOUT`.
pub struct Just {
    root_pc: u8,
    pool: MpeVoices,
    /// The pool voice per active input (channel, key).
    active: PerNote<Option<Voice>>,
}

impl Just {
    /// `root_pc` is reduced modulo 12.
    pub fn new(root_pc: u8, mpe: MpeParams) -> Self {
        Self {
            root_pc: root_pc % 12,
            pool: MpeVoices::new(mpe),
            active: PerNote::new(),
        }
    }

    /// The cents deviation for a key, by its interval above the root.
    fn cents(&self, key: u8) -> f32 {
        JUST_CENTS[((key % 12 + 12 - self.root_pc) % 12) as usize]
    }
}

impl Effect for Just {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                if let Some(prev) = self.active.take(ch, key) {
                    self.pool.note_off(ev.time, prev, out, cx);
                }
                let voice = self
                    .pool
                    .note_on(ev.time, key, self.cents(key), vel, out, cx);
                self.active.set(ch, key, Some(voice));
            }
            EventKind::NoteOff { ch, key, .. } => {
                if let Some(voice) = self.active.take(ch, key) {
                    self.pool.note_off(ev.time, voice, out, cx);
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        // The records only hold pool handles; the pool itself releases
        // every active voice and resets the bends.
        self.active = PerNote::new();
        self.pool.flush(cx.now, out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flush, off, on, run};

    fn mpe(lo: u8, hi: u8) -> MpeParams {
        MpeParams {
            lo,
            hi,
            bend_range: 48.0,
        }
    }

    fn bend(ch: u8, value: i16) -> EventKind {
        EventKind::PitchBend { ch, value }
    }

    fn von(ch: u8, key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch, key, vel }
    }

    fn voff(ch: u8, key: u8) -> EventKind {
        EventKind::NoteOff { ch, key, vel: 0 }
    }

    #[test]
    fn intervals_follow_the_five_limit_table() {
        let mut fx = Just::new(0, mpe(1, 8));
        // The root itself still goes through the pool, bend 0.
        assert_eq!(run(&mut fx, on(60)), vec![bend(1, 0), von(1, 60, 100)]);
        // Major third, 5/4: -13.69 cents is bend -23 at range 48.
        assert_eq!(run(&mut fx, on(64)), vec![bend(2, -23), von(2, 64, 100)]);
        // Perfect fifth, 3/2: +1.96 cents is bend 3.
        assert_eq!(run(&mut fx, on(67)), vec![bend(3, 3), von(3, 67, 100)]);
        // Minor seventh, 9/5: +17.60 cents is bend 30.
        assert_eq!(run(&mut fx, on(70)), vec![bend(4, 30), von(4, 70, 100)]);
        // Minor second, 16/15: +11.73 cents is bend 20.
        assert_eq!(run(&mut fx, on(61)), vec![bend(5, 20), von(5, 61, 100)]);
    }

    #[test]
    fn the_root_pitch_class_anchors_the_table() {
        // Root A: C is a minor third above it, 6/5, +15.64 cents, bend 27.
        let mut fx = Just::new(9, mpe(1, 8));
        assert_eq!(run(&mut fx, on(60)), vec![bend(1, 27), von(1, 60, 100)]);
        // root_pc reduces modulo 12: 21 behaves like 9.
        let mut fx = Just::new(21, mpe(1, 8));
        assert_eq!(run(&mut fx, on(60)), vec![bend(1, 27), von(1, 60, 100)]);
    }

    #[test]
    fn the_note_off_releases_the_voice() {
        let mut fx = Just::new(0, mpe(1, 8));
        run(&mut fx, on(64));
        assert_eq!(run(&mut fx, off(64)), vec![voff(1, 64)]);
        // An orphan note-off is dropped.
        assert_eq!(run(&mut fx, off(64)), vec![]);
    }

    #[test]
    fn a_retrigger_cuts_the_previous_voice() {
        let mut fx = Just::new(0, mpe(1, 8));
        assert_eq!(run(&mut fx, on(64)), vec![bend(1, -23), von(1, 64, 100)]);
        assert_eq!(
            run(&mut fx, on(64)),
            vec![voff(1, 64), bend(1, -23), von(1, 64, 100)]
        );
        assert_eq!(run(&mut fx, off(64)), vec![voff(1, 64)]);
    }

    #[test]
    fn flush_releases_everything_and_resets_the_bends() {
        let mut fx = Just::new(0, mpe(1, 8));
        run(&mut fx, on(60));
        run(&mut fx, on(64));
        assert_eq!(
            flush(&mut fx),
            vec![voff(1, 60), voff(2, 64), bend(1, 0), bend(2, 0)]
        );
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = Just::new(0, mpe(1, 8));
        let player_bend = EventKind::PitchBend { ch: 0, value: -100 };
        assert_eq!(run(&mut fx, player_bend), vec![player_bend]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
