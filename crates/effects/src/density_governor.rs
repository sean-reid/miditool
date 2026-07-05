//! A statistical gate that thins the note stream toward a target rate.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::{NoteRouter, push};

/// How many recent onsets the governor remembers.
const RING: usize = 64;

/// Keep the note-on rate near `target_hz` by thinning: every incoming
/// note-on is recorded in a ring of the last 64 onsets, the rate over the
/// trailing `window_ns` is measured (onsets in the window divided by the
/// window length), and the note passes with probability
/// `min(1, target_hz / measured)`. With fewer than 2 onsets in the window
/// there is no rate to speak of and the note always passes; at or under
/// target everything passes and the rng never advances, so sparse playing
/// is untouched and only floods are thinned, deterministically per seed.
/// Rates above `64 / window` are measured against the newest 64 onsets
/// only.
///
/// Dropped note-ons swallow their note-offs through the router; note-offs
/// for passed notes pass, and orphan note-offs and poly pressure are
/// dropped, since the gate never ruled on them. Non-note events pass
/// unchanged. `flush` releases whatever still sounds and clears the
/// measurement.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// note-on), well under `MAX_FANOUT`.
pub struct DensityGovernor {
    target_hz: f32,
    window_ns: u64,
    onsets: [u64; RING],
    head: usize,
    len: usize,
    rng: Prng,
    router: NoteRouter,
}

impl DensityGovernor {
    /// `target_hz` is clamped to 0.1..=100.0 and `window_ns` to at least 1.
    pub fn new(seed: u64, target_hz: f32, window_ns: u64) -> Self {
        Self {
            target_hz: target_hz.clamp(0.1, 100.0),
            window_ns: window_ns.max(1),
            onsets: [0; RING],
            head: 0,
            len: 0,
            rng: seeded(seed, 0),
            router: NoteRouter::new(),
        }
    }

    /// Record an onset and decide whether it may pass.
    fn admit(&mut self, now: u64) -> bool {
        self.onsets[self.head] = now;
        self.head = (self.head + 1) % RING;
        self.len = (self.len + 1).min(RING);
        let count = self.onsets[..self.len]
            .iter()
            .filter(|&&t| now.saturating_sub(t) <= self.window_ns)
            .count();
        if count < 2 {
            return true;
        }
        let measured = count as f64 * 1e9 / self.window_ns as f64;
        let target = self.target_hz as f64;
        // The rng advances only when the stream is over target, so under
        // target the pass pattern is independent of the seed.
        measured <= target || self.rng.random::<f64>() < target / measured
    }
}

impl Effect for DensityGovernor {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { key, .. } => {
                let mapped = self.admit(ev.time).then_some(key);
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
        self.head = 0;
        self.len = 0;
        self.router.flush(out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{off, on, run, run_timed};

    /// Play a note-on and its note-off; true when the note passed. A
    /// dropped note-on must swallow its off.
    fn play(fx: &mut DensityGovernor, time: u64, key: u8) -> bool {
        let passed = match run_timed(fx, time, on(key))[..] {
            [ev] => {
                assert!(matches!(ev.kind, EventKind::NoteOn { .. }));
                true
            }
            [] => false,
            ref other => panic!("unexpected output {other:?}"),
        };
        let offs = run_timed(fx, time + 1, off(key));
        assert_eq!(!offs.is_empty(), passed, "off must match on");
        passed
    }

    #[test]
    fn under_target_everything_passes() {
        // 10 notes per second against a target of 100.
        let mut fx = DensityGovernor::new(1, 100.0, 1_000_000_000);
        for i in 0..50u64 {
            assert!(play(&mut fx, i * 100_000_000, 60), "note {i}");
        }
    }

    #[test]
    fn sparse_playing_passes_even_at_the_lowest_target() {
        // Notes 2s apart with a 1s window: never 2 onsets in the window,
        // so there is no measured rate and everything passes, target 0.1.
        let mut fx = DensityGovernor::new(1, 0.0, 1_000_000_000);
        for i in 0..20u64 {
            assert!(play(&mut fx, i * 2_000_000_000, 60), "note {i}");
        }
    }

    #[test]
    fn the_first_note_always_passes() {
        let mut fx = DensityGovernor::new(1, 0.0, u64::MAX);
        assert!(play(&mut fx, 0, 60));
    }

    #[test]
    fn a_flood_is_thinned_deterministically() {
        let pattern = |seed: u64| -> Vec<bool> {
            // 1000 notes per second against a target of 1.
            let mut fx = DensityGovernor::new(seed, 1.0, 1_000_000_000);
            (0..300u64)
                .map(|i| play(&mut fx, i * 1_000_000, 60))
                .collect()
        };
        let first = pattern(7);
        let passes = first.iter().filter(|&&p| p).count();
        assert!(first[0], "the first note must pass");
        assert!(passes < 100, "the flood must thin, passed {passes}");
        assert!(passes > 0, "some notes must survive");
        // The same seed replays the exact same gate pattern.
        assert_eq!(first, pattern(7));
    }

    #[test]
    fn the_rate_recovers_when_the_flood_ends() {
        let mut fx = DensityGovernor::new(3, 1.0, 1_000_000_000);
        for i in 0..100u64 {
            play(&mut fx, i * 1_000_000, 60);
        }
        // Two seconds of silence later the window is empty again.
        assert!(play(&mut fx, 3_000_000_000, 60));
    }

    #[test]
    fn retrigger_of_a_passed_note_cuts_first() {
        // Two onsets in a 1s window measure 2 Hz, well under target.
        let mut fx = DensityGovernor::new(1, 100.0, 1_000_000_000);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, on(60)), vec![off(60), on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = DensityGovernor::new(1, 100.0, 1_000_000_000);
        assert_eq!(run(&mut fx, off(60)), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = DensityGovernor::new(1, 0.0, u64::MAX);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal), vec![pedal]);
    }
}
