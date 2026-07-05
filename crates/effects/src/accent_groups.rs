//! Additive accent grouping: pulses counted, never clocked.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Group note-ons by count in the manner of Ligeti's Desordre: successive
/// note-ons cycle through the group lengths (`[3, 5]` makes groups of 3,
/// then 5, then 3 again), the first note of each group is accented to at
/// least `accent`, and the rest are held down to at most `rest`. No clock
/// is involved: only the order of note-ons matters, so the grouping
/// breathes with the player's tempo.
///
/// Formally, the first note of a group gets `max(input, accent)` and the
/// others `min(input, rest)`, so a genuinely loud downbeat survives and a
/// genuinely soft inner note stays soft. Keys are unchanged: note-offs
/// and everything else pass untouched, and they do not advance the count.
///
/// Fanout bound: exactly one output per input.
pub struct AccentGroups {
    /// The group lengths cycled through, fixed at construction.
    groups: Vec<u8>,
    /// Index of the current group.
    group: usize,
    /// Position within the current group; 0 is the accented note.
    pos: u8,
    accent: u8,
    rest: u8,
}

impl AccentGroups {
    /// Group entries are clamped to 1..=16 and an empty list falls back
    /// to `[3]`; `accent` and `rest` are clamped to 1..=127.
    pub fn new(groups: &[u8], accent: u8, rest: u8) -> Self {
        let groups: Vec<u8> = groups.iter().map(|&g| g.clamp(1, 16)).collect();
        let groups = if groups.is_empty() { vec![3] } else { groups };
        Self {
            groups,
            group: 0,
            pos: 0,
            accent: accent.clamp(1, 127),
            rest: rest.clamp(1, 127),
        }
    }

    /// The velocity for the next note-on, advancing the count.
    fn shape(&mut self, vel: u8) -> u8 {
        let shaped = if self.pos == 0 {
            vel.max(self.accent)
        } else {
            vel.min(self.rest)
        };
        self.pos += 1;
        if self.pos >= self.groups[self.group] {
            self.pos = 0;
            self.group = (self.group + 1) % self.groups.len();
        }
        shaped
    }
}

impl Effect for AccentGroups {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        let kind = match ev.kind {
            EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                ch,
                key,
                vel: self.shape(vel),
            },
            other => other,
        };
        push(out, cx, Event::new(ev.time, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{off, run};

    fn on_vel(fx: &mut AccentGroups, vel: u8) -> u8 {
        let out = run(
            fx,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel,
            },
        );
        match out[..] {
            [EventKind::NoteOn { vel, .. }] => vel,
            ref other => panic!("expected one note-on, got {other:?}"),
        }
    }

    #[test]
    fn groups_of_three_and_five_cycle_and_wrap() {
        let mut fx = AccentGroups::new(&[3, 5], 100, 40);
        // Two full cycles plus one note: accents land on positions 0, 3,
        // 8, 11, and 16 of the note-on sequence.
        let shaped: Vec<u8> = (0..17).map(|_| on_vel(&mut fx, 64)).collect();
        let expected = [
            100, 40, 40, // the group of 3
            100, 40, 40, 40, 40, // the group of 5
            100, 40, 40, // the cycle wraps
            100, 40, 40, 40, 40,  //
            100, // and wraps again
        ];
        assert_eq!(shaped, expected);
    }

    #[test]
    fn a_loud_downbeat_survives_the_accent_floor() {
        let mut fx = AccentGroups::new(&[3], 100, 40);
        assert_eq!(on_vel(&mut fx, 120), 120);
    }

    #[test]
    fn a_soft_inner_note_stays_soft() {
        let mut fx = AccentGroups::new(&[3], 100, 40);
        on_vel(&mut fx, 64);
        assert_eq!(on_vel(&mut fx, 20), 20);
    }

    #[test]
    fn note_offs_pass_and_do_not_advance_the_count() {
        let mut fx = AccentGroups::new(&[2], 100, 40);
        assert_eq!(on_vel(&mut fx, 64), 100);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        assert_eq!(on_vel(&mut fx, 64), 40);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        // Two offs later the count still says: next group starts here.
        assert_eq!(on_vel(&mut fx, 64), 100);
    }

    #[test]
    fn constructor_clamps_groups_and_velocities() {
        // Empty groups fall back to [3].
        let mut fx = AccentGroups::new(&[], 100, 40);
        let shaped: Vec<u8> = (0..6).map(|_| on_vel(&mut fx, 64)).collect();
        assert_eq!(shaped, vec![100, 40, 40, 100, 40, 40]);
        // A zero-length group is raised to 1: every note accented.
        let mut fx = AccentGroups::new(&[0], 100, 40);
        assert_eq!(on_vel(&mut fx, 64), 100);
        assert_eq!(on_vel(&mut fx, 64), 100);
        // A group past 16 clamps to 16.
        let mut fx = AccentGroups::new(&[200], 100, 40);
        let shaped: Vec<u8> = (0..17).map(|_| on_vel(&mut fx, 64)).collect();
        assert_eq!(shaped[0], 100);
        assert!(shaped[1..16].iter().all(|&v| v == 40));
        assert_eq!(shaped[16], 100);
        // accent and rest clamp into 1..=127; velocity 0 never emits.
        let mut fx = AccentGroups::new(&[2], 200, 0);
        assert_eq!(on_vel(&mut fx, 64), 127);
        assert_eq!(on_vel(&mut fx, 64), 1);
    }

    #[test]
    fn non_note_events_pass_unchanged() {
        let mut fx = AccentGroups::new(&[3], 100, 40);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
