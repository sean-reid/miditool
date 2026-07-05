//! Interval telescoping: stretch or compress the keyboard about a pivot.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::{NoteRouter, push};

/// Scale every note's distance from a reference key:
/// out = reference + round((key - reference) * factor). A factor above 1
/// stretches intervals away from the pivot, a factor between 0 and 1
/// compresses toward it, and a negative factor inverts about it. A result
/// outside 0..=127 drops the note-on and its note-off with it. The mapping
/// is deterministic; the router keeps note-offs, retriggers, and poly
/// pressure consistent, and maps orphan note-offs statelessly.
pub struct Telescope {
    factor: f32,
    reference: u8,
    router: NoteRouter,
}

impl Telescope {
    /// `reference` is clamped to 127.
    pub fn new(factor: f32, reference: u8) -> Self {
        Self {
            factor,
            reference: reference.min(127),
            router: NoteRouter::new(),
        }
    }

    fn map(&self, key: u8) -> Option<u8> {
        let reference = self.reference as f32;
        let out = (reference + (key as f32 - reference) * self.factor).round();
        // NaN (a NaN factor) fails the range test and drops the note.
        (0.0..=127.0).contains(&out).then_some(out as u8)
    }
}

impl Effect for Telescope {
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
    fn factor_two_doubles_intervals() {
        let mut fx = Telescope::new(2.0, 60);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
        assert_eq!(run(&mut fx, on(67)), vec![on(74)]);
        assert_eq!(run(&mut fx, off(67)), vec![off(74)]);
        assert_eq!(run(&mut fx, on(55)), vec![on(50)]);
        assert_eq!(run(&mut fx, off(55)), vec![off(50)]);
    }

    #[test]
    fn fractional_factor_compresses_and_rounds() {
        let mut fx = Telescope::new(0.5, 60);
        assert_eq!(run(&mut fx, on(72)), vec![on(66)]);
        assert_eq!(run(&mut fx, off(72)), vec![off(66)]);
        // 60 + (67 - 60) * 0.5 = 63.5 rounds to 64.
        assert_eq!(run(&mut fx, on(67)), vec![on(64)]);
        assert_eq!(run(&mut fx, off(67)), vec![off(64)]);
    }

    #[test]
    fn negative_factor_inverts_about_the_reference() {
        let mut fx = Telescope::new(-1.0, 60);
        assert_eq!(run(&mut fx, on(65)), vec![on(55)]);
        assert_eq!(run(&mut fx, off(65)), vec![off(55)]);
    }

    #[test]
    fn out_of_range_drops_the_pair() {
        let mut fx = Telescope::new(2.0, 60);
        // 60 + (100 - 60) * 2 = 140: gone, together with its off.
        assert_eq!(run(&mut fx, on(100)), vec![]);
        assert_eq!(run(&mut fx, off(100)), vec![]);
        // 60 + (20 - 60) * 2 = -20 drops at the low edge.
        assert_eq!(run(&mut fx, on(20)), vec![]);
        assert_eq!(run(&mut fx, off(20)), vec![]);
    }

    #[test]
    fn retrigger_cuts_the_previous_note() {
        let mut fx = Telescope::new(2.0, 60);
        assert_eq!(run(&mut fx, on(67)), vec![on(74)]);
        assert_eq!(run(&mut fx, on(67)), vec![off(74), on(74)]);
        assert_eq!(run(&mut fx, off(67)), vec![off(74)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly() {
        let mut fx = Telescope::new(2.0, 60);
        assert_eq!(run(&mut fx, off(67)), vec![off(74)]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = Telescope::new(2.0, 60);
        let bend = EventKind::PitchBend { ch: 0, value: 512 };
        assert_eq!(run(&mut fx, bend), vec![bend]);
    }
}
