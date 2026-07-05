//! Isorhythm: a fixed cycle of durations, whatever the player holds.

use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};

use crate::router::push;

/// Impose a talea in the manner of the ars nova motet: the player's
/// durations are discarded and each note-on is emitted together with a
/// scheduled note-off after the next duration in the cycling list,
/// advancing once per note-on. Each pair is self-contained via
/// `push_pair`, so buffer truncation can never keep the on and drop the
/// off.
///
/// The player's real note-offs are swallowed, the duration-lottery idiom:
/// the scheduled off already balances every on, so the real one would end
/// a note the talea still owns, and an orphan off has no scheduled note
/// to end. Poly pressure passes through on the played key unchanged,
/// since the scheduled note sounds on that same key while it lasts.
/// Retriggering a held key emits a fresh pair without a cut; the
/// downstream tracker counts stacked ons and each carries its own
/// scheduled off. All other events pass untouched. `flush` emits nothing:
/// every off is already scheduled at note-on time.
///
/// Fanout bound: exactly 2 outputs per note-on, well under `MAX_FANOUT`.
pub struct Talea {
    /// The cycle, fixed at construction.
    durations_ns: Vec<u64>,
    /// Next cycling position.
    next: usize,
}

impl Talea {
    /// At most 32 entries are kept, each clamped to 1ms..=60s; an empty
    /// list falls back to a single 250ms entry defensively.
    pub fn new(durations_ns: &[u64]) -> Self {
        let durations_ns: Vec<u64> = durations_ns
            .iter()
            .take(32)
            .map(|&d| d.clamp(1_000_000, 60_000_000_000))
            .collect();
        let durations_ns = if durations_ns.is_empty() {
            vec![250_000_000]
        } else {
            durations_ns
        };
        Self {
            durations_ns,
            next: 0,
        }
    }

    fn advance(&mut self) -> u64 {
        let duration = self.durations_ns[self.next];
        self.next = (self.next + 1) % self.durations_ns.len();
        duration
    }
}

impl Effect for Talea {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, .. } => {
                let duration = self.advance();
                let release = EventKind::NoteOff { ch, key, vel: 0 };
                cx.push_pair(
                    out,
                    *ev,
                    Event::new(ev.time.saturating_add(duration), release),
                );
            }
            // The scheduled off balances the on; the player's off (or an
            // orphan) is swallowed.
            EventKind::NoteOff { .. } => {}
            _ => push(out, cx, *ev),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    const MS: u64 = 1_000_000;

    #[test]
    fn durations_cycle_and_wrap() {
        let mut fx = Talea::new(&[100 * MS, 200 * MS, 300 * MS]);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(100 * MS, off(60))]
        );
        assert_eq!(
            run_timed(&mut fx, 10, on(62)),
            vec![at(10, on(62)), at(200 * MS + 10, off(62))]
        );
        assert_eq!(
            run_timed(&mut fx, 20, on(64)),
            vec![at(20, on(64)), at(300 * MS + 20, off(64))]
        );
        // The fourth note wraps back to the first duration.
        assert_eq!(
            run_timed(&mut fx, 30, on(65)),
            vec![at(30, on(65)), at(100 * MS + 30, off(65))]
        );
    }

    #[test]
    fn the_players_note_off_is_swallowed() {
        let mut fx = Talea::new(&[100 * MS]);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(run_timed(&mut fx, 50, off(60)), vec![]);
        // An orphan off is swallowed the same way.
        assert_eq!(run_timed(&mut fx, 60, off(72)), vec![]);
    }

    #[test]
    fn retrigger_emits_a_fresh_pair_without_a_cut() {
        let mut fx = Talea::new(&[100 * MS]);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(100 * MS, off(60))]
        );
        assert_eq!(
            run_timed(&mut fx, 50, on(60)),
            vec![at(50, on(60)), at(100 * MS + 50, off(60))]
        );
    }

    #[test]
    fn entries_clamp_into_one_ms_to_sixty_s() {
        let mut fx = Talea::new(&[0, u64::MAX]);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(MS, off(60))]
        );
        assert_eq!(
            run_timed(&mut fx, 10, on(62)),
            vec![at(10, on(62)), at(60_000_000_000 + 10, off(62))]
        );
    }

    #[test]
    fn an_empty_talea_falls_back_to_250ms() {
        let mut fx = Talea::new(&[]);
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(250 * MS, off(60))]
        );
    }

    #[test]
    fn the_cycle_keeps_at_most_32_entries() {
        // Entry 33 is discarded: the 33rd note-on wraps to the first.
        let durations: Vec<u64> = (1..=33).map(|i| i * MS).collect();
        let mut fx = Talea::new(&durations);
        for i in 0..32u64 {
            let out = run_timed(&mut fx, i, on(60));
            assert_eq!(out[1].time - out[0].time, (i + 1) * MS, "note {i}");
        }
        let out = run_timed(&mut fx, 32, on(60));
        assert_eq!(out[1].time - out[0].time, MS, "the cycle must wrap at 32");
    }

    #[test]
    fn a_nearly_full_buffer_never_splits_a_pair() {
        use miditool_core::MAX_FANOUT;
        use std::sync::atomic::Ordering;

        let mut fx = Talea::new(&[100 * MS]);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let filler = EventKind::PitchBend { ch: 0, value: 0 };
        for _ in 0..MAX_FANOUT - 1 {
            out.push(Event::new(0, filler));
        }
        fx.process(&Event::new(0, on(60)), &mut out, &cx);
        assert_eq!(out.len(), MAX_FANOUT - 1);
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn poly_pressure_and_other_events_pass() {
        let mut fx = Talea::new(&[100 * MS]);
        run_timed(&mut fx, 0, on(60));
        let pressure = EventKind::PolyPressure {
            ch: 0,
            key: 60,
            value: 33,
        };
        assert_eq!(run_timed(&mut fx, 10, pressure), vec![at(10, pressure)]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 20, pedal), vec![at(20, pedal)]);
    }
}
