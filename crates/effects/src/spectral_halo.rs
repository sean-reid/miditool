//! Grisey spectral halo: the played fundamental plus detuned partials.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::mpe::{MpeParams, MpeVoices, Voice};
use crate::router::push;

/// The pool voices one input note holds, partials 2..=8 in order.
/// `Default` (all `None`) never marks an active note by itself: the
/// record is wrapped in `Option`, since a note whose partials all fell
/// off the keyboard still holds its dry fundamental.
type Partials = [Option<Voice>; 7];

/// Surround every played note with the upper partials of its harmonic
/// series, the instrumental synthesis of Grisey's Partiels: the played
/// note passes dry on its own channel as the fundamental, and partials
/// 2..=`partials` are added through an MPE voice pool so each carries its
/// own microtonal detune. Partial `k`'s frequency multiple is
/// `k^stretch`, so it sits `12 * stretch * log2(k)` semitones above the
/// fundamental (1.0 is the natural series; below compresses, above
/// stretches, like piano inharmonicity): the nearest key gets the
/// note-on and the remainder rides as a pitch bend in cents on the
/// voice's member channel. A partial whose key leaves 0..=127 is
/// skipped. Velocity rolls off geometrically: partial `k` plays at
/// `round(vel * rolloff^(k-1))`, at least 1.
///
/// The player's note-off releases the dry note and every pool voice its
/// note-on started; a retrigger cuts them first; `flush` releases
/// everything and resets the pool's bends. An orphan note-off passes dry
/// alone: pool channels are assigned dynamically, so there is nothing
/// stateless to map it to. Non-note events pass unchanged; in particular
/// a pitch bend from the player only affects the dry notes on the
/// original channel, never the pool's member channels.
///
/// Fanout bound: a fresh note-on is 1 dry note plus up to 7 voices at 2
/// events each (bend then on), 15 events; a retrigger adds the dry cut
/// and up to 7 releases, and a full pool adds one steal-off per voice,
/// at most 30 events, well under `MAX_FANOUT`.
pub struct SpectralHalo {
    partials: u8,
    rolloff: f32,
    stretch: f32,
    pool: MpeVoices,
    /// The pool voices per active input (channel, key); `Some` while the
    /// dry fundamental is sounding.
    active: PerNote<Option<Partials>>,
}

impl SpectralHalo {
    /// `partials` is clamped to 2..=8, `rolloff` to 0.0..=1.0, and
    /// `stretch` to 0.5..=2.0.
    pub fn new(partials: u8, rolloff: f32, stretch: f32, mpe: MpeParams) -> Self {
        Self {
            partials: partials.clamp(2, 8),
            rolloff: rolloff.clamp(0.0, 1.0),
            stretch: stretch.clamp(0.5, 2.0),
            pool: MpeVoices::new(mpe),
            active: PerNote::new(),
        }
    }

    /// Release every pool voice in a record.
    fn release(&mut self, time: u64, set: Partials, out: &mut EventBuf, cx: &ProcCx) {
        for voice in set.into_iter().flatten() {
            self.pool.note_off(time, voice, out, cx);
        }
    }
}

impl Effect for SpectralHalo {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut the dry fundamental and every partial the
                // previous strike left sounding.
                if let Some(prev) = self.active.take(ch, key) {
                    let cut = EventKind::NoteOff { ch, key, vel: 0 };
                    push(out, cx, Event::new(ev.time, cut));
                    self.release(ev.time, prev, out, cx);
                }
                push(out, cx, *ev);
                let mut set: Partials = [None; 7];
                for k in 2..=self.partials {
                    let semis = 12.0 * self.stretch * (k as f32).log2();
                    let nearest = semis.round();
                    let key_out = key as i32 + nearest as i32;
                    if !(0..=127).contains(&key_out) {
                        continue;
                    }
                    let cents = 100.0 * (semis - nearest);
                    let vel_k = (vel as f32 * self.rolloff.powi(k as i32 - 1))
                        .round()
                        .max(1.0) as u8;
                    let voice = self
                        .pool
                        .note_on(ev.time, key_out as u8, cents, vel_k, out, cx);
                    set[(k - 2) as usize] = Some(voice);
                }
                self.active.set(ch, key, Some(set));
            }
            EventKind::NoteOff { ch, key, .. } => {
                push(out, cx, *ev);
                if let Some(prev) = self.active.take(ch, key) {
                    self.release(ev.time, prev, out, cx);
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        // One dry note-off per active fundamental; the pool releases its
        // own voices and resets the bends.
        let active = std::mem::take(&mut self.active);
        active.for_each(|ch, key, set| {
            if set.is_some() {
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

    #[test]
    fn the_natural_series_lands_on_the_hand_computed_keys() {
        // Stretch 1.0 on middle C: partial 2 is exactly +12 (0 cents);
        // partial 3 is 12 * log2(3) = 19.02, so +19 keys and +1.955 cents
        // (bend 3 at range 48); partial 4 is exactly +24; partial 5 is
        // 27.86, so +28 keys and -13.7 cents (bend -23).
        let mut fx = SpectralHalo::new(5, 1.0, 1.0, mpe(1, 8));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![
                on(60),
                bend(1, 0),
                von(1, 72, 100),
                bend(2, 3),
                von(2, 79, 100),
                bend(3, 0),
                von(3, 84, 100),
                bend(4, -23),
                von(4, 88, 100),
            ]
        );
        // The player's off releases the dry note and every partial.
        assert_eq!(
            run(&mut fx, off(60)),
            vec![off(60), voff(1, 72), voff(2, 79), voff(3, 84), voff(4, 88)]
        );
    }

    #[test]
    fn stretch_scales_the_offsets() {
        // Stretch 2.0: partial 2 sits at 24 semitones, still 0 cents.
        let mut fx = SpectralHalo::new(2, 1.0, 2.0, mpe(1, 8));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![on(60), bend(1, 0), von(1, 84, 100)]
        );
        // Stretch 0.5: partial 3 sits at 9.51 semitones, +10 keys and
        // -49.0 cents (bend -84 at range 48).
        let mut fx = SpectralHalo::new(3, 1.0, 0.5, mpe(1, 8));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![
                on(60),
                bend(1, 0),
                von(1, 66, 100),
                bend(2, -84),
                von(2, 70, 100),
            ]
        );
    }

    #[test]
    fn rolloff_decays_the_partial_velocities() {
        let mut fx = SpectralHalo::new(5, 0.5, 1.0, mpe(1, 8));
        let vels: Vec<u8> = run(&mut fx, on(60))
            .iter()
            .filter_map(|kind| match kind {
                EventKind::NoteOn { ch, vel, .. } if *ch != 0 => Some(*vel),
                _ => None,
            })
            .collect();
        // 100 * 0.5^(k-1) for k = 2..=5, rounded.
        assert_eq!(vels, vec![50, 25, 13, 6]);
        // Rolloff 0 floors every partial at velocity 1.
        let mut fx = SpectralHalo::new(2, 0.0, 1.0, mpe(1, 8));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![on(60), bend(1, 0), von(1, 72, 1)]
        );
    }

    #[test]
    fn partials_off_the_keyboard_are_skipped() {
        // From key 120, partials 2 (132) and 3 (139) both leave 0..=127.
        let mut fx = SpectralHalo::new(3, 1.0, 1.0, mpe(1, 8));
        assert_eq!(run(&mut fx, on(120)), vec![on(120)]);
        assert_eq!(run(&mut fx, off(120)), vec![off(120)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn a_retrigger_cuts_the_dry_note_and_the_partials() {
        let mut fx = SpectralHalo::new(2, 1.0, 1.0, mpe(1, 8));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![on(60), bend(1, 0), von(1, 72, 100)]
        );
        assert_eq!(
            run(&mut fx, on(60)),
            vec![off(60), voff(1, 72), on(60), bend(1, 0), von(1, 72, 100)]
        );
        assert_eq!(run(&mut fx, off(60)), vec![off(60), voff(1, 72)]);
    }

    #[test]
    fn an_orphan_note_off_passes_dry_alone() {
        let mut fx = SpectralHalo::new(3, 1.0, 1.0, mpe(1, 8));
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn flush_releases_the_fundamental_and_resets_the_bends() {
        let mut fx = SpectralHalo::new(2, 1.0, 1.0, mpe(1, 8));
        run(&mut fx, on(60));
        assert_eq!(flush(&mut fx), vec![off(60), voff(1, 72), bend(1, 0)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn the_worst_case_note_on_fans_out_fifteen_wide() {
        // 1 dry note plus 7 partials at 2 events each.
        let mut fx = SpectralHalo::new(8, 1.0, 1.0, mpe(1, 15));
        assert_eq!(run(&mut fx, on(30)).len(), 15);
        // A retrigger prepends the dry cut and 7 releases.
        assert_eq!(run(&mut fx, on(30)).len(), 8 + 15);
    }

    #[test]
    fn parameters_clamp() {
        // partials 0 clamps to 2, stretch 10 to 2.0.
        let mut fx = SpectralHalo::new(0, 1.0, 10.0, mpe(1, 8));
        assert_eq!(
            run(&mut fx, on(60)),
            vec![on(60), bend(1, 0), von(1, 84, 100)]
        );
        // partials 20 clamps to 8.
        let mut fx = SpectralHalo::new(20, 1.0, 1.0, mpe(1, 15));
        assert_eq!(run(&mut fx, on(30)).len(), 15);
    }

    #[test]
    fn player_bends_pass_and_touch_only_the_dry_channel() {
        let mut fx = SpectralHalo::new(3, 1.0, 1.0, mpe(1, 8));
        run(&mut fx, on(60));
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
