//! Quantize the keyboard onto a Xenakis sieve.

use miditool_core::sieve::Sieve;
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::{NoteRouter, push};

/// How a key that misses the sieve is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SieveSnap {
    /// Snap to the closest member, ties breaking downward.
    Nearest,
    /// Snap to the member at or above, dropping the pair when none is.
    Up,
    /// Snap to the member at or below, dropping the pair when none is.
    Down,
    /// Pass members untouched and drop everything else.
    Drop,
}

/// Force every note onto a sieve, the residue-class pitch lattices Xenakis
/// built for Jonchaies or Akrata: members pass, non-members snap per the
/// mode or drop. A parsed sieve is never empty, so `Nearest` always finds
/// a member; `Up` and `Down` drop the pair beyond the last member in their
/// direction. The mapping is deterministic; the router keeps note-offs,
/// retriggers, and poly pressure consistent, and maps orphan note-offs
/// statelessly.
pub struct SieveQuantizer {
    sieve: Sieve,
    snap: SieveSnap,
    router: NoteRouter,
}

impl SieveQuantizer {
    pub fn new(sieve: Sieve, snap: SieveSnap) -> Self {
        Self {
            sieve,
            snap,
            router: NoteRouter::new(),
        }
    }

    fn map(&self, key: u8) -> Option<u8> {
        match self.snap {
            SieveSnap::Nearest => self.sieve.nearest(key),
            SieveSnap::Up => self.sieve.up(key),
            SieveSnap::Down => self.sieve.down(key),
            SieveSnap::Drop => self.sieve.contains(key).then_some(key),
        }
    }
}

impl Effect for SieveQuantizer {
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

    /// The C major triad classes: members 0, 4, 7, 12, 16, 19, ...
    fn triad() -> Sieve {
        Sieve::parse("12@0 | 12@4 | 12@7").unwrap()
    }

    #[test]
    fn members_pass_in_every_mode() {
        for snap in [
            SieveSnap::Nearest,
            SieveSnap::Up,
            SieveSnap::Down,
            SieveSnap::Drop,
        ] {
            let mut fx = SieveQuantizer::new(triad(), snap);
            assert_eq!(run(&mut fx, on(60)), vec![on(60)], "{snap:?}");
            assert_eq!(run(&mut fx, off(60)), vec![off(60)], "{snap:?}");
        }
    }

    #[test]
    fn nearest_snaps_with_ties_downward() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Nearest);
        assert_eq!(run(&mut fx, on(61)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(60)]);
        // 62 sits between 60 and 64: the tie breaks downward.
        assert_eq!(run(&mut fx, on(62)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(62)), vec![off(60)]);
        assert_eq!(run(&mut fx, on(63)), vec![on(64)]);
        assert_eq!(run(&mut fx, off(63)), vec![off(64)]);
    }

    #[test]
    fn up_snaps_upward_and_drops_past_the_top() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Up);
        assert_eq!(run(&mut fx, on(61)), vec![on(64)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(64)]);
        // The last member is 127 (12@7 reaches it), so nothing drops up
        // here; use a sieve topping out at 119 instead.
        let mut fx = SieveQuantizer::new(Sieve::parse("12@11").unwrap(), SieveSnap::Up);
        assert_eq!(run(&mut fx, on(120)), vec![]);
        assert_eq!(run(&mut fx, off(120)), vec![]);
    }

    #[test]
    fn down_snaps_downward_and_drops_past_the_bottom() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Down);
        assert_eq!(run(&mut fx, on(63)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(63)), vec![off(60)]);
        let mut fx = SieveQuantizer::new(Sieve::parse("12@11").unwrap(), SieveSnap::Down);
        assert_eq!(run(&mut fx, on(5)), vec![]);
        assert_eq!(run(&mut fx, off(5)), vec![]);
    }

    #[test]
    fn drop_silences_non_members() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Drop);
        assert_eq!(run(&mut fx, on(61)), vec![]);
        assert_eq!(run(&mut fx, off(61)), vec![]);
        assert_eq!(run(&mut fx, on(67)), vec![on(67)]);
        assert_eq!(run(&mut fx, off(67)), vec![off(67)]);
    }

    #[test]
    fn retrigger_cuts_the_snapped_note() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Nearest);
        assert_eq!(run(&mut fx, on(61)), vec![on(60)]);
        assert_eq!(run(&mut fx, on(61)), vec![off(60), on(60)]);
        assert_eq!(run(&mut fx, off(61)), vec![off(60)]);
    }

    #[test]
    fn orphan_note_off_maps_statelessly() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Nearest);
        assert_eq!(run(&mut fx, off(62)), vec![off(60)]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = SieveQuantizer::new(triad(), SieveSnap::Drop);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
