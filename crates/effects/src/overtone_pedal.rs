//! Under the pedal, every note snaps to one fundamental's harmonic series.

use miditool_core::event::CC_SUSTAIN;
use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::mpe::{MpeParams, MpeVoices, Route};
use crate::router::push;

/// Snap the keyboard to the overtone series of a fundamental while the
/// sustain pedal is down, one string's worth of harmonics under the
/// whole hand: CC64 is tracked per channel from the stream (64 or higher
/// is down; the CC itself passes through). While the pedal is down on a
/// note's channel, each note-on is compared against the partials
/// `k` in 1..=`max_partial` of `fundamental`, which sit at
/// `12 * log2(k)` semitones above it; the nearest partial (smallest `k`
/// on a tie) wins, and the note is re-emitted through an MPE voice pool
/// at `fundamental + round(semis_k)` with the remainder as a per-note
/// pitch bend in cents. Notes below the fundamental, notes with no
/// partial within 6 semitones, and snaps that would leave 0..=127 pass
/// dry instead. With the pedal up everything passes dry.
///
/// Each note's record remembers dry-versus-voice, so its note-off
/// releases the right thing even if the pedal moved between the on and
/// the off; a retrigger cuts first; `flush` releases the dry notes,
/// releases the pool, and resets its bends. An orphan note-off passes
/// dry: the pedal may have moved since its note-on, so the dry guess is
/// the only stateless one. Non-note events pass unchanged; in particular
/// a pitch bend from the player only affects the dry notes on the
/// original channel, never the pool's member channels.
///
/// Fanout bound: at most 1 retrigger cut, 1 steal-off, and the voice's
/// bend and note-on per input event, 4 events, well under `MAX_FANOUT`.
pub struct OvertonePedal {
    fundamental: u8,
    max_partial: u8,
    pool: MpeVoices,
    /// Channels whose sustain pedal is down, one bit per channel.
    sustain_down: u16,
    /// How each active input (channel, key) was routed.
    active: PerNote<Route>,
}

impl OvertonePedal {
    /// `fundamental` is clamped to 127 and `max_partial` to 1..=32.
    pub fn new(fundamental: u8, max_partial: u8, mpe: MpeParams) -> Self {
        Self {
            fundamental: fundamental.min(127),
            max_partial: max_partial.clamp(1, 32),
            pool: MpeVoices::new(mpe),
            sustain_down: 0,
            active: PerNote::new(),
        }
    }

    /// The harmonic snap for an input key: `Some((key_out, cents))` when
    /// a partial of the fundamental lies within 6 semitones on the
    /// keyboard, `None` to pass dry.
    fn snap(&self, key: u8) -> Option<(u8, f32)> {
        if key < self.fundamental {
            return None;
        }
        let offset = (key - self.fundamental) as f32;
        let mut best: Option<(f32, f32)> = None;
        for k in 1..=self.max_partial {
            let semis = 12.0 * (k as f32).log2();
            let distance = (offset - semis).abs();
            if best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, semis));
            }
        }
        let (distance, semis) = best?;
        if distance > 6.0 {
            return None;
        }
        let key_out = self.fundamental as i32 + semis.round() as i32;
        let cents = 100.0 * (semis - semis.round());
        (0..=127)
            .contains(&key_out)
            .then_some((key_out as u8, cents))
    }
}

impl Effect for OvertonePedal {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::ControlChange {
                ch,
                cc: CC_SUSTAIN,
                value,
            } => {
                if value >= 64 {
                    self.sustain_down |= 1 << ch;
                } else {
                    self.sustain_down &= !(1 << ch);
                }
                push(out, cx, *ev);
            }
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut whatever the previous strike left, on
                // whichever path it took.
                match self.active.take(ch, key) {
                    Route::Silent => {}
                    Route::Dry => {
                        let cut = EventKind::NoteOff { ch, key, vel: 0 };
                        push(out, cx, Event::new(ev.time, cut));
                    }
                    Route::Tuned(voice) => self.pool.note_off(ev.time, voice, out, cx),
                }
                let snapped = if self.sustain_down & (1 << ch) != 0 {
                    self.snap(key)
                } else {
                    None
                };
                match snapped {
                    Some((key_out, cents)) => {
                        let voice = self.pool.note_on(ev.time, key_out, cents, vel, out, cx);
                        self.active.set(ch, key, Route::Tuned(voice));
                    }
                    None => {
                        push(out, cx, *ev);
                        self.active.set(ch, key, Route::Dry);
                    }
                }
            }
            EventKind::NoteOff { ch, key, .. } => match self.active.take(ch, key) {
                Route::Tuned(voice) => self.pool.note_off(ev.time, voice, out, cx),
                // An orphan off passes dry: the pedal may have moved
                // since the note-on, so dry is the only stateless guess.
                Route::Dry | Route::Silent => push(out, cx, *ev),
            },
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        // One note-off per dry pass-through; the pool releases its own
        // voices and resets the bends. The pedal state is stream-derived,
        // so it survives the flush untouched.
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

    fn pedal(ch: u8, value: u8) -> EventKind {
        EventKind::ControlChange { ch, cc: 64, value }
    }

    #[test]
    fn pedal_up_passes_everything_dry() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        assert_eq!(run(&mut fx, on(67)), vec![on(67)]);
        assert_eq!(run(&mut fx, off(67)), vec![off(67)]);
    }

    #[test]
    fn pedal_down_snaps_to_the_nearest_partial() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        assert_eq!(run(&mut fx, pedal(0, 127)), vec![pedal(0, 127)]);
        // Key 67 is 19 above the fundamental: partial 3 at 19.02, so it
        // stays on 67 with +1.955 cents (bend 3 at range 48).
        assert_eq!(run(&mut fx, on(67)), vec![bend(1, 3), von(1, 67, 100)]);
        // Key 64 (offset 16) is closer to partial 3 (19.02) than partial
        // 2 (12): it snaps UP to key 67.
        assert_eq!(run(&mut fx, on(64)), vec![bend(2, 3), von(2, 67, 100)]);
        // Key 76 (offset 28) hits partial 5 at 27.86: -13.7 cents.
        assert_eq!(run(&mut fx, on(76)), vec![bend(3, -23), von(3, 76, 100)]);
        // The fundamental itself is partial 1, bend 0, still pooled.
        assert_eq!(run(&mut fx, on(48)), vec![bend(4, 0), von(4, 48, 100)]);
        // Note-offs release the snapped voices, not the input keys.
        assert_eq!(run(&mut fx, off(64)), vec![voff(2, 67)]);
        assert_eq!(run(&mut fx, off(67)), vec![voff(1, 67)]);
    }

    #[test]
    fn notes_below_the_fundamental_pass_dry() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        run(&mut fx, pedal(0, 127));
        assert_eq!(run(&mut fx, on(46)), vec![on(46)]);
        assert_eq!(run(&mut fx, off(46)), vec![off(46)]);
    }

    #[test]
    fn a_note_far_from_every_partial_passes_dry() {
        // With only partial 1, key 55 (offset 7) is 7 semitones from the
        // nearest candidate: over the 6-semitone limit, so it passes dry.
        let mut fx = OvertonePedal::new(48, 1, mpe(1, 8));
        run(&mut fx, pedal(0, 127));
        assert_eq!(run(&mut fx, on(55)), vec![on(55)]);
        // Allowing partial 2 (offset 12, distance 5) pulls it in.
        let mut fx = OvertonePedal::new(48, 2, mpe(1, 8));
        run(&mut fx, pedal(0, 127));
        assert_eq!(run(&mut fx, on(55)), vec![bend(1, 0), von(1, 60, 100)]);
    }

    #[test]
    fn the_pedal_is_tracked_per_channel() {
        let mut fx = OvertonePedal::new(48, 8, mpe(2, 8));
        run(&mut fx, pedal(1, 127));
        // Channel 0's pedal is up: dry.
        assert_eq!(run(&mut fx, on(67)), vec![on(67)]);
        // Channel 1's is down: snapped.
        let on_ch1 = EventKind::NoteOn {
            ch: 1,
            key: 67,
            vel: 100,
        };
        assert_eq!(run(&mut fx, on_ch1), vec![bend(2, 3), von(2, 67, 100)]);
    }

    #[test]
    fn the_record_survives_pedal_moves_between_on_and_off() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        // Snapped under the pedal, released after it lifts: the pool
        // voice still gets its off.
        run(&mut fx, pedal(0, 127));
        run(&mut fx, on(67));
        assert_eq!(run(&mut fx, pedal(0, 0)), vec![pedal(0, 0)]);
        assert_eq!(run(&mut fx, off(67)), vec![voff(1, 67)]);
        // Struck dry, released under the pedal: the dry off passes.
        run(&mut fx, on(60));
        run(&mut fx, pedal(0, 127));
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn retriggers_cut_on_both_paths() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        run(&mut fx, on(67));
        // Dry retrigger while the pedal comes down: the dry cut precedes
        // the snapped restrike.
        run(&mut fx, pedal(0, 127));
        assert_eq!(
            run(&mut fx, on(67)),
            vec![off(67), bend(1, 3), von(1, 67, 100)]
        );
        // Snapped retrigger: the voice is cut and re-struck.
        assert_eq!(
            run(&mut fx, on(67)),
            vec![voff(1, 67), bend(1, 3), von(1, 67, 100)]
        );
    }

    #[test]
    fn parameters_clamp() {
        // max_partial 0 clamps to 1: only the fundamental snaps.
        let mut fx = OvertonePedal::new(48, 0, mpe(1, 8));
        run(&mut fx, pedal(0, 127));
        assert_eq!(run(&mut fx, on(48)), vec![bend(1, 0), von(1, 48, 100)]);
        assert_eq!(run(&mut fx, on(55)), vec![on(55)]);
    }

    #[test]
    fn flush_releases_dry_notes_and_the_pool() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        run(&mut fx, on(40));
        run(&mut fx, pedal(0, 127));
        run(&mut fx, on(67));
        assert_eq!(flush(&mut fx), vec![off(40), voff(1, 67), bend(1, 0)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = OvertonePedal::new(48, 8, mpe(1, 8));
        run(&mut fx, pedal(0, 127));
        let player_bend = EventKind::PitchBend { ch: 0, value: 512 };
        assert_eq!(run(&mut fx, player_bend), vec![player_bend]);
        let soft = EventKind::ControlChange {
            ch: 0,
            cc: 67,
            value: 127,
        };
        assert_eq!(run(&mut fx, soft), vec![soft]);
    }
}
