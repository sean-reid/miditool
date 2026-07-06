//! Ligeti's Poeme symphonique: every key winds up its own metronome.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx, Timestamp};
use rand::Rng;

use crate::router::push;

/// How many metronomes run at once; the next one steals the oldest.
const MAX_METROS: usize = 16;

/// A very late tick emits at most this many catch-up strikes per
/// metronome before its schedule resynchronizes past `now`.
const MAX_CATCHUP: usize = 2;

/// One wound-up metronome.
#[derive(Debug, Clone, Copy)]
struct Metro {
    ch: u8,
    key: u8,
    /// Velocity of the next strike, before rounding.
    vel: f32,
    period_ns: u64,
    next_due: Timestamp,
    /// Strikes still owed, counting the next one.
    remaining: u8,
    /// Monotonic age stamp; the smallest is the oldest.
    seq: u64,
}

/// Ligeti's Poeme symphonique for 100 metronomes, scaled down to 16. Each
/// input note-on is consumed and winds up an independent metronome on the
/// played key: its tempo is a seeded uniform draw in `bpm_lo..=bpm_hi`,
/// its first strike sounds immediately at the note-on's time, and every
/// strike is a self-contained pair, a note-on at the grid point and its
/// note-off 40% of the period later, so nothing the swarm starts ever
/// needs cancelling. Strike velocity begins at the played velocity and
/// scales by `fade` per repeat, never below 1. A metronome winds down
/// after `max_repeats` strikes or when the player's note-off for its
/// (channel, key) arrives, whichever comes first; note-offs are consumed
/// whether or not they match. A 17th metronome steals the oldest slot,
/// which is already silent between strikes, so the steal emits nothing.
/// All other events pass.
///
/// Ticks may be late: strikes fire once `now` reaches their target and
/// are stamped with the target, not `now`. A very late tick emits at most
/// 2 catch-up strikes per metronome, then jumps that metronome to the
/// first grid point past `now`; missed beats are dropped, not compressed,
/// and do not count against `max_repeats`.
///
/// `flush` clears the swarm without emitting: every strike already
/// carried its off.
///
/// Fanout bound: `process` emits at most one pair; a tick emits at most
/// 16 metronomes x 2 strikes x 2 events, 64 in all, under `MAX_FANOUT`.
pub struct MetronomeSwarm {
    rng: Prng,
    bpm_lo: f32,
    bpm_hi: f32,
    max_repeats: u8,
    fade: f32,
    metros: [Option<Metro>; MAX_METROS],
    next_seq: u64,
}

/// One strike: a pair, off at 40% of the period.
fn strike(out: &mut EventBuf, cx: &ProcCx, ch: u8, key: u8, vel: f32, at: Timestamp, period: u64) {
    let vel = vel.round().clamp(1.0, 127.0) as u8;
    let on = EventKind::NoteOn { ch, key, vel };
    let off = EventKind::NoteOff { ch, key, vel: 0 };
    cx.push_pair(
        out,
        Event::new(at, on),
        Event::new(at.saturating_add(period * 2 / 5), off),
    );
}

impl MetronomeSwarm {
    /// Tempo bounds are clamped to 20..=400 bpm and ordered, `max_repeats`
    /// to 1..=64, and `fade` to 0.5..=1.0.
    pub fn new(seed: u64, bpm_lo: f32, bpm_hi: f32, max_repeats: u8, fade: f32) -> Self {
        let (a, b) = (bpm_lo.clamp(20.0, 400.0), bpm_hi.clamp(20.0, 400.0));
        Self {
            rng: seeded(seed, 0),
            bpm_lo: a.min(b),
            bpm_hi: a.max(b),
            max_repeats: max_repeats.clamp(1, 64),
            fade: fade.clamp(0.5, 1.0),
            metros: [None; MAX_METROS],
            next_seq: 0,
        }
    }

    /// A free slot, or the oldest occupied one to steal.
    fn slot(&self) -> usize {
        let mut oldest = 0;
        let mut oldest_seq = u64::MAX;
        for (i, m) in self.metros.iter().enumerate() {
            match m {
                None => return i,
                Some(m) if m.seq < oldest_seq => {
                    oldest_seq = m.seq;
                    oldest = i;
                }
                _ => {}
            }
        }
        oldest
    }
}

impl Effect for MetronomeSwarm {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let bpm = self.rng.random_range(self.bpm_lo..=self.bpm_hi);
                let period_ns = (60_000_000_000.0 / bpm as f64) as u64;
                strike(out, cx, ch, key, vel as f32, ev.time, period_ns);
                if self.max_repeats > 1 {
                    let slot = self.slot();
                    self.metros[slot] = Some(Metro {
                        ch,
                        key,
                        vel: (vel as f32 * self.fade).max(1.0),
                        period_ns,
                        next_due: ev.time + period_ns,
                        remaining: self.max_repeats - 1,
                        seq: self.next_seq,
                    });
                    self.next_seq += 1;
                }
            }
            EventKind::NoteOff { ch, key, .. } => {
                // The player's off stops every metronome on that key; the
                // event itself is consumed either way.
                for slot in self.metros.iter_mut() {
                    if matches!(slot, Some(m) if m.ch == ch && m.key == key) {
                        *slot = None;
                    }
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        for entry in self.metros.iter_mut() {
            let Some(mut m) = *entry else { continue };
            let mut emitted = 0;
            let mut done = false;
            while m.next_due <= now && !done {
                if emitted == MAX_CATCHUP {
                    // Missed beats are dropped, not compressed: jump to
                    // the first grid point past now without spending
                    // strikes.
                    let missed = (now - m.next_due) / m.period_ns + 1;
                    m.next_due += missed * m.period_ns;
                    break;
                }
                strike(out, cx, m.ch, m.key, m.vel, m.next_due, m.period_ns);
                m.vel = (m.vel * self.fade).max(1.0);
                m.next_due += m.period_ns;
                m.remaining -= 1;
                emitted += 1;
                done = m.remaining == 0;
            }
            *entry = if done { None } else { Some(m) };
        }
    }

    fn flush(&mut self, _out: &mut EventBuf, _cx: &ProcCx) {
        // Every strike already carried its off; only the swarm clears.
        self.metros = [None; MAX_METROS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, flush, off, on, run_timed, tick};

    const PERIOD: u64 = 500_000_000;

    fn von(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    /// A swarm pinned to 120 bpm: every metronome ticks at 500ms.
    fn swarm(max_repeats: u8, fade: f32) -> MetronomeSwarm {
        MetronomeSwarm::new(1, 120.0, 120.0, max_repeats, fade)
    }

    fn strike_vels(out: &[Event]) -> Vec<u8> {
        out.iter()
            .filter_map(|ev| match ev.kind {
                EventKind::NoteOn { vel, .. } => Some(vel),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_note_on_strikes_immediately_and_is_consumed() {
        let mut fx = swarm(8, 1.0);
        // Only the strike pair comes out; the input note-on does not pass.
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![at(0, on(60)), at(200_000_000, off(60))]
        );
    }

    #[test]
    fn strikes_repeat_on_the_drawn_grid() {
        let mut fx = swarm(8, 1.0);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, PERIOD - 1), vec![]);
        assert_eq!(
            tick(&mut fx, PERIOD),
            vec![at(PERIOD, on(60)), at(PERIOD + 200_000_000, off(60))]
        );
        assert_eq!(tick(&mut fx, 2 * PERIOD)[0].time, 2 * PERIOD);
    }

    #[test]
    fn velocity_fades_with_a_floor_of_one() {
        let mut fx = swarm(64, 0.5);
        let mut vels = strike_vels(&run_timed(&mut fx, 0, on(60)));
        for k in 1..12 {
            vels.extend(strike_vels(&tick(&mut fx, k * PERIOD)));
        }
        assert_eq!(&vels[..4], &[100, 50, 25, 13]);
        assert_eq!(*vels.last().unwrap(), 1, "the fade floors at 1");
    }

    #[test]
    fn stops_after_max_repeats() {
        let mut fx = swarm(3, 1.0);
        assert_eq!(run_timed(&mut fx, 0, on(60)).len(), 2);
        assert_eq!(tick(&mut fx, PERIOD).len(), 2);
        assert_eq!(tick(&mut fx, 2 * PERIOD).len(), 2);
        assert_eq!(tick(&mut fx, 3 * PERIOD), vec![]);
        assert_eq!(tick(&mut fx, 30 * PERIOD), vec![]);
    }

    #[test]
    fn the_players_off_stops_its_metronome_and_is_consumed() {
        let mut fx = swarm(64, 1.0);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 0, on(64));
        assert_eq!(run_timed(&mut fx, 250_000_000, off(60)), vec![]);
        // Only the 64 metronome survives.
        assert_eq!(
            tick(&mut fx, PERIOD),
            vec![at(PERIOD, on(64)), at(PERIOD + 200_000_000, off(64))]
        );
        // An off with no metronome is consumed the same way.
        assert_eq!(run_timed(&mut fx, PERIOD + 1, off(99)), vec![]);
    }

    #[test]
    fn a_seventeenth_metronome_steals_the_oldest() {
        let mut fx = swarm(64, 1.0);
        for key in 0..16 {
            run_timed(&mut fx, 0, on(key));
        }
        // The steal itself is silent: only the newcomer's strike sounds.
        assert_eq!(
            run_timed(&mut fx, 1, on(100)),
            vec![at(1, on(100)), at(1 + 200_000_000, off(100))]
        );
        let keys: Vec<u8> = tick(&mut fx, PERIOD + 1)
            .iter()
            .filter_map(|ev| match ev.kind {
                EventKind::NoteOn { key, .. } => Some(key),
                _ => None,
            })
            .collect();
        assert_eq!(keys.len(), 16);
        assert!(!keys.contains(&0), "the oldest kept ticking: {keys:?}");
        assert!(keys.contains(&100));
    }

    #[test]
    fn tempo_draws_stay_in_range() {
        for seed in 0..20 {
            let mut fx = MetronomeSwarm::new(seed, 100.0, 200.0, 8, 1.0);
            run_timed(&mut fx, 0, on(60));
            let out = tick(&mut fx, 700_000_000);
            let period = out[0].time;
            // 200 bpm is 300ms, 100 bpm is 600ms.
            assert!(
                (300_000_000..=600_000_000).contains(&period),
                "seed {seed}: {period}"
            );
        }
    }

    #[test]
    fn a_late_tick_emits_at_most_two_strikes_then_resyncs() {
        let mut fx = swarm(64, 1.0);
        run_timed(&mut fx, 0, on(60));
        // Ten periods late: two strikes on the old grid, then a jump.
        let out = tick(&mut fx, 10 * PERIOD);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].time, PERIOD);
        assert_eq!(out[2].time, 2 * PERIOD);
        assert_eq!(tick(&mut fx, 10 * PERIOD), vec![]);
        assert_eq!(tick(&mut fx, 11 * PERIOD)[0].time, 11 * PERIOD);
    }

    #[test]
    fn same_seed_same_swarm() {
        let mut a = MetronomeSwarm::new(9, 60.0, 240.0, 16, 0.8);
        let mut b = MetronomeSwarm::new(9, 60.0, 240.0, 16, 0.8);
        for key in [60, 64, 67] {
            assert_eq!(run_timed(&mut a, 0, on(key)), run_timed(&mut b, 0, on(key)));
        }
        for k in 1..20 {
            let now = k * 100_000_000;
            assert_eq!(tick(&mut a, now), tick(&mut b, now));
        }
    }

    #[test]
    fn parameters_clamp() {
        // max_repeats 0 clamps to 1: a single strike, then nothing.
        let mut fx = MetronomeSwarm::new(1, 120.0, 120.0, 0, 2.0);
        assert_eq!(run_timed(&mut fx, 0, on(60)).len(), 2);
        assert_eq!(tick(&mut fx, 100 * PERIOD), vec![]);
        // 1 bpm clamps to 20 (3s period), fade 0 to 0.5.
        let mut fx = MetronomeSwarm::new(1, 1.0, 1.0, 4, 0.0);
        run_timed(&mut fx, 0, von(60, 100));
        let out = tick(&mut fx, 3_000_000_000);
        assert_eq!(out[0], at(3_000_000_000, von(60, 50)));
    }

    #[test]
    fn other_events_pass() {
        let mut fx = swarm(8, 1.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 5, pedal), vec![at(5, pedal)]);
    }

    #[test]
    fn flush_clears_the_swarm() {
        let mut fx = swarm(64, 1.0);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(flush(&mut fx), vec![]);
        assert_eq!(tick(&mut fx, 10 * PERIOD), vec![]);
    }
}
