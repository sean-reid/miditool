//! Negative harmony: melody reflected around the tonic-dominant axis.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::{NoteRouter, push};

/// The component keys one input note produces in `add` mode: the played
/// key first, then its mirror. `Default` (all `None`) marks an inactive
/// slot.
type Components = [Option<u8>; 2];

/// Reflect every note around the key's tonic-dominant axis, the negative
/// harmony of Levy's Theory of Harmony: pitch class `pc` maps to
/// `(7 + 2 * tonic_pc - pc) mod 12`, the reflection whose axis lies
/// midway between the tonic and its dominant. Under this convention the
/// tonic maps to the dominant (in C: C to G, D to F, E to Eb, B to Ab)
/// and major triads become minor ones. The output key is the candidate
/// with the mirrored pitch class nearest to the input key, ties breaking
/// downward (unreachable in practice: the reflection always moves the
/// pitch class an odd number of semitones, never exactly six).
///
/// With `add` false the mirror replaces the note through the router,
/// which keeps note-offs, retriggers, and poly pressure consistent and
/// maps orphan note-offs statelessly. With `add` true the dry note is
/// emitted plus the mirror at `round(vel * level)` floored at 1, both
/// remembered per input (channel, key) so the note-off, a retrigger cut,
/// and `flush` release exactly the pair; the mapping is deterministic, so
/// an orphan note-off is recomputed statelessly. Non-note events pass
/// unchanged.
///
/// Fanout bound: at most 2 retrigger cuts plus 2 note-ons per input
/// event, well under `MAX_FANOUT`.
pub struct NegativeHarmony {
    tonic_pc: u8,
    add: bool,
    level: f32,
    /// Routes the replacement when `add` is false.
    router: NoteRouter,
    /// The component pair per active input (channel, key) when `add`.
    active: PerNote<Components>,
}

impl NegativeHarmony {
    /// `tonic_pc` is masked to a pitch class and `level` clamped to
    /// 0.0..=1.0.
    pub fn new(tonic_pc: u8, add: bool, level: f32) -> Self {
        Self {
            tonic_pc: tonic_pc % 12,
            add,
            level: level.clamp(0.0, 1.0),
            router: NoteRouter::new(),
            active: PerNote::new(),
        }
    }

    /// The mirror of a key: the reflected pitch class voiced at the
    /// candidate nearest to the input, ties down.
    fn mirror(&self, key: u8) -> u8 {
        let pc = (7 + 2 * self.tonic_pc as i16 - (key % 12) as i16).rem_euclid(12) as u8;
        for d in 0..12u8 {
            if key >= d && (key - d) % 12 == pc {
                return key - d;
            }
            let above = key as u16 + d as u16;
            if above <= 127 && (above % 12) as u8 == pc {
                return above as u8;
            }
        }
        // Every pitch class occurs within eleven keys of any key on the
        // 128-key board, so the loop always returns.
        key
    }

    /// Mirror velocity in `add` mode: the dry velocity scaled by `level`,
    /// floored at 1.
    fn mirror_vel(&self, vel: u8) -> u8 {
        (vel as f32 * self.level).round().clamp(1.0, 127.0) as u8
    }

    /// The pair in `add` mode; the reflection always changes the pitch
    /// class, so the two keys never collide.
    fn components(&self, key: u8) -> Components {
        [Some(key), Some(self.mirror(key))]
    }
}

impl Effect for NegativeHarmony {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        if !self.add {
            match ev.kind {
                EventKind::NoteOn { key, .. } => {
                    self.router.note_on(ev, Some(self.mirror(key)), out, cx);
                }
                EventKind::NoteOff { key, .. } => {
                    self.router.note_off(ev, Some(self.mirror(key)), out, cx);
                }
                EventKind::PolyPressure { key, .. } => {
                    self.router
                        .poly_pressure(ev, Some(self.mirror(key)), out, cx);
                }
                _ => push(out, cx, *ev),
            }
            return;
        }
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut both keys the previous strike left
                // sounding.
                for prev in self.active.take(ch, key).into_iter().flatten() {
                    let cut = EventKind::NoteOff {
                        ch,
                        key: prev,
                        vel: 0,
                    };
                    push(out, cx, Event::new(ev.time, cut));
                }
                let set = self.components(key);
                push(
                    out,
                    cx,
                    Event::new(ev.time, EventKind::NoteOn { ch, key, vel }),
                );
                let kind = EventKind::NoteOn {
                    ch,
                    key: self.mirror(key),
                    vel: self.mirror_vel(vel),
                };
                push(out, cx, Event::new(ev.time, kind));
                self.active.set(ch, key, set);
            }
            EventKind::NoteOff { ch, key, vel } => {
                let mut set = self.active.take(ch, key);
                if set.iter().all(Option::is_none) {
                    // Orphan: the mapping is deterministic, recompute it.
                    set = self.components(key);
                }
                for key_out in set.into_iter().flatten() {
                    let kind = EventKind::NoteOff {
                        ch,
                        key: key_out,
                        vel,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
            }
            EventKind::PolyPressure { ch, key, value } => {
                let mut set = self.active.get(ch, key);
                if set.iter().all(Option::is_none) {
                    set = self.components(key);
                }
                for key_out in set.into_iter().flatten() {
                    let kind = EventKind::PolyPressure {
                        ch,
                        key: key_out,
                        value,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.router.flush(out, cx);
        let active = std::mem::take(&mut self.active);
        active.for_each(|ch, _key, set| {
            for key_out in set.into_iter().flatten() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flush, off, on, run};

    fn von(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    /// The full pitch-class table for tonic C, hand-derived from the axis
    /// between Eb and E: 0<->7 (C<->G), 1<->6 (Db<->F#), 2<->5 (D<->F),
    /// 3<->4 (Eb<->E), 8<->11 (Ab<->B), 9<->10 (A<->Bb). The voiced
    /// output is the candidate nearest each input key.
    #[test]
    fn the_c_major_table_matches_hand_derived_values() {
        // (input key, mirrored key nearest to it).
        let table = [
            (60, 55), // C -> G below (5 vs 7 semitones away)
            (61, 66), // Db -> F# above (5 vs 7)
            (62, 65), // D -> F above (3 vs 9)
            (63, 64), // Eb -> E above (1 vs 11)
            (64, 63), // E -> Eb below (1)
            (65, 62), // F -> D below (3)
            (66, 61), // F# -> Db below (5)
            (67, 72), // G -> C above (5 vs 7)
            (68, 71), // Ab -> B above (3)
            (69, 70), // A -> Bb above (1)
            (70, 69), // Bb -> A below (1)
            (71, 68), // B -> Ab below (3)
        ];
        for (input, expected) in table {
            let mut fx = NegativeHarmony::new(0, false, 1.0);
            assert_eq!(run(&mut fx, on(input)), vec![on(expected)], "key {input}");
            assert_eq!(run(&mut fx, off(input)), vec![off(expected)]);
        }
    }

    #[test]
    fn the_reflection_is_an_involution_on_pitch_classes() {
        for tonic in 0..12u8 {
            let fx = NegativeHarmony::new(tonic, false, 1.0);
            for key in 50..70u8 {
                let mirrored = fx.mirror(key);
                assert_eq!(
                    fx.mirror(mirrored) % 12,
                    key % 12,
                    "tonic {tonic} key {key}"
                );
            }
        }
    }

    #[test]
    fn the_tonic_shifts_the_axis() {
        // Tonic G (7): out_pc = (21 - pc) mod 12 = (9 - pc) mod 12, so
        // G (7) maps to D (2) and B (11) to Bb (10).
        let mut fx = NegativeHarmony::new(7, false, 1.0);
        assert_eq!(run(&mut fx, on(67)), vec![on(62)]);
        run(&mut fx, off(67));
        assert_eq!(run(&mut fx, on(71)), vec![on(70)]);
    }

    #[test]
    fn a_major_triad_mirrors_to_a_minor_one() {
        // C major (60, 64, 67) reflects to keys spelling a C minor
        // sonority: G below, Eb, C above.
        let mut fx = NegativeHarmony::new(0, false, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![on(55)]);
        assert_eq!(run(&mut fx, on(64)), vec![on(63)]);
        assert_eq!(run(&mut fx, on(67)), vec![on(72)]);
    }

    #[test]
    fn add_emits_dry_plus_scaled_mirror() {
        let mut fx = NegativeHarmony::new(0, true, 0.5);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(55, 50)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(55)]);
    }

    #[test]
    fn mirror_velocity_floors_at_one() {
        let mut fx = NegativeHarmony::new(0, true, 0.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(55, 1)]);
    }

    #[test]
    fn replace_retrigger_cuts_the_mirror() {
        let mut fx = NegativeHarmony::new(0, false, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![on(55)]);
        assert_eq!(run(&mut fx, on(60)), vec![off(55), on(55)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(55)]);
    }

    #[test]
    fn add_retrigger_cuts_both_keys() {
        let mut fx = NegativeHarmony::new(0, true, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(55, 100)]);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![off(60), off(55), von(60, 100), von(55, 100)]
        );
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(55)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly_in_both_modes() {
        let mut fx = NegativeHarmony::new(0, false, 1.0);
        assert_eq!(run(&mut fx, off(60)), vec![off(55)]);
        let mut fx = NegativeHarmony::new(0, true, 1.0);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(55)]);
    }

    #[test]
    fn poly_pressure_follows_the_sounding_keys() {
        let mut fx = NegativeHarmony::new(0, true, 1.0);
        run(&mut fx, on(60));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        let at = |key| EventKind::PolyPressure {
            ch: 0,
            key,
            value: 33,
        };
        assert_eq!(run(&mut fx, pressure), vec![at(60), at(55)]);
    }

    #[test]
    fn flush_releases_everything_in_both_modes() {
        let mut fx = NegativeHarmony::new(0, false, 1.0);
        run(&mut fx, on(60));
        assert_eq!(flush(&mut fx), vec![off(55)]);
        assert_eq!(flush(&mut fx), vec![]);
        let mut fx = NegativeHarmony::new(0, true, 1.0);
        run(&mut fx, on(60));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off(55), off(60)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = NegativeHarmony::new(0, true, 1.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
