//! Ligeti's touches bloquees: keys held silent under the playing hand.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::{NoteRouter, push};

/// Silence a fixed set of keys, the way the silent hand in Ligeti's
/// "Touches bloquees" holds keys down so the playing hand's strikes on
/// them produce nothing but the gap. A note-on whose key (or pitch class,
/// with `by_class`) is in the set is dropped, and its matching note-off
/// drops with it through the router; everything else passes unchanged.
pub struct BlockedKeys {
    blocked: [bool; 128],
    router: NoteRouter,
}

impl BlockedKeys {
    /// `items` are keys, or pitch classes when `by_class` (values are
    /// reduced mod 12 then). Keys past 127 are clamped onto the keyboard.
    pub fn new(items: &[u8], by_class: bool) -> Self {
        let mut blocked = [false; 128];
        for &item in items {
            if by_class {
                for key in ((item % 12)..128).step_by(12) {
                    blocked[key as usize] = true;
                }
            } else {
                blocked[item.min(127) as usize] = true;
            }
        }
        Self {
            blocked,
            router: NoteRouter::new(),
        }
    }

    /// Identity for open keys, `None` for blocked ones. Deterministic, so
    /// it doubles as the stateless fallback for orphan note-offs.
    fn map(&self, key: u8) -> Option<u8> {
        (!self.blocked[key as usize & 127]).then_some(key)
    }
}

impl Effect for BlockedKeys {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                self.router.note_on(ev, self.map(key), out, cx);
            }
            EventKind::NoteOff { key, .. } => {
                self.router.note_off(ev, self.map(key), out, cx);
            }
            EventKind::PolyPressure { key, .. } => {
                self.router.poly_pressure(ev, self.map(key), out, cx);
            }
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.router.flush(out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{off, on, run};

    #[test]
    fn blocked_key_drops_the_pair() {
        let mut fx = BlockedKeys::new(&[60, 64], false);
        assert_eq!(run(&mut fx, on(60)), vec![]);
        assert_eq!(run(&mut fx, off(60)), vec![]);
        assert_eq!(run(&mut fx, on(64)), vec![]);
        assert_eq!(run(&mut fx, off(64)), vec![]);
    }

    #[test]
    fn open_keys_pass_unchanged() {
        let mut fx = BlockedKeys::new(&[60], false);
        assert_eq!(run(&mut fx, on(61)), vec![on(61)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(61)]);
        // Blocking a key must not touch its other octaves.
        assert_eq!(run(&mut fx, on(72)), vec![on(72)]);
        assert_eq!(run(&mut fx, off(72)), vec![off(72)]);
    }

    #[test]
    fn by_class_blocks_every_octave() {
        let mut fx = BlockedKeys::new(&[0, 7], true);
        for key in [0, 12, 60, 120, 7, 19, 67, 127] {
            assert_eq!(run(&mut fx, on(key)), vec![], "key {key}");
            assert_eq!(run(&mut fx, off(key)), vec![], "key {key}");
        }
        assert_eq!(run(&mut fx, on(62)), vec![on(62)]);
        assert_eq!(run(&mut fx, off(62)), vec![off(62)]);
    }

    #[test]
    fn by_class_reduces_items_mod_twelve() {
        let mut fx = BlockedKeys::new(&[60], true);
        assert_eq!(run(&mut fx, on(12)), vec![]);
        assert_eq!(run(&mut fx, off(12)), vec![]);
    }

    #[test]
    fn poly_pressure_is_gated_with_its_key() {
        let mut fx = BlockedKeys::new(&[60], false);
        let blocked = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        let open = EventKind::PolyPressure {
            ch: 0,
            key: 61,
            value: 33,
        };
        assert_eq!(run(&mut fx, blocked), vec![]);
        assert_eq!(run(&mut fx, open), vec![open]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = BlockedKeys::new(&[60], false);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
