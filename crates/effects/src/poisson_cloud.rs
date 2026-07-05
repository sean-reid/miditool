//! Xenakis clouds: every note-on scatters a decaying swarm of grains.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx};
use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::router::push;

/// A grain never sounds longer than this, whatever the next arrival says.
const MAX_GRAIN_HOLD_NS: u64 = 120_000_000;

/// Scatter a decaying grain cloud behind every note-on, the stochastic
/// texture of Xenakis' granular thinking reduced to note pairs. The played
/// note passes unchanged (its note-off will find it), then grain onsets
/// follow a Poisson process: exponential inter-arrival times at
/// `density_hz`, thinned linearly to zero across `duration_ns` so the
/// cloud dies out. Arrivals are drawn until one lands past `duration_ns`
/// or `max_grains` of them have been drawn; an arrival at time `t` into
/// the cloud survives with probability `1 - t / duration_ns`.
///
/// Each surviving grain is a self-contained pair via `push_pair`: a
/// note-on at the arrival time and a matching note-off at the next
/// inter-arrival gap later, capped at 120ms, so grains barely overlap and
/// never orphan. Grain pitch is the input key plus
/// `round(gaussian * pitch_sigma)`; a grain that leaves 0..=127 is dropped
/// whole, on and off together. Grain velocity is the input velocity
/// scaled by `1 - t / duration_ns` plus `gaussian * vel_sigma`, rounded
/// and clamped to 1..=127.
///
/// Randomness is seeded and deterministic (`rng::seeded`). Per arrival
/// the draws are gap, survival coin, then pitch and velocity for every
/// survivor, even one dropped for leaving the keyboard, so a dropped
/// grain never shifts its neighbors' draws. The same seed and input
/// replay the same cloud. Note-offs and all other events pass through
/// untouched.
///
/// Fanout bound: at most `1 + 2 * max_grains` outputs per input, and
/// `max_grains` is clamped to 24, so 49 events, well under `MAX_FANOUT`.
pub struct PoissonCloud {
    rng: Prng,
    /// Mean inter-arrival gap in nanoseconds, `1e9 / density_hz`.
    mean_gap_ns: f64,
    duration_ns: u64,
    pitch: Normal<f32>,
    vel: Normal<f32>,
    max_grains: u8,
}

impl PoissonCloud {
    /// `density_hz` is clamped to 0.001..=10_000.0, `duration_ns` to at
    /// least 1, both sigmas to 0.0..=127.0, and `max_grains` to 1..=24.
    /// Panics if a sigma is NaN.
    pub fn new(
        seed: u64,
        density_hz: f32,
        duration_ns: u64,
        pitch_sigma: f32,
        vel_sigma: f32,
        max_grains: u8,
    ) -> Self {
        let normal =
            |sigma: f32| Normal::new(0.0, sigma.clamp(0.0, 127.0)).expect("sigma must not be NaN");
        Self {
            rng: seeded(seed, 0),
            mean_gap_ns: 1e9 / density_hz.clamp(0.001, 10_000.0) as f64,
            duration_ns: duration_ns.max(1),
            pitch: normal(pitch_sigma),
            vel: normal(vel_sigma),
            max_grains: max_grains.clamp(1, 24),
        }
    }

    /// One exponential inter-arrival gap, rounded to whole nanoseconds.
    fn gap(&mut self) -> u64 {
        let u: f64 = self.rng.random();
        (-(1.0 - u).ln() * self.mean_gap_ns).round() as u64
    }
}

impl Effect for PoissonCloud {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        push(out, cx, *ev);
        let EventKind::NoteOn { ch, key, vel } = ev.kind else {
            return;
        };
        let mut t = self.gap();
        for _ in 0..self.max_grains {
            if t > self.duration_ns {
                break;
            }
            let next = self.gap();
            let frac = t as f64 / self.duration_ns as f64;
            if self.rng.random::<f64>() < 1.0 - frac {
                let shift = self.pitch.sample(&mut self.rng).round() as i32;
                let noise = self.vel.sample(&mut self.rng);
                let scaled = vel as f32 * (1.0 - frac as f32);
                let grain_key = key as i32 + shift;
                if (0..=127).contains(&grain_key) {
                    let key = grain_key as u8;
                    // Velocity 0 would read as a note-off on the wire;
                    // never emit it.
                    let vel = (scaled + noise).round().clamp(1.0, 127.0) as u8;
                    let at = ev.time.saturating_add(t);
                    let strike = EventKind::NoteOn { ch, key, vel };
                    let release = EventKind::NoteOff { ch, key, vel: 0 };
                    // Pushed as a pair so truncation can never keep the on
                    // and drop the off, which would leave the grain stuck.
                    cx.push_pair(
                        out,
                        Event::new(at, strike),
                        Event::new(at.saturating_add(next.min(MAX_GRAIN_HOLD_NS)), release),
                    );
                }
            }
            t = t.saturating_add(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    /// A dense, long-lived cloud: plenty of grains for every seed.
    fn cloud(seed: u64, pitch_sigma: f32, vel_sigma: f32) -> PoissonCloud {
        PoissonCloud::new(seed, 100.0, 1_000_000_000, pitch_sigma, vel_sigma, 24)
    }

    fn grain_ons(out: &[Event]) -> Vec<Event> {
        out[1..]
            .iter()
            .filter(|ev| matches!(ev.kind, EventKind::NoteOn { .. }))
            .copied()
            .collect()
    }

    #[test]
    fn the_played_note_and_its_off_pass_through() {
        let mut fx = cloud(1, 3.0, 10.0);
        let out = run_timed(&mut fx, 5_000, on(60));
        assert_eq!(out[0], at(5_000, on(60)));
        // The player's note-off passes alone: the grains bring their own.
        assert_eq!(run_timed(&mut fx, 9_000, off(60)), vec![at(9_000, off(60))]);
    }

    #[test]
    fn grains_come_as_balanced_pairs_within_bounds() {
        for seed in 0..20 {
            let mut fx = cloud(seed, 3.0, 10.0);
            let out = run_timed(&mut fx, 0, on(60));
            assert!(out.len() <= 1 + 2 * 24, "seed {seed}: {}", out.len());
            assert_eq!(out.len() % 2, 1, "seed {seed}: grains must pair up");
            let net: i32 = out
                .iter()
                .map(|ev| match ev.kind {
                    EventKind::NoteOn { .. } => 1,
                    EventKind::NoteOff { .. } => -1,
                    _ => 0,
                })
                .sum();
            assert_eq!(net, 1, "seed {seed}: only the original on is open");
        }
    }

    #[test]
    fn grain_offs_release_within_the_cap() {
        let mut fx = cloud(3, 0.0, 0.0);
        let out = run_timed(&mut fx, 0, on(60));
        for pair in out[1..].chunks(2) {
            let [on_ev, off_ev] = pair else {
                panic!("grains must pair up, got {pair:?}");
            };
            assert!(matches!(on_ev.kind, EventKind::NoteOn { .. }));
            assert!(matches!(off_ev.kind, EventKind::NoteOff { .. }));
            assert_eq!(on_ev.kind.key(), off_ev.kind.key());
            assert!(off_ev.time >= on_ev.time);
            assert!(off_ev.time - on_ev.time <= MAX_GRAIN_HOLD_NS);
        }
    }

    #[test]
    fn grain_onsets_stay_inside_the_duration() {
        let mut fx = PoissonCloud::new(5, 1_000.0, 50_000_000, 0.0, 0.0, 24);
        let out = run_timed(&mut fx, 7_000, on(60));
        for ev in grain_ons(&out) {
            assert!(ev.time <= 7_000 + 50_000_000, "grain at {}", ev.time);
        }
    }

    #[test]
    fn zero_pitch_sigma_keeps_grains_on_the_played_key() {
        let mut fx = cloud(2, 0.0, 10.0);
        let out = run_timed(&mut fx, 0, on(64));
        assert!(out.len() > 1, "the cloud must produce grains");
        for ev in &out[1..] {
            assert_eq!(ev.kind.key(), Some(64));
        }
    }

    #[test]
    fn grain_velocity_decays_across_the_cloud() {
        let mut fx = cloud(4, 0.0, 0.0);
        let out = run_timed(&mut fx, 0, on(60));
        let vels: Vec<u8> = grain_ons(&out)
            .iter()
            .map(|ev| match ev.kind {
                EventKind::NoteOn { vel, .. } => vel,
                other => panic!("expected a note-on, got {other:?}"),
            })
            .collect();
        assert!(vels.len() > 1, "the cloud must produce grains");
        assert!(vels.windows(2).all(|w| w[0] >= w[1]), "vels: {vels:?}");
        assert!(vels.iter().all(|&v| v >= 1));
    }

    #[test]
    fn out_of_range_grains_drop_whole_without_shifting_the_rest() {
        // A dropped grain consumed the same draws as a kept one, so the
        // wild-sigma run keeps a subset of the tame run's grain times.
        let mut kept = 0usize;
        let mut wild_kept = 0usize;
        for seed in 0..10 {
            let mut tame = cloud(seed, 0.0, 0.0);
            let mut wild = cloud(seed, 127.0, 0.0);
            let tame_out = run_timed(&mut tame, 0, on(0));
            let wild_out = run_timed(&mut wild, 0, on(0));
            let tame_times: Vec<u64> = grain_ons(&tame_out).iter().map(|ev| ev.time).collect();
            let wild_times: Vec<u64> = grain_ons(&wild_out).iter().map(|ev| ev.time).collect();
            for time in &wild_times {
                assert!(tame_times.contains(time), "seed {seed}: grain at {time}");
            }
            kept += tame_times.len();
            wild_kept += wild_times.len();
        }
        // From key 0 roughly half the wild draws fall below the keyboard.
        assert!(wild_kept < kept, "{wild_kept} vs {kept}");
    }

    #[test]
    fn same_seed_same_input_same_output() {
        let mut a = cloud(9, 5.0, 20.0);
        let mut b = cloud(9, 5.0, 20.0);
        for (time, key) in [(0, 60), (1_000, 64), (2_000, 67)] {
            assert_eq!(
                run_timed(&mut a, time, on(key)),
                run_timed(&mut b, time, on(key))
            );
        }
    }

    #[test]
    fn max_grains_clamps() {
        let mut fx = PoissonCloud::new(1, 10_000.0, u64::MAX, 0.0, 0.0, u8::MAX);
        let out = run_timed(&mut fx, 0, on(60));
        assert!(out.len() <= 1 + 2 * 24, "{}", out.len());
        let mut fx = PoissonCloud::new(1, 10_000.0, u64::MAX, 0.0, 0.0, 0);
        let out = run_timed(&mut fx, 0, on(60));
        assert!(out.len() <= 3, "{}", out.len());
    }

    #[test]
    fn a_nearly_full_buffer_never_splits_a_pair() {
        use miditool_core::MAX_FANOUT;

        let mut fx = PoissonCloud::new(1, 10_000.0, u64::MAX, 0.0, 0.0, 24);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        // Three slots left: the pass-through on, one whole pair, and one
        // slot that must not receive a lone grain on.
        let filler = EventKind::PitchBend { ch: 0, value: 0 };
        for _ in 0..MAX_FANOUT - 4 {
            out.push(Event::new(0, filler));
        }
        fx.process(&Event::new(0, on(60)), &mut out, &cx);
        let net: i32 = out
            .iter()
            .map(|ev| match ev.kind {
                EventKind::NoteOn { .. } => 1,
                EventKind::NoteOff { .. } => -1,
                _ => 0,
            })
            .sum();
        assert_eq!(net, 1, "only the original on awaits the player's off");
    }

    #[test]
    fn non_note_events_pass_through() {
        let mut fx = cloud(1, 3.0, 10.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 9, pedal), vec![at(9, pedal)]);
    }
}
