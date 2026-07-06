//! Xenakis Mists: planted keys wander the keyboard in Brownian steps.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx, Timestamp};
use rand_distr::{Distribution, Normal};

use crate::router::push;

/// How many walkers roam at once; the next one steals the oldest.
const MAX_WALKERS: usize = 8;

/// A very late tick takes at most this many catch-up steps per walker
/// before its schedule resynchronizes past `now`.
const MAX_CATCHUP: usize = 2;

/// Walkers never step faster than this.
const MIN_INTERVAL_NS: u64 = 20_000_000;

/// One roaming walker.
#[derive(Debug, Clone, Copy)]
struct Walker {
    ch: u8,
    /// The key the player planted; its note-off stops the walker.
    origin: u8,
    /// The key currently sounding.
    current: u8,
    vel: u8,
    next_due: Timestamp,
    /// Monotonic age stamp; the smallest is the oldest.
    seq: u64,
}

/// Fold a position into `lo..=hi` by reflecting off both walls.
fn reflect(pos: i32, lo: u8, hi: u8) -> u8 {
    let (lo, hi) = (lo as i32, hi as i32);
    let range = hi - lo;
    if range == 0 {
        return lo as u8;
    }
    let m = (pos - lo).rem_euclid(2 * range);
    let folded = if m > range { 2 * range - m } else { m };
    (lo + folded) as u8
}

/// The random walks of Xenakis' Mists, one per planted key. Each input
/// note-on is consumed and plants a walker (up to 8; a 9th steals the
/// oldest, releasing whatever it was sounding first): the walker sounds
/// the played key immediately at the played velocity, then every
/// `interval_ns` steps by `round(gaussian * sigma)` semitones, reflecting
/// off `lo` and `hi`, emitted legato as a note-off for the previous key
/// and a note-on for the new one, both stamped at the grid point. A key
/// planted outside `lo..=hi` starts where it was played and folds into
/// range on its first step. The player's note-off for a walker's origin
/// (channel, key) stops it with a note-off for its current key; note-offs
/// are consumed whether or not they match. All other events pass.
///
/// Ticks may be late: steps happen once `now` reaches their target and
/// are stamped with the target, not `now`. A very late tick takes at most
/// 2 catch-up steps per walker, then jumps that walker to the first grid
/// point past `now`; missed steps are dropped, not bunched.
///
/// Randomness comes from `rng::seeded`: the same seed and the same
/// sequence of events and ticks replay the same walks.
///
/// `flush` releases every walker's current key.
///
/// Fanout bound: `process` emits at most 2 events (a steal's release plus
/// the new key's on); a tick emits at most 8 walkers x 2 steps x 2
/// events, 32 in all; `flush` at most 8.
pub struct BrownianWalker {
    rng: Prng,
    step: Normal<f32>,
    interval_ns: u64,
    lo: u8,
    hi: u8,
    walkers: [Option<Walker>; MAX_WALKERS],
    next_seq: u64,
}

impl BrownianWalker {
    /// `interval_ns` is raised to at least 20ms, `sigma` is clamped to
    /// 0.5..=12.0 (panics on NaN), and `lo`/`hi` are clamped to 127 and
    /// ordered.
    pub fn new(seed: u64, interval_ns: u64, sigma: f32, lo: u8, hi: u8) -> Self {
        let (lo, hi) = (lo.min(127), hi.min(127));
        Self {
            rng: seeded(seed, 0),
            step: Normal::new(0.0, sigma.clamp(0.5, 12.0)).expect("sigma must not be NaN"),
            interval_ns: interval_ns.max(MIN_INTERVAL_NS),
            lo: lo.min(hi),
            hi: lo.max(hi),
            walkers: [None; MAX_WALKERS],
            next_seq: 0,
        }
    }

    /// A free slot, or the oldest occupied one to steal.
    fn slot(&self) -> usize {
        let mut oldest = 0;
        let mut oldest_seq = u64::MAX;
        for (i, w) in self.walkers.iter().enumerate() {
            match w {
                None => return i,
                Some(w) if w.seq < oldest_seq => {
                    oldest_seq = w.seq;
                    oldest = i;
                }
                _ => {}
            }
        }
        oldest
    }
}

impl Effect for BrownianWalker {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let slot = self.slot();
                if let Some(stolen) = self.walkers[slot] {
                    // Steal the oldest: release what it was sounding.
                    let cut = EventKind::NoteOff {
                        ch: stolen.ch,
                        key: stolen.current,
                        vel: 0,
                    };
                    push(out, cx, Event::new(ev.time, cut));
                }
                self.walkers[slot] = Some(Walker {
                    ch,
                    origin: key,
                    current: key,
                    vel,
                    next_due: ev.time + self.interval_ns,
                    seq: self.next_seq,
                });
                self.next_seq += 1;
                push(
                    out,
                    cx,
                    Event::new(ev.time, EventKind::NoteOn { ch, key, vel }),
                );
            }
            EventKind::NoteOff { ch, key, .. } => {
                // Stop every walker planted on that key; the event itself
                // is consumed either way.
                for slot in self.walkers.iter_mut() {
                    if let Some(w) = slot
                        && w.ch == ch
                        && w.origin == key
                    {
                        let cut = EventKind::NoteOff {
                            ch: w.ch,
                            key: w.current,
                            vel: 0,
                        };
                        push(out, cx, Event::new(ev.time, cut));
                        *slot = None;
                    }
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        let (lo, hi) = (self.lo, self.hi);
        for entry in self.walkers.iter_mut() {
            let Some(mut w) = *entry else { continue };
            let mut emitted = 0;
            while w.next_due <= now {
                if emitted == MAX_CATCHUP {
                    // Missed steps are dropped, not bunched: jump to the
                    // first grid point past now.
                    let missed = (now - w.next_due) / self.interval_ns + 1;
                    w.next_due += missed * self.interval_ns;
                    break;
                }
                let shift = self.step.sample(&mut self.rng).round() as i32;
                let next = reflect(w.current as i32 + shift, lo, hi);
                let off = EventKind::NoteOff {
                    ch: w.ch,
                    key: w.current,
                    vel: 0,
                };
                let on = EventKind::NoteOn {
                    ch: w.ch,
                    key: next,
                    vel: w.vel,
                };
                cx.push_pair(out, Event::new(w.next_due, off), Event::new(w.next_due, on));
                w.current = next;
                w.next_due += self.interval_ns;
                emitted += 1;
            }
            *entry = Some(w);
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        for slot in self.walkers.iter_mut() {
            if let Some(w) = slot.take() {
                let cut = EventKind::NoteOff {
                    ch: w.ch,
                    key: w.current,
                    vel: 0,
                };
                push(out, cx, Event::new(cx.now, cut));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, flush, off, on, run_timed, tick};

    const INTERVAL: u64 = 100_000_000;

    /// A walker box with a 100ms stride over the full keyboard.
    fn walkers(seed: u64) -> BrownianWalker {
        BrownianWalker::new(seed, INTERVAL, 3.0, 0, 127)
    }

    /// The key a walker currently sounds, read off its latest note-on.
    fn last_on_key(out: &[Event]) -> u8 {
        out.iter()
            .rev()
            .find_map(|ev| match ev.kind {
                EventKind::NoteOn { key, .. } => Some(key),
                _ => None,
            })
            .expect("a note-on")
    }

    #[test]
    fn a_note_on_sounds_immediately_and_is_consumed() {
        let mut fx = walkers(1);
        assert_eq!(run_timed(&mut fx, 5, on(60)), vec![at(5, on(60))]);
    }

    #[test]
    fn steps_are_legato_pairs_stamped_on_the_grid() {
        let mut fx = walkers(1);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, INTERVAL - 1), vec![]);
        let out = tick(&mut fx, INTERVAL);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], at(INTERVAL, off(60)), "off the previous key");
        assert!(
            matches!(
                out[1].kind,
                EventKind::NoteOn {
                    ch: 0,
                    vel: 100,
                    ..
                }
            ),
            "{out:?}"
        );
        assert_eq!(out[1].time, INTERVAL);
        // The next step releases exactly the key the walker moved to.
        let current = last_on_key(&out);
        let out = tick(&mut fx, 2 * INTERVAL);
        assert_eq!(out[0], at(2 * INTERVAL, off(current)));
    }

    #[test]
    fn keys_reflect_inside_the_walls() {
        let mut fx = BrownianWalker::new(3, INTERVAL, 12.0, 60, 62);
        run_timed(&mut fx, 0, on(60));
        for k in 1..=40 {
            for ev in tick(&mut fx, k * INTERVAL) {
                let key = ev.kind.key().unwrap();
                if matches!(ev.kind, EventKind::NoteOn { .. }) {
                    assert!((60..=62).contains(&key), "escaped to {key}");
                }
            }
        }
    }

    #[test]
    fn the_origin_off_stops_the_walker() {
        let mut fx = walkers(2);
        run_timed(&mut fx, 0, on(60));
        let mut current = 60;
        for k in 1..=3 {
            current = last_on_key(&tick(&mut fx, k * INTERVAL));
        }
        // The off names the origin; the release lands on the current key.
        assert_eq!(
            run_timed(&mut fx, 350_000_000, off(60)),
            vec![at(350_000_000, off(current))]
        );
        assert_eq!(tick(&mut fx, 10 * INTERVAL), vec![]);
    }

    #[test]
    fn offs_stop_only_the_matching_walker() {
        let mut fx = walkers(4);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 0, on(90));
        assert_eq!(tick(&mut fx, INTERVAL).len(), 4);
        run_timed(&mut fx, INTERVAL + 1, off(60));
        // Only the 90 walker keeps stepping.
        assert_eq!(tick(&mut fx, 2 * INTERVAL).len(), 2);
    }

    #[test]
    fn a_ninth_walker_steals_the_oldest() {
        let mut fx = walkers(5);
        for key in 10..18 {
            run_timed(&mut fx, 0, on(key));
        }
        // The steal releases the oldest walker's current key (still its
        // origin, no steps yet), then sounds the newcomer.
        assert_eq!(
            run_timed(&mut fx, 1, on(90)),
            vec![at(1, off(10)), at(1, on(90))]
        );
        // The stolen walker is gone: its origin off is consumed silently.
        assert_eq!(run_timed(&mut fx, 2, off(10)), vec![]);
    }

    #[test]
    fn note_offs_are_consumed_even_unmatched() {
        let mut fx = walkers(1);
        assert_eq!(run_timed(&mut fx, 0, off(99)), vec![]);
    }

    #[test]
    fn a_late_tick_takes_at_most_two_steps() {
        let mut fx = walkers(6);
        run_timed(&mut fx, 0, on(60));
        let out = tick(&mut fx, 20 * INTERVAL);
        assert_eq!(out.len(), 4, "two legato pairs");
        assert_eq!(out[0].time, INTERVAL);
        assert_eq!(out[2].time, 2 * INTERVAL);
        // Resynchronized: the next step lands on the grid past now.
        assert_eq!(tick(&mut fx, 20 * INTERVAL), vec![]);
        assert_eq!(tick(&mut fx, 21 * INTERVAL)[0].time, 21 * INTERVAL);
    }

    #[test]
    fn same_seed_same_walk() {
        let mut a = walkers(9);
        let mut b = walkers(9);
        for key in [60, 72] {
            assert_eq!(run_timed(&mut a, 0, on(key)), run_timed(&mut b, 0, on(key)));
        }
        for k in 1..20 {
            assert_eq!(tick(&mut a, k * INTERVAL), tick(&mut b, k * INTERVAL));
        }
    }

    #[test]
    fn flush_releases_every_current_key() {
        let mut fx = walkers(7);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 0, on(72));
        let out = tick(&mut fx, INTERVAL);
        let mut current: Vec<u8> = out
            .iter()
            .filter_map(|ev| match ev.kind {
                EventKind::NoteOn { key, .. } => Some(key),
                _ => None,
            })
            .collect();
        current.sort_unstable();
        let mut released: Vec<u8> = flush(&mut fx).iter().filter_map(|k| k.key()).collect();
        released.sort_unstable();
        assert_eq!(released, current);
        assert_eq!(flush(&mut fx), vec![]);
    }

    #[test]
    fn other_events_pass() {
        let mut fx = walkers(1);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 5, pedal), vec![at(5, pedal)]);
    }

    #[test]
    fn interval_clamps_to_twenty_ms() {
        let mut fx = BrownianWalker::new(1, 1, 3.0, 0, 127);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 19_999_999), vec![]);
        assert_eq!(tick(&mut fx, 20_000_000)[0].time, 20_000_000);
    }
}
