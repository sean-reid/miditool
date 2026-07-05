//! Stockhausen ring modulation: sum and difference tones on the keyboard.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::router::push;

/// The component keys one input note produces: sum, difference, and dry,
/// deduplicated, in that order. `Default` (all `None`) marks an inactive
/// slot.
type Components = [Option<u8>; 3];

/// Ring-modulate every note against a fixed carrier, the electronics of
/// Stockhausen's Mixtur and Mantra reduced to the keyboard: the played key
/// and `carrier` become frequencies (440 * 2^((k - 69) / 12)), and the sum
/// and/or absolute difference come back as the nearest MIDI keys, plus the
/// dry note when `dry`. A component below the frequency of MIDI key 0
/// (about 8.18 Hz) or above key 127 is dropped; components of one input
/// note that land on the same key are emitted once.
///
/// Every emitted component gets its own note-off: the full component set
/// is remembered per input (channel, key) at note-on, the note-off
/// releases exactly that set, a retrigger cuts it first, and `flush`
/// releases everything. The mapping is deterministic, so an orphan
/// note-off (or poly pressure with nothing sounding) is mapped statelessly
/// instead of dropped, like `Transpose`. Non-note events pass unchanged.
///
/// Fanout bound: at most 3 retrigger cuts plus 3 note-ons per input event,
/// well under `MAX_FANOUT`.
pub struct RingMod {
    carrier: u8,
    sum: bool,
    diff: bool,
    dry: bool,
    /// The component set per active input (channel, key).
    active: PerNote<Components>,
}

/// Equal-tempered frequency of a MIDI key, A4 = 440 Hz.
fn freq(key: u8) -> f32 {
    440.0 * ((key as f32 - 69.0) / 12.0).exp2()
}

/// The nearest MIDI key for a frequency, or `None` when it falls below
/// key 0 or rounds above key 127.
fn key_of(f: f32) -> Option<u8> {
    if f < freq(0) {
        return None;
    }
    let key = (69.0 + 12.0 * (f / 440.0).log2()).round();
    (key <= 127.0).then_some(key.max(0.0) as u8)
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

impl RingMod {
    /// `carrier` is clamped to 127. At least one of `sum`, `diff`, `dry`
    /// should be true (caller validated); with none set, `dry` is forced
    /// on so the effect degrades to a pass instead of eating every note.
    pub fn new(carrier: u8, sum: bool, diff: bool, dry: bool) -> Self {
        Self {
            carrier: carrier.min(127),
            sum,
            diff,
            dry: dry || (!sum && !diff),
            active: PerNote::new(),
        }
    }

    fn components(&self, key: u8) -> Components {
        let f_in = freq(key);
        let f_carrier = freq(self.carrier);
        let mut set: Components = [None; 3];
        if self.sum {
            add(&mut set, key_of(f_in + f_carrier));
        }
        if self.diff {
            add(&mut set, key_of((f_in - f_carrier).abs()));
        }
        if self.dry {
            add(&mut set, Some(key));
        }
        set
    }

    /// The remembered set for an input note, or the statelessly recomputed
    /// one when nothing is remembered (the orphan-note-off fallback; the
    /// mapping is deterministic, so both agree whenever a note-on was
    /// seen).
    fn recall(set: Components, fallback: impl FnOnce() -> Components) -> Components {
        if set.iter().all(Option::is_none) {
            fallback()
        } else {
            set
        }
    }
}

impl Effect for RingMod {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                // Retrigger: cut every component the previous strike left
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
                let set = Self::recall(self.active.take(ch, key), || self.components(key));
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
                let set = Self::recall(self.active.get(ch, key), || self.components(key));
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

    #[test]
    fn sum_and_diff_match_the_hand_computed_case() {
        // Carrier 60 (261.63 Hz) against input 64 (329.63 Hz): the sum is
        // 591.25 Hz, nearest key 74; the difference is 68.00 Hz, nearest
        // key 37.
        let mut fx = RingMod::new(60, true, true, false);
        assert_eq!(run(&mut fx, on(64)), vec![on(74), on(37)]);
        assert_eq!(run(&mut fx, off(64)), vec![off(74), off(37)]);
    }

    #[test]
    fn dry_adds_the_played_note() {
        let mut fx = RingMod::new(60, true, true, true);
        assert_eq!(run(&mut fx, on(64)), vec![on(74), on(37), on(64)]);
        assert_eq!(run(&mut fx, off(64)), vec![off(74), off(37), off(64)]);
    }

    #[test]
    fn input_at_the_carrier_doubles_up_an_octave() {
        // Equal frequencies: the sum is exactly an octave above and the
        // difference is 0 Hz, below any note, so it drops.
        let mut fx = RingMod::new(60, true, true, false);
        assert_eq!(run(&mut fx, on(60)), vec![on(72)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(72)]);
    }

    #[test]
    fn colliding_components_are_emitted_once() {
        // Carrier 0 (8.18 Hz) barely detunes input 73 (554.37 Hz): sum
        // (562.54 Hz) and difference (546.19 Hz) both round back to key
        // 73, and so does the dry note. One note-on, one note-off.
        let mut fx = RingMod::new(0, true, true, true);
        assert_eq!(run(&mut fx, on(73)), vec![on(73)]);
        assert_eq!(run(&mut fx, off(73)), vec![off(73)]);
    }

    #[test]
    fn out_of_range_components_drop_with_their_offs() {
        // Carrier 127 against input 127: the sum rounds to key 139, off
        // the keyboard, and diff is 0 Hz, so nothing sounds at all.
        let mut fx = RingMod::new(127, true, true, false);
        assert_eq!(run(&mut fx, on(127)), vec![]);
        assert_eq!(run(&mut fx, off(127)), vec![]);
    }

    #[test]
    fn retrigger_cuts_every_component() {
        let mut fx = RingMod::new(60, true, true, false);
        assert_eq!(run(&mut fx, on(64)), vec![on(74), on(37)]);
        assert_eq!(run(&mut fx, on(64)), vec![off(74), off(37), on(74), on(37)]);
        assert_eq!(run(&mut fx, off(64)), vec![off(74), off(37)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly() {
        let mut fx = RingMod::new(60, true, true, false);
        assert_eq!(run(&mut fx, off(64)), vec![off(74), off(37)]);
    }

    #[test]
    fn poly_pressure_follows_every_component() {
        let mut fx = RingMod::new(60, true, true, false);
        run(&mut fx, on(64));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 64,
            value: 33,
        };
        let at = |key| EventKind::PolyPressure {
            ch: 0,
            key,
            value: 33,
        };
        assert_eq!(run(&mut fx, pressure), vec![at(74), at(37)]);
    }

    #[test]
    fn flush_releases_every_component() {
        let mut fx = RingMod::new(60, true, true, false);
        run(&mut fx, on(64));
        let mut released = flush(&mut fx);
        released.sort_by_key(|kind| kind.key());
        assert_eq!(released, vec![off(37), off(74)]);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn all_flags_off_degrades_to_dry() {
        let mut fx = RingMod::new(60, false, false, false);
        assert_eq!(run(&mut fx, on(64)), vec![on(64)]);
        assert_eq!(run(&mut fx, off(64)), vec![off(64)]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = RingMod::new(60, true, true, true);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
