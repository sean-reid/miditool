//! Part tintinnabuli: a T-voice shadows the melody on a fixed triad.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::push;

/// Which side of the melody the T-voice sounds on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TDirection {
    /// The T-voice sits above the played key.
    Superior,
    /// The T-voice sits below the played key.
    Inferior,
    /// The T-voice alternates sides per note-on, starting above.
    Alternating,
}

/// The component keys one input note produces: the played key (the
/// M-voice) first, then the T-voice when it fits the keyboard. `Default`
/// (all `None`) marks an inactive slot.
type Components = [Option<u8>; 2];

/// The tintinnabuli technique of Arvo Part: the played line is the M-voice
/// and every note-on also sounds a T-voice drawn from the tonic triad
/// (`root_pc` major or minor: pitch classes root, root+4 or root+3, and
/// root+7). The T-voice is the `position`-th triad tone strictly above
/// (`Superior`) or strictly below (`Inferior`) the played key; position 1
/// is the nearest triad tone, position 2 the second nearest.
/// `Alternating` flips the side per note-on, starting above. The T-voice
/// sounds at `round(vel * level)`, floored at 1; when the walk runs off
/// the keyboard only the dry note is emitted.
///
/// The emitted pair is remembered per input (channel, key), so the
/// note-off, a retrigger cut, and `flush` release exactly what is
/// sounding. The T-voice can be history-dependent (`Alternating`), so an
/// orphan note-off (or poly pressure with nothing sounding) is not
/// recomputed; it maps to the played key alone. Non-note events pass
/// unchanged.
///
/// Fanout bound: at most 2 retrigger cuts plus 2 note-ons per input
/// event, well under `MAX_FANOUT`.
pub struct Tintinnabuli {
    /// Bit `pc` set means the pitch class is a triad tone.
    triad: u16,
    /// 1 or 2: which triad tone past the played key the T-voice takes.
    position: u8,
    direction: TDirection,
    level: f32,
    /// Whether the next `Alternating` note-on sounds above.
    superior_next: bool,
    /// The component pair per active input (channel, key).
    active: PerNote<Components>,
}

impl Tintinnabuli {
    /// `root_pc` is masked to a pitch class, `position` clamped to 1..=2,
    /// and `level` clamped to 0.0..=1.0.
    pub fn new(root_pc: u8, minor: bool, position: u8, direction: TDirection, level: f32) -> Self {
        let root = root_pc % 12;
        let third = (root + if minor { 3 } else { 4 }) % 12;
        let fifth = (root + 7) % 12;
        Self {
            triad: 1 << root | 1 << third | 1 << fifth,
            position: position.clamp(1, 2),
            direction,
            level: level.clamp(0.0, 1.0),
            superior_next: true,
            active: PerNote::new(),
        }
    }

    fn in_triad(&self, key: u8) -> bool {
        self.triad >> (key % 12) & 1 == 1
    }

    /// The `position`-th triad tone strictly above or below the played
    /// key, or `None` when the walk leaves the keyboard first.
    fn t_voice(&self, key: u8, superior: bool) -> Option<u8> {
        let mut remaining = self.position;
        if superior {
            for k in key as u16 + 1..=127 {
                if self.in_triad(k as u8) {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(k as u8);
                    }
                }
            }
        } else {
            for k in (0..key).rev() {
                if self.in_triad(k) {
                    remaining -= 1;
                    if remaining == 0 {
                        return Some(k);
                    }
                }
            }
        }
        None
    }

    /// The side the next note-on's T-voice takes, flipping the
    /// `Alternating` state as a side effect.
    fn next_superior(&mut self) -> bool {
        match self.direction {
            TDirection::Superior => true,
            TDirection::Inferior => false,
            TDirection::Alternating => {
                let superior = self.superior_next;
                self.superior_next = !superior;
                superior
            }
        }
    }

    /// T-voice velocity: the M-voice scaled by `level`, floored at 1.
    fn t_vel(&self, vel: u8) -> u8 {
        (vel as f32 * self.level).round().clamp(1.0, 127.0) as u8
    }
}

impl Effect for Tintinnabuli {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut whatever the previous strike left
                // sounding.
                for prev in self.active.take(ch, key).into_iter().flatten() {
                    let cut = EventKind::NoteOff {
                        ch,
                        key: prev,
                        vel: 0,
                    };
                    push(out, cx, Event::new(ev.time, cut));
                }
                let superior = self.next_superior();
                let t = self.t_voice(key, superior);
                let set: Components = [Some(key), t];
                push(
                    out,
                    cx,
                    Event::new(ev.time, EventKind::NoteOn { ch, key, vel }),
                );
                if let Some(t_key) = t {
                    let kind = EventKind::NoteOn {
                        ch,
                        key: t_key,
                        vel: self.t_vel(vel),
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
                self.active.set(ch, key, set);
            }
            EventKind::NoteOff { ch, key, vel } => {
                // Orphan note-offs release only the played key: the
                // T-voice's side may be history-dependent (Alternating),
                // so it cannot be reconstructed statelessly.
                let mut set = self.active.take(ch, key);
                if set.iter().all(Option::is_none) {
                    set = [Some(key), None];
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
                    set = [Some(key), None];
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

    #[test]
    fn superior_takes_the_nearest_triad_tone_above() {
        // C major triad {0, 4, 7}. Above C4 (60) the first tone is E (64).
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 0.5);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 50)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(64)]);
    }

    #[test]
    fn superior_second_position_skips_one_tone() {
        // Above C4: E (64) first, G (67) second.
        let mut fx = Tintinnabuli::new(0, false, 2, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(67, 100)]);
    }

    #[test]
    fn inferior_walks_down_across_the_triad_boundary() {
        // Below C4 (60): G (55) first, then E (52) across the octave.
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Inferior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(55, 100)]);
        let mut fx = Tintinnabuli::new(0, false, 2, TDirection::Inferior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(52, 100)]);
    }

    #[test]
    fn a_played_triad_tone_still_gets_a_strictly_offset_t_voice() {
        // F (65) is not in the triad: above it G (67), below it E (64).
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(65)), vec![von(65, 100), von(67, 100)]);
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Inferior, 1.0);
        assert_eq!(run(&mut fx, on(65)), vec![von(65, 100), von(64, 100)]);
        // E (64) is in the triad: "strictly" means the T-voice moves off
        // it, up to G (67) or down to C (60).
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(64)), vec![von(64, 100), von(67, 100)]);
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Inferior, 1.0);
        assert_eq!(run(&mut fx, on(64)), vec![von(64, 100), von(60, 100)]);
    }

    #[test]
    fn minor_triad_uses_the_flat_third() {
        // A minor triad {9, 0, 4}. Above C4 (60): E (64). Below: A (57).
        let mut fx = Tintinnabuli::new(9, true, 1, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 100)]);
        let mut fx = Tintinnabuli::new(9, true, 1, TDirection::Inferior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(57, 100)]);
    }

    #[test]
    fn alternating_flips_per_note_starting_superior() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Alternating, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 100)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(64)]);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(55, 100)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(55)]);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 100)]);
    }

    #[test]
    fn a_t_voice_off_the_keyboard_leaves_the_dry_note_alone() {
        // Nothing sits strictly above 127.
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(127)), vec![von(127, 100)]);
        assert_eq!(run(&mut fx, off(127)), vec![off(127)]);
        // Nothing sits strictly below 0.
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Inferior, 1.0);
        assert_eq!(run(&mut fx, on(0)), vec![von(0, 100)]);
        assert_eq!(run(&mut fx, off(0)), vec![off(0)]);
    }

    #[test]
    fn t_voice_velocity_floors_at_one() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 0.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 1)]);
    }

    #[test]
    fn position_and_level_clamp() {
        // Position 0 clamps to 1, position 9 to 2; level past 1 to 1.
        let mut fx = Tintinnabuli::new(0, false, 0, TDirection::Superior, 2.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 100)]);
        let mut fx = Tintinnabuli::new(0, false, 9, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(67, 100)]);
    }

    #[test]
    fn root_pc_is_masked() {
        let mut a = Tintinnabuli::new(12, false, 1, TDirection::Superior, 1.0);
        let mut b = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        assert_eq!(run(&mut a, on(60)), run(&mut b, on(60)));
    }

    #[test]
    fn retrigger_cuts_both_voices() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(64, 100)]);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![off(60), off(64), von(60, 100), von(64, 100)]
        );
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(64)]);
    }

    #[test]
    fn orphan_note_off_releases_the_played_key_only() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Alternating, 1.0);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn poly_pressure_follows_both_voices() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
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
        assert_eq!(run(&mut fx, pressure), vec![at(60), at(64)]);
    }

    #[test]
    fn flush_releases_both_voices() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        run(&mut fx, on(60));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off(60), off(64)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = Tintinnabuli::new(0, false, 1, TDirection::Superior, 1.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
