//! Messiaen's modes of limited transposition as a keyboard lock.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::{NoteRouter, push};
use crate::sieve_quantizer::SieveSnap;

/// The seven modes of limited transposition as pitch-class sets at
/// transposition 0, indexed by mode number minus one:
///
/// 1. Whole tone: {0, 2, 4, 6, 8, 10}, the scale of Voiles.
/// 2. Octatonic (half-whole diminished): {0, 1, 3, 4, 6, 7, 9, 10}.
/// 3. Tone plus two semitones, repeated three times:
///    {0, 2, 3, 4, 6, 7, 8, 10, 11}.
/// 4. Two semitones, a minor third, a semitone, twice:
///    {0, 1, 2, 5, 6, 7, 8, 11}.
/// 5. Semitone, major third, semitone, twice: {0, 1, 5, 6, 7, 11}.
/// 6. Two whole tones and two semitones, twice:
///    {0, 2, 4, 5, 6, 8, 10, 11}.
/// 7. Three semitones, a whole tone, a semitone, twice:
///    {0, 1, 2, 3, 5, 6, 7, 8, 9, 11}.
const MODES: [&[u8]; 7] = [
    &[0, 2, 4, 6, 8, 10],
    &[0, 1, 3, 4, 6, 7, 9, 10],
    &[0, 2, 3, 4, 6, 7, 8, 10, 11],
    &[0, 1, 2, 5, 6, 7, 8, 11],
    &[0, 1, 5, 6, 7, 11],
    &[0, 2, 4, 5, 6, 8, 10, 11],
    &[0, 1, 2, 3, 5, 6, 7, 8, 9, 11],
];

/// Lock the keyboard onto one of Messiaen's modes of limited
/// transposition: note-ons whose pitch class is outside the transposed
/// mode snap per the `SieveSnap` semantics (`Nearest` with ties breaking
/// downward, `Up`, `Down`, or `Drop`), members pass untouched. The mode's
/// pitch-class set is rotated upward by `transposition` semitones; a
/// plain modulo 12 suffices, since rotating past a mode's period simply
/// lands on a set it already produced.
///
/// Every mode repeats within the octave, so a member is never more than a
/// few semitones away and `Nearest` always finds one; `Up` and `Down` can
/// only miss within the top or bottom part-octave, where they drop the
/// pair like the sieve quantizer. The mapping is deterministic; the
/// router keeps note-offs, retriggers, and poly pressure consistent, and
/// maps orphan note-offs statelessly.
pub struct ModeLock {
    /// Bit `pc` set means the pitch class is in the transposed mode.
    pcs: u16,
    snap: SieveSnap,
    router: NoteRouter,
}

impl ModeLock {
    /// `mode` is clamped to 1..=7 and `transposition` reduced modulo 12.
    pub fn new(mode: u8, transposition: u8, snap: SieveSnap) -> Self {
        let mode = mode.clamp(1, 7);
        let shift = transposition % 12;
        let mut pcs = 0u16;
        for &pc in MODES[mode as usize - 1] {
            pcs |= 1 << ((pc + shift) % 12);
        }
        Self {
            pcs,
            snap,
            router: NoteRouter::new(),
        }
    }

    fn contains(&self, key: u8) -> bool {
        self.pcs >> (key % 12) & 1 == 1
    }

    /// The member closest to `key`, ties breaking downward. The set is
    /// never empty, so this always finds one.
    fn nearest(&self, key: u8) -> Option<u8> {
        for d in 0..12u8 {
            if key >= d && self.contains(key - d) {
                return Some(key - d);
            }
            let above = key as u16 + d as u16;
            if above <= 127 && self.contains(above as u8) {
                return Some(above as u8);
            }
        }
        None
    }

    fn map(&self, key: u8) -> Option<u8> {
        match self.snap {
            SieveSnap::Nearest => self.nearest(key),
            SieveSnap::Up => (key..=127).find(|&k| self.contains(k)),
            SieveSnap::Down => (0..=key).rev().find(|&k| self.contains(k)),
            SieveSnap::Drop => self.contains(key).then_some(key),
        }
    }
}

impl Effect for ModeLock {
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

    /// Membership of every pitch class per mode, checked against the
    /// documented sets.
    #[test]
    fn every_mode_matches_its_documented_pitch_class_set() {
        let expected: [&[u8]; 7] = [
            &[0, 2, 4, 6, 8, 10],
            &[0, 1, 3, 4, 6, 7, 9, 10],
            &[0, 2, 3, 4, 6, 7, 8, 10, 11],
            &[0, 1, 2, 5, 6, 7, 8, 11],
            &[0, 1, 5, 6, 7, 11],
            &[0, 2, 4, 5, 6, 8, 10, 11],
            &[0, 1, 2, 3, 5, 6, 7, 8, 9, 11],
        ];
        for (mode, pcs) in expected.iter().enumerate() {
            let mut fx = ModeLock::new(mode as u8 + 1, 0, SieveSnap::Drop);
            for pc in 0..12u8 {
                let expected_pass = pcs.contains(&pc);
                let out = run(&mut fx, on(60 + pc));
                assert_eq!(
                    out.len(),
                    expected_pass as usize,
                    "mode {} pc {pc}",
                    mode + 1
                );
                run(&mut fx, off(60 + pc));
            }
        }
    }

    #[test]
    fn transposition_rotates_the_set() {
        // Mode 5 at transposition 2: {2, 3, 7, 8, 9, 1}.
        let mut fx = ModeLock::new(5, 2, SieveSnap::Drop);
        for pc in 0..12u8 {
            let member = [1, 2, 3, 7, 8, 9].contains(&pc);
            assert_eq!(run(&mut fx, on(60 + pc)).len(), member as usize, "pc {pc}");
            run(&mut fx, off(60 + pc));
        }
        // Transposition wraps modulo 12.
        let mut a = ModeLock::new(2, 13, SieveSnap::Nearest);
        let mut b = ModeLock::new(2, 1, SieveSnap::Nearest);
        assert_eq!(run(&mut a, on(60)), run(&mut b, on(60)));
    }

    #[test]
    fn nearest_snaps_with_ties_downward() {
        // Whole tone: 61 sits between 60 and 62, the tie breaks down.
        let mut fx = ModeLock::new(1, 0, SieveSnap::Nearest);
        assert_eq!(run(&mut fx, on(61)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(60)]);
        // Mode 5 {0, 1, 5, 6, 7, 11}: 63 ties between 61 and 65 and the
        // tie breaks down; 64 is strictly nearer to 65.
        let mut fx = ModeLock::new(5, 0, SieveSnap::Nearest);
        assert_eq!(run(&mut fx, on(63)), vec![on(61)]);
        assert_eq!(run(&mut fx, off(63)), vec![off(61)]);
        assert_eq!(run(&mut fx, on(64)), vec![on(65)]);
        assert_eq!(run(&mut fx, off(64)), vec![off(65)]);
    }

    #[test]
    fn up_and_down_snap_in_their_direction() {
        let mut fx = ModeLock::new(1, 0, SieveSnap::Up);
        assert_eq!(run(&mut fx, on(61)), vec![on(62)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(62)]);
        let mut fx = ModeLock::new(1, 0, SieveSnap::Down);
        assert_eq!(run(&mut fx, on(61)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(60)]);
    }

    #[test]
    fn up_and_down_drop_past_the_last_member() {
        // Mode 5 transposed by 9: {9, 10, 2, 3, 4, 8}. The top keys 125,
        // 126, 127 (pcs 5, 6, 7) hold no member, nor do the bottom keys
        // 0 and 1 (pcs 0 and 1).
        let mut fx = ModeLock::new(5, 9, SieveSnap::Up);
        assert_eq!(run(&mut fx, on(125)), vec![]);
        assert_eq!(run(&mut fx, off(125)), vec![]);
        let mut fx = ModeLock::new(5, 9, SieveSnap::Down);
        assert_eq!(run(&mut fx, on(1)), vec![]);
        assert_eq!(run(&mut fx, off(1)), vec![]);
    }

    #[test]
    fn drop_silences_non_members() {
        let mut fx = ModeLock::new(1, 0, SieveSnap::Drop);
        assert_eq!(run(&mut fx, on(61)), vec![]);
        assert_eq!(run(&mut fx, off(61)), vec![]);
        assert_eq!(run(&mut fx, on(62)), vec![on(62)]);
        assert_eq!(run(&mut fx, off(62)), vec![off(62)]);
    }

    #[test]
    fn mode_clamps() {
        let mut lo = ModeLock::new(0, 0, SieveSnap::Drop);
        let mut one = ModeLock::new(1, 0, SieveSnap::Drop);
        assert_eq!(run(&mut lo, on(61)), run(&mut one, on(61)));
        let mut hi = ModeLock::new(9, 0, SieveSnap::Drop);
        let mut seven = ModeLock::new(7, 0, SieveSnap::Drop);
        assert_eq!(run(&mut hi, on(64)), run(&mut seven, on(64)));
    }

    #[test]
    fn retrigger_cuts_the_snapped_note() {
        let mut fx = ModeLock::new(1, 0, SieveSnap::Nearest);
        assert_eq!(run(&mut fx, on(61)), vec![on(60)]);
        assert_eq!(run(&mut fx, on(61)), vec![off(60), on(60)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(60)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly() {
        let mut fx = ModeLock::new(1, 0, SieveSnap::Nearest);
        assert_eq!(run(&mut fx, off(61)), vec![off(60)]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = ModeLock::new(2, 0, SieveSnap::Drop);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
