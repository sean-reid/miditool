//! Feldman-style restrikes: a note re-touched at long, slightly irregular
//! intervals with dying velocity until it fades below a floor.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;

use crate::router::push;

/// Re-touch every note-on at roughly `interval_ns` spacing with dying
/// velocity, the way a Feldman page keeps returning to the same sonority a
/// shade softer and a shade off the grid.
///
/// The original note-on passes unchanged. Restrike `k` (1, 2, ...) has
/// velocity `vel * decay^k`, rounded; the series stops as soon as that
/// falls below `floor` or `k` exceeds `max_repeats`. Its time is
/// cumulative: the k-th interval is `interval_ns` scaled by
/// `1 + jitter * u` with `u` drawn uniformly from [-1, 1]. Every restrike
/// is emitted up front as a self-contained pair: a note-on at its time plus
/// a matching note-off 60% of the base interval later, so pairs never
/// orphan regardless of what the player does. The player's own note-off
/// passes through and ends only the original note; all other events pass
/// through untouched.
///
/// Randomness is seeded and deterministic (`rng::seeded`). The draw
/// sequence is the only state: each emitted restrike advances the stream,
/// so a note's jitter depends on how many restrikes preceded it. For a
/// given seed and input sequence the output is fully reproducible.
///
/// Fanout bound: at most `1 + 2 * max_repeats` outputs per input, and
/// `max_repeats` is clamped to 24, so 49 events, well under `MAX_FANOUT`.
pub struct Restrike {
    rng: Prng,
    interval_ns: u64,
    jitter: f32,
    decay: f32,
    floor: u8,
    max_repeats: u8,
}

impl Restrike {
    /// `jitter` is clamped to 0.0..=0.9 (so intervals stay positive and
    /// times strictly increase), `max_repeats` to 1..=24.
    pub fn new(
        seed: u64,
        interval_ns: u64,
        jitter: f32,
        decay: f32,
        floor: u8,
        max_repeats: u8,
    ) -> Self {
        Self {
            rng: seeded(seed, 0),
            interval_ns,
            jitter: jitter.clamp(0.0, 0.9),
            decay,
            floor,
            max_repeats: max_repeats.clamp(1, 24),
        }
    }
}

impl Effect for Restrike {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        push(out, cx, *ev);
        let EventKind::NoteOn { ch, key, vel } = ev.kind else {
            return;
        };
        // 60% of the base interval, unaffected by jitter, so a restrike
        // always releases well before the next one lands.
        let hold = self.interval_ns.saturating_mul(3) / 5;
        let mut time = ev.time;
        for k in 1..=self.max_repeats {
            let v = (vel as f32 * self.decay.powi(k as i32)).round();
            if v < self.floor as f32 {
                break;
            }
            // Velocity 0 would read as a note-off on the wire; never
            // emit it.
            let vel = v.clamp(1.0, 127.0) as u8;
            let u: f64 = self.rng.random_range(-1.0..=1.0);
            let scale = 1.0 + self.jitter as f64 * u;
            time = time.saturating_add((self.interval_ns as f64 * scale).round() as u64);
            let strike = EventKind::NoteOn { ch, key, vel };
            push(out, cx, Event::new(time, strike));
            let release = EventKind::NoteOff { ch, key, vel: 0 };
            push(out, cx, Event::new(time.saturating_add(hold), release));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    fn on_vel(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    #[test]
    fn zero_jitter_lands_on_the_grid_and_stops_at_the_floor() {
        // vel 100, decay 0.5: 50, 25, 13 (12.5 rounds up), then 6 < 10.
        let mut fx = Restrike::new(1, 1_000, 0.0, 0.5, 10, 24);
        assert_eq!(
            run_timed(&mut fx, 5_000, on_vel(60, 100)),
            vec![
                at(5_000, on_vel(60, 100)),
                at(6_000, on_vel(60, 50)),
                at(6_600, off(60)),
                at(7_000, on_vel(60, 25)),
                at(7_600, off(60)),
                at(8_000, on_vel(60, 13)),
                at(8_600, off(60)),
            ]
        );
    }

    #[test]
    fn stops_at_max_repeats() {
        let mut fx = Restrike::new(1, 1_000, 0.0, 1.0, 1, 4);
        let out = run_timed(&mut fx, 0, on_vel(60, 100));
        assert_eq!(out.len(), 1 + 2 * 4);
        assert_eq!(out.last(), Some(&at(4_600, off(60))));
    }

    #[test]
    fn same_seed_same_input_same_output() {
        let mut a = Restrike::new(9, 1_000_000, 0.5, 0.7, 5, 24);
        let mut b = Restrike::new(9, 1_000_000, 0.5, 0.7, 5, 24);
        for (time, key) in [(0, 60), (10, 64), (20, 67)] {
            assert_eq!(
                run_timed(&mut a, time, on(key)),
                run_timed(&mut b, time, on(key))
            );
        }
    }

    #[test]
    fn jitter_keeps_times_strictly_increasing() {
        // Even with jitter over-asked (clamped to 0.9), every interval
        // scales by at least 0.1 of the base, so time always advances.
        let mut fx = Restrike::new(3, 1_000, 5.0, 1.0, 1, 24);
        let out = run_timed(&mut fx, 0, on_vel(60, 100));
        let ons: Vec<u64> = out
            .iter()
            .filter(|ev| matches!(ev.kind, EventKind::NoteOn { .. }))
            .map(|ev| ev.time)
            .collect();
        assert_eq!(ons.len(), 25);
        assert!(ons.windows(2).all(|w| w[0] < w[1]), "times: {ons:?}");
    }

    #[test]
    fn player_note_off_passes_through() {
        let mut fx = Restrike::new(1, 1_000, 0.0, 0.5, 10, 24);
        assert_eq!(run_timed(&mut fx, 123, off(60)), vec![at(123, off(60))]);
    }

    #[test]
    fn non_note_events_pass_through() {
        let mut fx = Restrike::new(1, 1_000, 0.0, 0.5, 10, 24);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 9, pedal), vec![at(9, pedal)]);
    }
}
