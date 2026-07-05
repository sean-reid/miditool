//! Cowell clusters: one key becomes a fistful of neighbors.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx, Sieve};

use crate::router::push;

/// The member keys one input note produces, bottom to top. `Default`
/// (all `None`) marks an inactive slot.
type Members = [Option<u8>; 12];

/// The key set cluster members are drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterKind {
    /// Every key.
    Chromatic,
    /// The piano's white keys.
    White,
    /// The piano's black keys.
    Black,
    /// The members of a sieve.
    Sieve(Sieve),
}

impl ClusterKind {
    fn contains(&self, key: u8) -> bool {
        match self {
            ClusterKind::Chromatic => true,
            ClusterKind::White => matches!(key % 12, 0 | 2 | 4 | 5 | 7 | 9 | 11),
            ClusterKind::Black => matches!(key % 12, 1 | 3 | 6 | 8 | 10),
            ClusterKind::Sieve(sieve) => sieve.contains(key),
        }
    }
}

/// Where the played key sits in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterAnchor {
    Bottom,
    Center,
    Top,
}

/// Turn every note-on into a tone cluster in the manner of Cowell's fist
/// and forearm pieces: `width` member keys drawn from the kind's key set
/// (chromatic, the white keys, the black keys, or a sieve's members),
/// positioned so the played key is the bottom, center, or top member. The
/// played key is always included, even when the kind's set does not
/// contain it: a black-key cluster anchored on a white key keeps its
/// white anchor. A center anchor puts `(width - 1) / 2` members below and
/// the rest above; a side that runs off the keyboard (or past the set's
/// last member) truncates, shrinking the cluster.
///
/// The played key keeps the input velocity; every other member is scaled
/// by `rolloff^rank`, where rank orders the members by distance from the
/// played key (1 for the nearest, ties breaking toward the lower key),
/// rounded and clamped to 1..=127. The emitted member set is remembered
/// per input (channel, key), so the note-off, a retrigger cut, and
/// `flush` release exactly the sounding members. The mapping is
/// deterministic, so an orphan note-off (or poly pressure with nothing
/// sounding) is mapped statelessly instead of dropped, like `RingMod`.
/// Non-note events pass unchanged.
///
/// Fanout bound: at most 12 retrigger cuts plus 12 note-ons per input
/// event, 24 total, well under `MAX_FANOUT`.
pub struct ClusterFist {
    kind: ClusterKind,
    width: u8,
    anchor: ClusterAnchor,
    rolloff: f32,
    /// The member set per active input (channel, key).
    active: PerNote<Members>,
}

impl ClusterFist {
    /// `width` is clamped to 2..=12 and `rolloff` to 0.0..=1.0.
    pub fn new(kind: ClusterKind, width: u8, anchor: ClusterAnchor, rolloff: f32) -> Self {
        Self {
            kind,
            width: width.clamp(2, 12),
            anchor,
            rolloff: rolloff.clamp(0.0, 1.0),
            active: PerNote::new(),
        }
    }

    /// The nearest set member strictly above `from`, if any.
    fn above(&self, from: u8) -> Option<u8> {
        (from as u16 + 1..=127)
            .map(|k| k as u8)
            .find(|&k| self.kind.contains(k))
    }

    /// The nearest set member strictly below `from`, if any.
    fn below(&self, from: u8) -> Option<u8> {
        (0..from).rev().find(|&k| self.kind.contains(k))
    }

    /// The cluster for a played key, bottom to top, played key included.
    fn members(&self, key: u8) -> Members {
        let spread = self.width - 1;
        let (below, above) = match self.anchor {
            ClusterAnchor::Bottom => (0, spread),
            ClusterAnchor::Center => (spread / 2, spread - spread / 2),
            ClusterAnchor::Top => (spread, 0),
        };
        let mut set: Members = [None; 12];
        // Walk down collecting nearest-first, then reverse into place so
        // the set reads bottom to top.
        let mut lows = [0u8; 11];
        let mut n = 0;
        let mut cur = key;
        for _ in 0..below {
            let Some(k) = self.below(cur) else { break };
            lows[n] = k;
            n += 1;
            cur = k;
        }
        let mut idx = 0;
        for &k in lows[..n].iter().rev() {
            set[idx] = Some(k);
            idx += 1;
        }
        set[idx] = Some(key);
        idx += 1;
        let mut cur = key;
        for _ in 0..above {
            let Some(k) = self.above(cur) else { break };
            set[idx] = Some(k);
            idx += 1;
            cur = k;
        }
        set
    }

    /// Velocity for one member: the played key keeps `vel`, the others
    /// decay by distance rank.
    fn member_vel(&self, set: &Members, member: u8, key: u8, vel: u8) -> u8 {
        if member == key {
            return vel;
        }
        let dist = key.abs_diff(member);
        let rank = 1 + set
            .iter()
            .flatten()
            .filter(|&&m| m != key && m != member)
            .filter(|&&m| {
                let d = key.abs_diff(m);
                d < dist || (d == dist && m < member)
            })
            .count();
        let scaled = vel as f32 * self.rolloff.powi(rank as i32);
        // Velocity 0 would read as a note-off on the wire; never emit it.
        scaled.round().clamp(1.0, 127.0) as u8
    }

    /// The remembered set for an input note, or the statelessly recomputed
    /// one when nothing is remembered (the orphan-note-off fallback; the
    /// mapping is deterministic, so both agree whenever a note-on was
    /// seen).
    fn recall(set: Members, fallback: impl FnOnce() -> Members) -> Members {
        if set.iter().all(Option::is_none) {
            fallback()
        } else {
            set
        }
    }
}

impl Effect for ClusterFist {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut every member the previous strike left
                // sounding.
                for prev in self.active.take(ch, key).into_iter().flatten() {
                    let cut = EventKind::NoteOff {
                        ch,
                        key: prev,
                        vel: 0,
                    };
                    push(out, cx, Event::new(ev.time, cut));
                }
                let set = self.members(key);
                for member in set.into_iter().flatten() {
                    let kind = EventKind::NoteOn {
                        ch,
                        key: member,
                        vel: self.member_vel(&set, member, key, vel),
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
                self.active.set(ch, key, set);
            }
            EventKind::NoteOff { ch, key, vel } => {
                let set = Self::recall(self.active.take(ch, key), || self.members(key));
                for member in set.into_iter().flatten() {
                    let kind = EventKind::NoteOff {
                        ch,
                        key: member,
                        vel,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
            }
            EventKind::PolyPressure { ch, key, value } => {
                let set = Self::recall(self.active.get(ch, key), || self.members(key));
                for member in set.into_iter().flatten() {
                    let kind = EventKind::PolyPressure {
                        ch,
                        key: member,
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
            for member in set.into_iter().flatten() {
                let kind = EventKind::NoteOff {
                    ch,
                    key: member,
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
    fn chromatic_bottom_stacks_upward_with_rolloff() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 4, ClusterAnchor::Bottom, 0.5);
        // 100 * 0.5 = 50, * 0.25 = 25, * 0.125 = 12.5 rounds to 13.
        assert_eq!(
            run(&mut fx, on(60)),
            vec![von(60, 100), von(61, 50), von(62, 25), von(63, 13)]
        );
        assert_eq!(
            run(&mut fx, off(60)),
            vec![off(60), off(61), off(62), off(63)]
        );
    }

    #[test]
    fn white_center_takes_the_scale_neighbors() {
        let mut fx = ClusterFist::new(ClusterKind::White, 5, ClusterAnchor::Center, 1.0);
        // Around E: two white keys below (D, C), two above (F, G).
        assert_eq!(
            run(&mut fx, on(64)),
            vec![
                von(60, 100),
                von(62, 100),
                von(64, 100),
                von(65, 100),
                von(67, 100)
            ]
        );
    }

    #[test]
    fn black_top_reaches_down_the_black_keys() {
        let mut fx = ClusterFist::new(ClusterKind::Black, 3, ClusterAnchor::Top, 1.0);
        // Below F#: D#, then C#.
        assert_eq!(
            run(&mut fx, on(66)),
            vec![von(61, 100), von(63, 100), von(66, 100)]
        );
    }

    #[test]
    fn an_anchor_outside_the_set_is_kept() {
        // A black-key cluster anchored on C keeps the white anchor.
        let mut fx = ClusterFist::new(ClusterKind::Black, 3, ClusterAnchor::Bottom, 1.0);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![von(60, 100), von(61, 100), von(63, 100)]
        );
        // A white-key cluster anchored on C# keeps the black anchor.
        let mut fx = ClusterFist::new(ClusterKind::White, 3, ClusterAnchor::Bottom, 1.0);
        assert_eq!(
            run(&mut fx, on(61)),
            vec![von(61, 100), von(62, 100), von(64, 100)]
        );
    }

    #[test]
    fn sieve_members_form_the_cluster() {
        let sieve = Sieve::parse("12@0").unwrap();
        let mut fx = ClusterFist::new(ClusterKind::Sieve(sieve), 3, ClusterAnchor::Center, 0.5);
        // Octaves around middle C; 48 and 72 are equidistant, so 48 takes
        // rank 1 (ties break toward the lower key) and 72 rank 2.
        assert_eq!(
            run(&mut fx, on(60)),
            vec![von(48, 50), von(60, 100), von(72, 25)]
        );
    }

    #[test]
    fn center_splits_below_first_then_above() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 4, ClusterAnchor::Center, 0.5);
        // Spread 3: one below, two above. Ranks: 59 and 61 tie at distance
        // 1 and the lower key wins, 62 is rank 3.
        assert_eq!(
            run(&mut fx, on(60)),
            vec![von(59, 50), von(60, 100), von(61, 25), von(62, 13)]
        );
    }

    #[test]
    fn top_anchor_reaches_down() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 3, ClusterAnchor::Top, 1.0);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![von(58, 100), von(59, 100), von(60, 100)]
        );
    }

    #[test]
    fn the_keyboard_edge_truncates_the_cluster() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 5, ClusterAnchor::Bottom, 1.0);
        assert_eq!(
            run(&mut fx, on(125)),
            vec![von(125, 100), von(126, 100), von(127, 100)]
        );
        assert_eq!(run(&mut fx, off(125)), vec![off(125), off(126), off(127)]);
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 5, ClusterAnchor::Top, 1.0);
        assert_eq!(run(&mut fx, on(1)), vec![von(0, 100), von(1, 100)]);
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 4, ClusterAnchor::Center, 1.0);
        assert_eq!(
            run(&mut fx, on(0)),
            vec![von(0, 100), von(1, 100), von(2, 100)]
        );
    }

    #[test]
    fn zero_rolloff_floors_member_velocity_at_one() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 3, ClusterAnchor::Bottom, 0.0);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![von(60, 100), von(61, 1), von(62, 1)]
        );
    }

    #[test]
    fn width_clamps() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 0, ClusterAnchor::Bottom, 1.0);
        assert_eq!(run(&mut fx, on(60)).len(), 2);
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, u8::MAX, ClusterAnchor::Bottom, 1.0);
        assert_eq!(run(&mut fx, on(60)).len(), 12);
    }

    #[test]
    fn retrigger_cuts_every_member() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 2, ClusterAnchor::Bottom, 1.0);
        assert_eq!(run(&mut fx, on(60)), vec![von(60, 100), von(61, 100)]);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![off(60), off(61), von(60, 100), von(61, 100)]
        );
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(61)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 3, ClusterAnchor::Bottom, 1.0);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(61), off(62)]);
    }

    #[test]
    fn poly_pressure_follows_every_member() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 2, ClusterAnchor::Bottom, 1.0);
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
        assert_eq!(run(&mut fx, pressure), vec![at(60), at(61)]);
    }

    #[test]
    fn flush_releases_every_member() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 2, ClusterAnchor::Bottom, 1.0);
        run(&mut fx, on(60));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off(60), off(61)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = ClusterFist::new(ClusterKind::Chromatic, 12, ClusterAnchor::Center, 0.5);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
