//! Neo-Riemannian triad walking on the Tonnetz.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::push;

/// The three neo-Riemannian transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plr {
    /// Parallel: same root, opposite quality (C major <-> C minor).
    P,
    /// Leittonwechsel: major(r) -> minor(r + 4), minor(r) -> major(r - 4)
    /// (C major <-> E minor).
    L,
    /// Relative: major(r) -> minor(r + 9), minor(r) -> major(r + 3)
    /// (C major <-> A minor).
    R,
}

/// The component keys one input note produces: the triad voiced root,
/// third, fifth, plus the played key when included, deduplicated.
/// `Default` (all `None`) marks an inactive slot.
type Components = [Option<u8>; 4];

/// Walk the Tonnetz one step per note-on, in the manner of the
/// neo-Riemannian analyses of late Romantic harmony (and the hexatonic
/// cycles Cohn hears in Wagner): the state is a triad (root pitch class
/// and quality), and each note-on first applies the next transform in the
/// cycling `sequence` (P, L, or R; an empty sequence falls back to
/// `[R, L]`, the diatonic circle), then emits the new triad. Each of the
/// triad's three pitch classes is voiced at the key inside `lo..=hi`
/// closest to the played key (ties breaking downward; a pitch class with
/// no key in the range is skipped), every triad note at the input
/// velocity, plus the played key itself when `include_played`, emitted
/// once even when it doubles a triad note.
///
/// The emitted set (up to 4 keys) is remembered per input (channel, key),
/// so the note-off, a retrigger cut, and `flush` release exactly what is
/// sounding. The triad state advances on every note-on, so an orphan
/// note-off (or poly pressure with nothing sounding) cannot be
/// reconstructed and is dropped, like `Klangfarben`. Non-note events pass
/// unchanged. The walk is deterministic: the same input sequence always
/// produces the same output.
///
/// Fanout bound: at most 4 retrigger cuts plus 4 note-ons per input
/// event, well under `MAX_FANOUT`.
pub struct Tonnetz {
    /// The cycling transform sequence, fixed at construction, never
    /// empty.
    sequence: Vec<Plr>,
    /// Next position in `sequence`.
    next: usize,
    /// Current triad root pitch class.
    root: u8,
    /// Current triad quality.
    minor: bool,
    lo: u8,
    hi: u8,
    include_played: bool,
    /// The component set per active input (channel, key).
    active: PerNote<Components>,
}

/// Insert a component, skipping `None` and keys already present.
fn add(set: &mut Components, key: Option<u8>) {
    let Some(key) = key else {
        return;
    };
    for slot in set.iter_mut() {
        match slot {
            Some(k) if *k == key => return,
            None => {
                *slot = Some(key);
                return;
            }
            _ => {}
        }
    }
}

impl Tonnetz {
    /// `root_pc` is masked to a pitch class; `lo` and `hi` are clamped to
    /// 127 and swapped if reversed; an empty `sequence` falls back to
    /// `[R, L]`.
    pub fn new(
        root_pc: u8,
        minor: bool,
        sequence: &[Plr],
        lo: u8,
        hi: u8,
        include_played: bool,
    ) -> Self {
        let sequence = if sequence.is_empty() {
            vec![Plr::R, Plr::L]
        } else {
            sequence.to_vec()
        };
        let (lo, hi) = (lo.min(127), hi.min(127));
        Self {
            sequence,
            next: 0,
            root: root_pc % 12,
            minor,
            lo: lo.min(hi),
            hi: lo.max(hi),
            include_played,
            active: PerNote::new(),
        }
    }

    /// Apply the next transform in the cycling sequence to the triad
    /// state.
    fn advance(&mut self) {
        let plr = self.sequence[self.next];
        self.next = (self.next + 1) % self.sequence.len();
        match plr {
            Plr::P => self.minor = !self.minor,
            Plr::L => {
                self.root = (self.root + if self.minor { 8 } else { 4 }) % 12;
                self.minor = !self.minor;
            }
            Plr::R => {
                self.root = (self.root + if self.minor { 3 } else { 9 }) % 12;
                self.minor = !self.minor;
            }
        }
    }

    /// The current triad's pitch classes: root, third, fifth.
    fn triad_pcs(&self) -> [u8; 3] {
        let third = if self.minor { 3 } else { 4 };
        [self.root, (self.root + third) % 12, (self.root + 7) % 12]
    }

    /// The key in `lo..=hi` with pitch class `pc` closest to `played`,
    /// ties breaking downward, or `None` when the range holds none.
    fn voice(&self, pc: u8, played: u8) -> Option<u8> {
        for d in 0..128u8 {
            if let Some(below) = played.checked_sub(d)
                && (self.lo..=self.hi).contains(&below)
                && below % 12 == pc
            {
                return Some(below);
            }
            let above = played as u16 + d as u16;
            if above <= self.hi as u16 && above >= self.lo as u16 && above % 12 == pc as u16 {
                return Some(above as u8);
            }
        }
        None
    }

    /// The voiced triad plus the played key when included, deduplicated.
    fn components(&self, played: u8) -> Components {
        let mut set: Components = [None; 4];
        for pc in self.triad_pcs() {
            add(&mut set, self.voice(pc, played));
        }
        if self.include_played {
            add(&mut set, Some(played));
        }
        set
    }
}

impl Effect for Tonnetz {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut every key the previous strike left
                // sounding.
                for prev in self.active.take(ch, key).into_iter().flatten() {
                    let cut = EventKind::NoteOff {
                        ch,
                        key: prev,
                        vel: 0,
                    };
                    push(out, cx, Event::new(ev.time, cut));
                }
                self.advance();
                let set = self.components(key);
                for key_out in set.into_iter().flatten() {
                    let kind = EventKind::NoteOn {
                        ch,
                        key: key_out,
                        vel,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
                self.active.set(ch, key, set);
            }
            EventKind::NoteOff { ch, key, vel } => {
                // Orphan note-offs are dropped: the transform that would
                // have placed them never happened.
                for key_out in self.active.take(ch, key).into_iter().flatten() {
                    let kind = EventKind::NoteOff {
                        ch,
                        key: key_out,
                        vel,
                    };
                    push(out, cx, Event::new(ev.time, kind));
                }
            }
            EventKind::PolyPressure { ch, key, value } => {
                for key_out in self.active.get(ch, key).into_iter().flatten() {
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

    /// Emitted pitch classes of one note-on, sorted.
    fn pcs(fx: &mut Tonnetz, key: u8) -> Vec<u8> {
        let out = run(fx, on(key));
        let mut pcs: Vec<u8> = out
            .iter()
            .filter_map(|kind| match kind {
                EventKind::NoteOn { key, .. } => Some(key % 12),
                _ => None,
            })
            .collect();
        run(fx, off(key));
        pcs.sort_unstable();
        pcs
    }

    #[test]
    fn plr_transform_table_from_c_major() {
        // P: C major -> C minor {0, 3, 7}.
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![0, 3, 7]);
        // L: C major -> E minor {4, 7, 11}.
        let mut fx = Tonnetz::new(0, false, &[Plr::L], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![4, 7, 11]);
        // R: C major -> A minor {9, 0, 4}.
        let mut fx = Tonnetz::new(0, false, &[Plr::R], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![0, 4, 9]);
    }

    #[test]
    fn plr_transform_table_from_a_minor() {
        // P: A minor -> A major {9, 1, 4}.
        let mut fx = Tonnetz::new(9, true, &[Plr::P], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![1, 4, 9]);
        // L: A minor -> F major {5, 9, 0}.
        let mut fx = Tonnetz::new(9, true, &[Plr::L], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![0, 5, 9]);
        // R: A minor -> C major {0, 4, 7}.
        let mut fx = Tonnetz::new(9, true, &[Plr::R], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![0, 4, 7]);
    }

    #[test]
    fn pl_cycles_through_the_hexatonic_system_in_six_steps() {
        // From C major: Cm, Ab, Abm, E, Em, C, then Cm again.
        let mut fx = Tonnetz::new(0, false, &[Plr::P, Plr::L], 0, 127, false);
        let expected = [
            vec![0, 3, 7],  // C minor
            vec![0, 3, 8],  // Ab major
            vec![3, 8, 11], // Ab minor
            vec![4, 8, 11], // E major
            vec![4, 7, 11], // E minor
            vec![0, 4, 7],  // C major
            vec![0, 3, 7],  // C minor: the cycle closed after 6 steps
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(pcs(&mut fx, 60), *want, "step {}", i + 1);
        }
    }

    #[test]
    fn voicing_picks_the_keys_nearest_the_played_key() {
        // P from C major gives C minor {0, 3, 7}. Near 60: the root at
        // 60, the third at 63, and the fifth at 55 (5 below beats 67,
        // which is 7 above).
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, false);
        assert_eq!(run(&mut fx, on(60)), vec![on(60), on(63), on(55)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(63), off(55)]);
    }

    #[test]
    fn voicing_ties_break_downward() {
        // P from C major gives C minor; played 61 puts pc 7 at 55 and 67,
        // both 6 away: the tie breaks down to 55.
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, false);
        assert_eq!(run(&mut fx, on(61)), vec![on(60), on(63), on(55)]);
    }

    #[test]
    fn voicing_respects_the_range() {
        // One octave 60..=71 voices each pitch class at exactly 60 + pc.
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 60, 71, false);
        assert_eq!(run(&mut fx, on(48)), vec![on(60), on(63), on(67)]);
    }

    #[test]
    fn a_pitch_class_with_no_key_in_range_is_skipped() {
        // Range 60..=62 holds only pcs 0, 1, 2; C minor keeps just its
        // root.
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 60, 62, false);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn include_played_adds_the_played_key_once() {
        // Played 61 is not a C minor key: four notes.
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, true);
        assert_eq!(run(&mut fx, on(61)), vec![on(60), on(63), on(55), on(61)]);
        assert_eq!(
            run(&mut fx, off(61)),
            vec![off(60), off(63), off(55), off(61)]
        );
        // Played 60 doubles the voiced root: emitted once, three notes.
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, true);
        assert_eq!(run(&mut fx, on(60)), vec![on(60), on(63), on(55)]);
    }

    #[test]
    fn triad_notes_keep_the_input_velocity() {
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, true);
        let quiet = EventKind::NoteOn {
            ch: 0,
            key: 61,
            vel: 9,
        };
        let out = run(&mut fx, quiet);
        assert_eq!(out.len(), 4);
        for kind in out {
            assert!(matches!(kind, EventKind::NoteOn { vel: 9, .. }), "{kind:?}");
        }
    }

    #[test]
    fn an_empty_sequence_falls_back_to_r_then_l() {
        // From C major: R gives A minor, then L gives F major.
        let mut fx = Tonnetz::new(0, false, &[], 0, 127, false);
        assert_eq!(pcs(&mut fx, 60), vec![0, 4, 9]);
        assert_eq!(pcs(&mut fx, 60), vec![0, 5, 9]);
        // And R again: F major -> D minor {2, 5, 9}.
        assert_eq!(pcs(&mut fx, 60), vec![2, 5, 9]);
    }

    #[test]
    fn the_walk_is_deterministic() {
        let mut a = Tonnetz::new(4, true, &[Plr::R, Plr::P, Plr::L], 30, 100, true);
        let mut b = Tonnetz::new(4, true, &[Plr::R, Plr::P, Plr::L], 30, 100, true);
        for key in [60, 64, 67, 60, 55, 72] {
            assert_eq!(run(&mut a, on(key)), run(&mut b, on(key)));
            assert_eq!(run(&mut a, off(key)), run(&mut b, off(key)));
        }
    }

    #[test]
    fn retrigger_cuts_the_previous_triad_and_advances() {
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, false);
        assert_eq!(run(&mut fx, on(60)), vec![on(60), on(63), on(55)]);
        // The second strike cuts C minor and applies P again: C major,
        // with the fifth again voiced at 55 (5 below 60 beats 67).
        assert_eq!(
            run(&mut fx, on(60)),
            vec![off(60), off(63), off(55), on(60), on(64), on(55)]
        );
        assert_eq!(run(&mut fx, off(60)), vec![off(60), off(64), off(55)]);
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, false);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn poly_pressure_follows_the_sounding_set() {
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, false);
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
        assert_eq!(run(&mut fx, pressure), vec![at(60), at(63), at(55)]);
        // With nothing sounding it is dropped.
        run(&mut fx, off(60));
        assert_eq!(run(&mut fx, pressure), vec![]);
    }

    #[test]
    fn flush_releases_the_whole_set() {
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, true);
        run(&mut fx, on(61));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off(55), off(60), off(61), off(63)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn reversed_bounds_swap() {
        let mut a = Tonnetz::new(0, false, &[Plr::P], 71, 60, false);
        let mut b = Tonnetz::new(0, false, &[Plr::P], 60, 71, false);
        assert_eq!(run(&mut a, on(48)), run(&mut b, on(48)));
    }

    #[test]
    fn other_events_pass() {
        let mut fx = Tonnetz::new(0, false, &[Plr::P], 0, 127, true);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
