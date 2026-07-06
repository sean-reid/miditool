//! Prepared tuning: a fixed detune per pitch class, Cage-adjacent.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::mpe::{MpeParams, MpeVoices, Route};
use crate::router::push;

/// Retune the keyboard with a fixed per-pitch-class detune map, the
/// keyboard cousin of a prepared piano or a scordatura string: each of
/// the twelve pitch classes carries a cents offset, applied to every
/// octave. Pitch classes mapped to 0 cents pass dry on their own channel
/// (no pool voice spent on a note that needs no bend); nonzero classes
/// are re-emitted through an MPE voice pool at their own key with the
/// mapped cents as a per-note pitch bend.
///
/// The matching note-off follows whichever path the note-on took, even
/// though the record is what remembers it, not the map; a retrigger cuts
/// first; `flush` releases the dry notes, releases the pool, and resets
/// its bends. An orphan note-off for a zero-cents class passes dry (the
/// map is deterministic there); one for a detuned class is dropped, since
/// pool channels are assigned dynamically. Non-note events pass
/// unchanged; in particular a pitch bend from the player only affects
/// the dry notes on the original channel, never the pool's members.
///
/// Fanout bound: at most 1 retrigger cut, 1 steal-off, and the voice's
/// bend and note-on per input event, 4 events, well under `MAX_FANOUT`.
pub struct Scordatura {
    /// Cents per pitch class, clamped to -100..=100.
    cents: [i16; 12],
    pool: MpeVoices,
    /// How each active input (channel, key) was routed.
    active: PerNote<Route>,
}

impl Scordatura {
    /// Each entry of `cents` is clamped to -100..=100.
    pub fn new(cents: [i16; 12], mpe: MpeParams) -> Self {
        let mut cents = cents;
        for entry in &mut cents {
            *entry = (*entry).clamp(-100, 100);
        }
        Self {
            cents,
            pool: MpeVoices::new(mpe),
            active: PerNote::new(),
        }
    }

    fn detune(&self, key: u8) -> i16 {
        self.cents[(key % 12) as usize]
    }
}

impl Effect for Scordatura {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut whatever the previous strike left.
                match self.active.take(ch, key) {
                    Route::Silent => {}
                    Route::Dry => {
                        let cut = EventKind::NoteOff { ch, key, vel: 0 };
                        push(out, cx, Event::new(ev.time, cut));
                    }
                    Route::Tuned(voice) => self.pool.note_off(ev.time, voice, out, cx),
                }
                let detune = self.detune(key);
                if detune == 0 {
                    push(out, cx, *ev);
                    self.active.set(ch, key, Route::Dry);
                } else {
                    let voice = self.pool.note_on(ev.time, key, detune as f32, vel, out, cx);
                    self.active.set(ch, key, Route::Tuned(voice));
                }
            }
            EventKind::NoteOff { ch, key, .. } => match self.active.take(ch, key) {
                Route::Dry => push(out, cx, *ev),
                Route::Tuned(voice) => self.pool.note_off(ev.time, voice, out, cx),
                // Orphan: a zero-cents class maps statelessly to the dry
                // path; a detuned class has no recoverable pool channel.
                Route::Silent => {
                    if self.detune(key) == 0 {
                        push(out, cx, *ev);
                    }
                }
            },
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        // One note-off per dry pass-through; the pool releases its own
        // voices and resets the bends.
        let active = std::mem::take(&mut self.active);
        active.for_each(|ch, key, route| {
            if route == Route::Dry {
                let kind = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(cx.now, kind));
            }
        });
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

    /// C detuned +50 cents, D detuned -100, everything else untouched.
    fn map() -> [i16; 12] {
        let mut cents = [0i16; 12];
        cents[0] = 50;
        cents[2] = -100;
        cents
    }

    #[test]
    fn zero_cents_classes_pass_dry() {
        let mut fx = Scordatura::new(map(), mpe(1, 8));
        assert_eq!(run(&mut fx, on(64)), vec![on(64)]);
        assert_eq!(run(&mut fx, off(64)), vec![off(64)]);
    }

    #[test]
    fn detuned_classes_go_through_the_pool() {
        let mut fx = Scordatura::new(map(), mpe(1, 8));
        // +50 cents at range 48 is bend 85, every octave of the class.
        assert_eq!(run(&mut fx, on(60)), vec![bend(1, 85), von(1, 60, 100)]);
        assert_eq!(run(&mut fx, on(72)), vec![bend(2, 85), von(2, 72, 100)]);
        // -100 cents is bend -171.
        assert_eq!(run(&mut fx, on(62)), vec![bend(3, -171), von(3, 62, 100)]);
        assert_eq!(run(&mut fx, off(60)), vec![voff(1, 60)]);
        assert_eq!(run(&mut fx, off(62)), vec![voff(3, 62)]);
    }

    #[test]
    fn entries_clamp_to_a_semitone() {
        let mut cents = [0i16; 12];
        cents[0] = 300;
        cents[1] = -300;
        let mut fx = Scordatura::new(cents, mpe(1, 8));
        // Clamped to +100 and -100 cents: bends 171 and -171.
        assert_eq!(run(&mut fx, on(60)), vec![bend(1, 171), von(1, 60, 100)]);
        assert_eq!(run(&mut fx, on(61)), vec![bend(2, -171), von(2, 61, 100)]);
    }

    #[test]
    fn retriggers_cut_on_both_paths() {
        let mut fx = Scordatura::new(map(), mpe(1, 8));
        run(&mut fx, on(64));
        assert_eq!(run(&mut fx, on(64)), vec![off(64), on(64)]);
        run(&mut fx, on(60));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![voff(1, 60), bend(1, 85), von(1, 60, 100)]
        );
    }

    #[test]
    fn orphan_note_offs_follow_the_map() {
        let mut fx = Scordatura::new(map(), mpe(1, 8));
        // Zero-cents class: statelessly dry.
        assert_eq!(run(&mut fx, off(64)), vec![off(64)]);
        // Detuned class: no recoverable pool channel, dropped.
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_dry_notes_and_the_pool() {
        let mut fx = Scordatura::new(map(), mpe(1, 8));
        run(&mut fx, on(60));
        run(&mut fx, on(64));
        assert_eq!(flush(&mut fx), vec![off(64), voff(1, 60), bend(1, 0)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = Scordatura::new(map(), mpe(1, 8));
        let player_bend = EventKind::PitchBend { ch: 0, value: 512 };
        assert_eq!(run(&mut fx, player_bend), vec![player_bend]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
