//! Serial discipline: every note-on takes the next pitch class of a
//! twelve-tone row.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::{NoteRouter, push};

/// The four classical transformations of a twelve-tone row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowForm {
    /// The row as given: P[i] = row[i].
    Prime,
    /// Each pitch class reflected about the row's first note:
    /// I[i] = (2 * row[0] - row[i]) mod 12, so I[0] = row[0].
    Inversion,
    /// The prime read backwards: R[i] = row[11 - i].
    Retrograde,
    /// The inversion read backwards: RI[i] = I[11 - i].
    RetrogradeInversion,
}

/// Replace every note-on's pitch class with the next element of a
/// twelve-tone row under the chosen form, wrapping after twelve, the way a
/// Schoenberg or Webern line spends the row and starts over. `transpose`
/// rotates the whole row's pitch classes by that many semitones. The input
/// note keeps its octave: out = octave * 12 + class, folded one octave
/// down when that leaves the keyboard (only the top part-octave can), so a
/// note-on is never dropped and the row position always advances exactly
/// once per note-on. Note-offs and poly pressure follow their note-on
/// through the router and are dropped when nothing is sounding.
pub struct RowSnap {
    /// The transformed, transposed row, fixed at construction.
    classes: [u8; 12],
    /// Next row position, 0..=11.
    pos: u8,
    router: NoteRouter,
}

impl RowSnap {
    /// `row` must be a permutation of the pitch classes 0..=11 (caller
    /// validated); entries are reduced mod 12 defensively.
    pub fn new(row: [u8; 12], form: RowForm, transpose: i8) -> Self {
        let shift = (transpose as i16).rem_euclid(12) as u8;
        let first = row[0] % 12;
        let classes = std::array::from_fn(|i| {
            let j = match form {
                RowForm::Prime | RowForm::Inversion => i,
                RowForm::Retrograde | RowForm::RetrogradeInversion => 11 - i,
            };
            let pc = row[j] % 12;
            let pc = match form {
                RowForm::Prime | RowForm::Retrograde => pc,
                RowForm::Inversion | RowForm::RetrogradeInversion => (24 + 2 * first - pc) % 12,
            };
            (pc + shift) % 12
        });
        Self {
            classes,
            pos: 0,
            router: NoteRouter::new(),
        }
    }

    /// The next row element in the input note's octave, folding the top
    /// part-octave (out would exceed 127 only for keys 120..=127, and one
    /// octave down always fits).
    fn next(&mut self, key: u8) -> u8 {
        let pc = self.classes[self.pos as usize];
        self.pos = (self.pos + 1) % 12;
        let out = (key / 12) * 12 + pc;
        if out > 127 { out - 12 } else { out }
    }
}

impl Effect for RowSnap {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                let mapped = Some(self.next(key));
                self.router.note_on(ev, mapped, out, cx);
            }
            EventKind::NoteOff { .. } => {
                self.router.note_off(ev, None, out, cx);
            }
            EventKind::PolyPressure { .. } => {
                self.router.poly_pressure(ev, None, out, cx);
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

    /// The hand-worked row for every form test below. First element 0, so
    /// I[i] = (12 - P[i]) mod 12.
    const ROW: [u8; 12] = [0, 11, 3, 4, 8, 7, 9, 5, 6, 1, 2, 10];

    /// Play `count` note-ons on `key` (releasing each) and collect the
    /// output keys.
    fn walk(fx: &mut RowSnap, key: u8, count: usize) -> Vec<u8> {
        (0..count)
            .map(|_| {
                let out = run(fx, on(key));
                let [EventKind::NoteOn { key: key_out, .. }] = out[..] else {
                    panic!("expected exactly one note-on, got {out:?}");
                };
                run(fx, off(key));
                key_out
            })
            .collect()
    }

    #[test]
    fn prime_walks_the_row_in_the_input_octave() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 0);
        let expected: Vec<u8> = ROW.iter().map(|pc| 60 + pc).collect();
        assert_eq!(walk(&mut fx, 60, 12), expected);
    }

    #[test]
    fn inversion_reflects_about_the_first_element() {
        let mut fx = RowSnap::new(ROW, RowForm::Inversion, 0);
        let inverted = [0, 1, 9, 8, 4, 5, 3, 7, 6, 11, 10, 2];
        let expected: Vec<u8> = inverted.iter().map(|pc| 60 + pc).collect();
        assert_eq!(walk(&mut fx, 60, 12), expected);
    }

    #[test]
    fn retrograde_reads_the_prime_backwards() {
        let mut fx = RowSnap::new(ROW, RowForm::Retrograde, 0);
        let expected: Vec<u8> = ROW.iter().rev().map(|pc| 60 + pc).collect();
        assert_eq!(walk(&mut fx, 60, 12), expected);
    }

    #[test]
    fn retrograde_inversion_reads_the_inversion_backwards() {
        let mut fx = RowSnap::new(ROW, RowForm::RetrogradeInversion, 0);
        let inverted = [0, 1, 9, 8, 4, 5, 3, 7, 6, 11, 10, 2];
        let expected: Vec<u8> = inverted.iter().rev().map(|pc| 60 + pc).collect();
        assert_eq!(walk(&mut fx, 60, 12), expected);
    }

    #[test]
    fn transpose_rotates_the_pitch_classes() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 3);
        let expected: Vec<u8> = ROW.iter().map(|pc| 60 + (pc + 3) % 12).collect();
        assert_eq!(walk(&mut fx, 60, 12), expected);
        // Negative transposition wraps the other way.
        let mut fx = RowSnap::new(ROW, RowForm::Prime, -5);
        let expected: Vec<u8> = ROW.iter().map(|pc| 60 + (pc + 7) % 12).collect();
        assert_eq!(walk(&mut fx, 60, 12), expected);
    }

    #[test]
    fn the_thirteenth_note_wraps_to_the_row_start() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 0);
        let keys = walk(&mut fx, 60, 13);
        assert_eq!(keys[12], keys[0]);
    }

    #[test]
    fn the_input_octave_is_kept() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 0);
        assert_eq!(walk(&mut fx, 60, 1), vec![60 + ROW[0]]);
        assert_eq!(walk(&mut fx, 30, 1), vec![24 + ROW[1]]);
        assert_eq!(walk(&mut fx, 5, 1), vec![ROW[2]]);
    }

    #[test]
    fn the_top_octave_folds_instead_of_dropping() {
        // Transposed so the first element is pitch class 11: octave 10
        // would put it at 131, so it folds down to 119.
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 11);
        assert_eq!(walk(&mut fx, 125, 1), vec![119]);
        // Pitch class 10 next: 130 folds to 118.
        assert_eq!(walk(&mut fx, 120, 1), vec![118]);
    }

    #[test]
    fn note_off_follows_and_the_position_holds() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 0);
        assert_eq!(run(&mut fx, on(60)), vec![on(60 + ROW[0])]);
        // The off must not advance the row.
        assert_eq!(run(&mut fx, off(60)), vec![off(60 + ROW[0])]);
        assert_eq!(run(&mut fx, on(60)), vec![on(60 + ROW[1])]);
    }

    #[test]
    fn retrigger_cuts_and_advances() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 0);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        // Same key again: the first row note ends, the second begins.
        assert_eq!(run(&mut fx, on(60)), vec![off(60), on(71)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(71)]);
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = RowSnap::new(ROW, RowForm::Prime, 0);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }
}
