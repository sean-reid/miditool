//! Fixed transposition in semitones.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::{NoteRouter, push};

/// Shift note-on, note-off, and poly-pressure keys by a fixed number of
/// semitones. A note-on shifted outside 0..=127 is dropped, and so is its
/// matching note-off. Everything else passes through.
pub struct Transpose {
    semis: i16,
    router: NoteRouter,
}

impl Transpose {
    pub fn new(semis: i16) -> Self {
        Self {
            semis,
            router: NoteRouter::new(),
        }
    }

    fn shift(&self, key: u8) -> Option<u8> {
        let shifted = key as i16 + self.semis;
        (0..=127).contains(&shifted).then_some(shifted as u8)
    }
}

impl Effect for Transpose {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                self.router.note_on(ev, self.shift(key), out, cx);
            }
            EventKind::NoteOff { key, .. } => {
                self.router.note_off(ev, self.shift(key), out, cx);
            }
            EventKind::PolyPressure { key, .. } => {
                self.router.poly_pressure(ev, self.shift(key), out, cx);
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
    use crate::testutil::{flush, off, on, run};

    #[test]
    fn in_range_round_trips() {
        let mut fx = Transpose::new(7);
        assert_eq!(run(&mut fx, on(60)), vec![on(67)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(67)]);
    }

    #[test]
    fn out_of_range_drops_note_on_and_its_note_off() {
        let mut fx = Transpose::new(12);
        assert_eq!(run(&mut fx, on(120)), vec![]);
        assert_eq!(run(&mut fx, off(120)), vec![]);

        let mut fx = Transpose::new(-12);
        assert_eq!(run(&mut fx, on(5)), vec![]);
        assert_eq!(run(&mut fx, off(5)), vec![]);
    }

    #[test]
    fn retrigger_cuts_previous_note() {
        let mut fx = Transpose::new(7);
        assert_eq!(run(&mut fx, on(60)), vec![on(67)]);
        assert_eq!(run(&mut fx, on(60)), vec![off(67), on(67)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(67)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly() {
        let mut fx = Transpose::new(7);
        assert_eq!(run(&mut fx, off(50)), vec![off(57)]);
    }

    #[test]
    fn poly_pressure_follows_the_note() {
        let mut fx = Transpose::new(7);
        run(&mut fx, on(60));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        assert_eq!(
            run(&mut fx, pressure),
            vec![EventKind::PolyPressure {
                ch: 0,
                key: 67,
                value: 33
            }]
        );
    }

    #[test]
    fn other_events_pass() {
        let mut fx = Transpose::new(7);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }

    #[test]
    fn flush_releases_active_notes() {
        let mut fx = Transpose::new(7);
        run(&mut fx, on(60));
        assert_eq!(flush(&mut fx), vec![off(67)]);
        assert_eq!(flush(&mut fx), vec![]);
    }
}
