//! Ligeti's mechanico looms: latched keys re-struck by a jamming machine.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx, Timestamp};
use rand::Rng;

use crate::router::push;

/// How many keys the loom holds; the next one evicts the oldest.
const MAX_LATCHED: usize = 12;

/// A very late tick runs at most this many catch-up pulses before the
/// clock resynchronizes past `now`.
const MAX_CATCHUP: usize = 2;

/// The loom never pulses faster than this.
const MIN_PULSE_NS: u64 = 50_000_000;

/// One latched key.
#[derive(Debug, Clone, Copy)]
struct Latched {
    ch: u8,
    key: u8,
    vel: u8,
    /// Strikes still owed, counting the next one.
    remaining: u8,
    /// Monotonic age stamp; the smallest is the oldest.
    seq: u64,
}

/// The obsessive machine rhythms of Ligeti's mechanico writing. Each
/// input note-on is consumed and latched into the loom (up to 12 keys; a
/// 13th evicts the oldest, silently, because every strike already carried
/// its note-off): from then on the whole loom is re-struck together every
/// `pulse_ns`, each strike a self-contained pair with its off at 50% of
/// the pulse, and each key dies after `repeats` strikes. Re-latching a
/// held key resets its count, velocity, and age. The first pulse lands at
/// the note-on that woke the empty loom; later keys join at the next
/// pulse. With probability `jam` per pulse the loom stutters: that pulse
/// is skipped in silence and the next one comes 50% early, at half the
/// pulse instead of a whole one. Player note-offs are consumed and
/// ignored, the loom owns all durations; every other event passes.
///
/// Ticks may be late: pulses fire once `now` reaches their target and
/// are stamped with the target, not `now`. A very late tick runs at most
/// 2 catch-up pulses, then jumps the clock to a whole-pulse grid point
/// past `now`; skipped pulses draw no jam coin and spend no strikes.
///
/// The jam coin comes from `rng::seeded`, one draw per executed pulse, so
/// the same seed and the same event and tick sequence replay the same
/// stutters.
///
/// `flush` clears the loom without emitting: nothing sounds between
/// strikes.
///
/// Fanout bound: `process` emits at most the one passed-through event; a
/// tick emits at most 2 pulses x 12 keys x 2 events, 48 in all.
pub struct Mechanico {
    pulse_ns: u64,
    repeats: u8,
    jam: f32,
    rng: Prng,
    keys: [Option<Latched>; MAX_LATCHED],
    /// The next pulse; `Some` exactly while the loom is running.
    next_due: Option<Timestamp>,
    next_seq: u64,
}

impl Mechanico {
    /// `pulse_ns` is raised to at least 50ms, `repeats` is clamped to
    /// 1..=64, and `jam` to 0.0..=0.5.
    pub fn new(pulse_ns: u64, repeats: u8, jam: f32, seed: u64) -> Self {
        Self {
            pulse_ns: pulse_ns.max(MIN_PULSE_NS),
            repeats: repeats.clamp(1, 64),
            jam: jam.clamp(0.0, 0.5),
            rng: seeded(seed, 0),
            keys: [None; MAX_LATCHED],
            next_due: None,
            next_seq: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.keys.iter().all(Option::is_none)
    }

    /// A free slot, or the oldest occupied one to evict.
    fn slot(&self) -> usize {
        let mut oldest = 0;
        let mut oldest_seq = u64::MAX;
        for (i, l) in self.keys.iter().enumerate() {
            match l {
                None => return i,
                Some(l) if l.seq < oldest_seq => {
                    oldest_seq = l.seq;
                    oldest = i;
                }
                _ => {}
            }
        }
        oldest
    }
}

impl Effect for Mechanico {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let seq = self.next_seq;
                self.next_seq += 1;
                if let Some(l) = self
                    .keys
                    .iter_mut()
                    .flatten()
                    .find(|l| l.ch == ch && l.key == key)
                {
                    l.vel = vel;
                    l.remaining = self.repeats;
                    l.seq = seq;
                } else {
                    let was_empty = self.is_empty();
                    let slot = self.slot();
                    self.keys[slot] = Some(Latched {
                        ch,
                        key,
                        vel,
                        remaining: self.repeats,
                        seq,
                    });
                    if was_empty {
                        self.next_due = Some(ev.time);
                    }
                }
            }
            // The loom owns durations; the player's note-off is consumed.
            EventKind::NoteOff { .. } => {}
            _ => push(out, cx, *ev),
        }
    }

    fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        let mut pulses = 0;
        while let Some(due) = self.next_due {
            if due > now {
                break;
            }
            if self.is_empty() {
                self.next_due = None;
                break;
            }
            if pulses == MAX_CATCHUP {
                // Missed pulses are dropped, not bunched: jump to a
                // whole-pulse grid point past now.
                let missed = (now - due) / self.pulse_ns + 1;
                self.next_due = Some(due + missed * self.pulse_ns);
                break;
            }
            pulses += 1;
            if self.rng.random::<f32>() < self.jam {
                // The stutter: this pulse is silent, the next comes early.
                self.next_due = Some(due + self.pulse_ns / 2);
                continue;
            }
            for entry in self.keys.iter_mut() {
                let Some(mut l) = *entry else { continue };
                let on = EventKind::NoteOn {
                    ch: l.ch,
                    key: l.key,
                    vel: l.vel,
                };
                let off = EventKind::NoteOff {
                    ch: l.ch,
                    key: l.key,
                    vel: 0,
                };
                cx.push_pair(
                    out,
                    Event::new(due, on),
                    Event::new(due.saturating_add(self.pulse_ns / 2), off),
                );
                l.remaining -= 1;
                *entry = if l.remaining == 0 { None } else { Some(l) };
            }
            self.next_due = Some(due + self.pulse_ns);
        }
    }

    fn flush(&mut self, _out: &mut EventBuf, _cx: &ProcCx) {
        // Nothing sounds between strikes; only the loom clears.
        self.keys = [None; MAX_LATCHED];
        self.next_due = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, flush, off, on, run_timed, tick};

    const PULSE: u64 = 100_000_000;

    /// A steady loom: 100ms pulse, no jamming.
    fn loom(repeats: u8) -> Mechanico {
        Mechanico::new(PULSE, repeats, 0.0, 1)
    }

    fn strike_keys(out: &[Event]) -> Vec<u8> {
        out.iter()
            .filter_map(|ev| match ev.kind {
                EventKind::NoteOn { key, .. } => Some(key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn latching_is_consumed_and_the_first_pulse_lands_at_latch_time() {
        let mut fx = loom(8);
        assert_eq!(run_timed(&mut fx, 1_000, on(60)), vec![]);
        assert_eq!(
            tick(&mut fx, 1_000),
            vec![at(1_000, on(60)), at(1_000 + PULSE / 2, off(60))]
        );
        assert_eq!(tick(&mut fx, 1_000), vec![]);
    }

    #[test]
    fn keys_join_on_the_next_pulse_and_strike_together() {
        let mut fx = loom(8);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(strike_keys(&tick(&mut fx, 0)), vec![60]);
        // A key latched mid-cycle waits for the next pulse.
        run_timed(&mut fx, 10_000_000, on(64));
        assert_eq!(tick(&mut fx, 50_000_000), vec![]);
        let out = tick(&mut fx, PULSE);
        assert_eq!(strike_keys(&out), vec![60, 64]);
        assert!(
            out.iter()
                .all(|ev| ev.time == PULSE || ev.time == PULSE + PULSE / 2)
        );
    }

    #[test]
    fn keys_die_after_their_repeats_and_the_loom_stops() {
        let mut fx = loom(2);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 0).len(), 2);
        assert_eq!(tick(&mut fx, PULSE).len(), 2);
        assert_eq!(tick(&mut fx, 2 * PULSE), vec![]);
        assert_eq!(tick(&mut fx, 20 * PULSE), vec![]);
        // A fresh latch restarts the clock at its own arrival.
        run_timed(&mut fx, 25 * PULSE + 3, on(64));
        assert_eq!(tick(&mut fx, 25 * PULSE + 3)[0].time, 25 * PULSE + 3);
    }

    #[test]
    fn a_jam_skips_the_pulse_and_pulls_the_next_one_early() {
        // A seed whose first coin jams and whose second does not.
        let seed = (0..1_000u64)
            .find(|&s| {
                let mut rng = seeded(s, 0);
                let a: f32 = rng.random();
                let b: f32 = rng.random();
                a < 0.5 && b >= 0.5
            })
            .expect("such a seed exists");
        let mut fx = Mechanico::new(PULSE, 8, 0.5, seed);
        run_timed(&mut fx, 0, on(60));
        // The first pulse is silent; the next comes half a pulse later.
        assert_eq!(tick(&mut fx, 0), vec![]);
        assert_eq!(tick(&mut fx, PULSE / 2 - 1), vec![]);
        let out = tick(&mut fx, PULSE / 2);
        assert_eq!(out[0].time, PULSE / 2);
    }

    #[test]
    fn relatching_resets_a_keys_count() {
        let mut fx = loom(2);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 0).len(), 2);
        // Re-latch after the first strike: two more strikes follow.
        run_timed(&mut fx, 50_000_000, on(60));
        assert_eq!(tick(&mut fx, PULSE).len(), 2);
        assert_eq!(tick(&mut fx, 2 * PULSE).len(), 2);
        assert_eq!(tick(&mut fx, 3 * PULSE), vec![]);
    }

    #[test]
    fn a_thirteenth_key_evicts_the_oldest_silently() {
        let mut fx = loom(64);
        for key in 0..12 {
            run_timed(&mut fx, 0, on(key));
        }
        assert_eq!(strike_keys(&tick(&mut fx, 0)).len(), 12);
        assert_eq!(run_timed(&mut fx, 1, on(50)), vec![]);
        let keys = strike_keys(&tick(&mut fx, PULSE));
        assert_eq!(keys.len(), 12);
        assert!(!keys.contains(&0), "the oldest survived: {keys:?}");
        assert!(keys.contains(&50));
    }

    #[test]
    fn player_offs_are_consumed_and_ignored() {
        let mut fx = loom(8);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 0).len(), 2);
        assert_eq!(run_timed(&mut fx, 10_000_000, off(60)), vec![]);
        // The key keeps striking regardless.
        assert_eq!(strike_keys(&tick(&mut fx, PULSE)), vec![60]);
    }

    #[test]
    fn a_late_tick_runs_at_most_two_pulses() {
        let mut fx = loom(64);
        run_timed(&mut fx, 0, on(60));
        let out = tick(&mut fx, 10 * PULSE);
        assert_eq!(out.len(), 4, "two pulses of one key");
        assert_eq!(out[0].time, 0);
        assert_eq!(out[2].time, PULSE);
        // Resynchronized onto the whole-pulse grid past now.
        assert_eq!(tick(&mut fx, 10 * PULSE), vec![]);
        assert_eq!(tick(&mut fx, 11 * PULSE)[0].time, 11 * PULSE);
    }

    #[test]
    fn same_seed_same_stutters() {
        let mut a = Mechanico::new(PULSE, 16, 0.5, 42);
        let mut b = Mechanico::new(PULSE, 16, 0.5, 42);
        for key in [60, 64] {
            assert_eq!(run_timed(&mut a, 0, on(key)), run_timed(&mut b, 0, on(key)));
        }
        for k in 0..40 {
            let now = k * PULSE / 2;
            assert_eq!(tick(&mut a, now), tick(&mut b, now));
        }
    }

    #[test]
    fn parameters_clamp() {
        // Pulse 1 raises to 50ms; repeats 0 to 1: one strike, off at 25ms.
        let mut fx = Mechanico::new(1, 0, 0.0, 1);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(
            tick(&mut fx, 0),
            vec![at(0, on(60)), at(25_000_000, off(60))]
        );
        assert_eq!(tick(&mut fx, 1_000_000_000), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = loom(8);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 5, pedal), vec![at(5, pedal)]);
    }

    #[test]
    fn flush_clears_the_loom() {
        let mut fx = loom(64);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 0).len(), 2);
        assert_eq!(flush(&mut fx), vec![]);
        assert_eq!(tick(&mut fx, 10 * PULSE), vec![]);
    }
}
