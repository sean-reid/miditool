//! Ligeti's Continuum: held keys dissolve into a fast mechanical cycle.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx, Timestamp};
use rand::Rng;

use crate::router::push;

/// How many held keys the machine tracks; further note-ons are consumed
/// but ignored until a slot frees up.
const MAX_HELD: usize = 16;

/// A very late tick emits at most this many catch-up pairs before the
/// cycle resynchronizes past `now`.
const MAX_CATCHUP: usize = 4;

/// The order in which the cycle walks the held set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuumOrder {
    /// Ascending by key, wrapping at the top.
    Up,
    /// Descending by key, wrapping at the bottom.
    Down,
    /// Order of arrival.
    Played,
    /// A seeded draw per emission, never repeating the previous key while
    /// more than one key is held.
    Random,
}

/// One held key: channel, key, and the velocity the player used.
#[derive(Debug, Clone, Copy, Default)]
struct Held {
    ch: u8,
    key: u8,
    vel: u8,
}

/// Ligeti's Continuum turned into a machine the player steers. Note-ons
/// and note-offs maintain a held set of up to 16 keys and are consumed,
/// since the cycle replaces direct sounding; every other event passes.
/// While the set is non-empty, `tick` emits the next key per `order`
/// every `1 / rate_hz` seconds as a self-contained pair: a note-on
/// stamped on the grid and its note-off `gate / rate_hz` later, carrying
/// the velocity the player used for that key. The pair's off is emitted
/// with its on and is never cancelled early, so even when the set empties
/// mid-gate nothing sticks. The first emission lands at the note-on that
/// woke the empty set; when the set empties, the cycle stops.
///
/// A retriggered held key updates its velocity in place. Random order
/// avoids repeating the previous key (compared by key) whenever more than
/// one key is held, drawing through `rng::seeded`, so the same seed
/// replays the same walk.
///
/// Ticks may be late or early: emission happens once `now` reaches the
/// target timestamp and is stamped with the target, not `now`, so musical
/// time stays even. A very late tick catches up at most 4 pairs, then
/// jumps the schedule to the first grid point past `now`, dropping the
/// missed periods instead of bunching them.
///
/// `flush` only clears the machine: every note it started already carries
/// its note-off.
///
/// Fanout bound: at most 4 pairs, 8 events, per tick call; `process`
/// emits at most the one passed-through event.
pub struct Continuum {
    period_ns: u64,
    gate_ns: u64,
    order: ContinuumOrder,
    rng: Prng,
    held: [Held; MAX_HELD],
    len: usize,
    /// The next grid point; `Some` exactly while the set is non-empty.
    next_due: Option<Timestamp>,
    /// The key of the previous emission, steering Up, Down, and Random.
    last_key: Option<u8>,
    /// The arrival-order cursor for Played.
    next_idx: usize,
}

impl Continuum {
    /// `rate_hz` is clamped to 2..=30 and `gate` to 0.1..=0.9.
    pub fn new(rate_hz: f32, order: ContinuumOrder, gate: f32, seed: u64) -> Self {
        let rate = rate_hz.clamp(2.0, 30.0);
        let gate = gate.clamp(0.1, 0.9);
        let period_ns = ((1_000_000_000.0 / rate as f64) as u64).max(1);
        Self {
            period_ns,
            gate_ns: (period_ns as f64 * gate as f64) as u64,
            order,
            rng: seeded(seed, 0),
            held: [Held::default(); MAX_HELD],
            len: 0,
            next_due: None,
            last_key: None,
            next_idx: 0,
        }
    }

    fn find(&self, ch: u8, key: u8) -> Option<usize> {
        self.held[..self.len]
            .iter()
            .position(|h| h.ch == ch && h.key == key)
    }

    fn remove(&mut self, i: usize) {
        for j in i..self.len - 1 {
            self.held[j] = self.held[j + 1];
        }
        self.len -= 1;
        if i < self.next_idx {
            self.next_idx -= 1;
        }
    }

    /// Pick the next held entry to sound. `self.len` must be non-zero.
    fn select(&mut self) -> Held {
        let held = &self.held[..self.len];
        let idx = match self.order {
            ContinuumOrder::Up => self
                .last_key
                .and_then(|last| {
                    held.iter()
                        .enumerate()
                        .filter(|(_, h)| h.key > last)
                        .min_by_key(|(_, h)| h.key)
                        .map(|(i, _)| i)
                })
                .unwrap_or_else(|| {
                    held.iter()
                        .enumerate()
                        .min_by_key(|(_, h)| h.key)
                        .map(|(i, _)| i)
                        .unwrap_or(0)
                }),
            ContinuumOrder::Down => self
                .last_key
                .and_then(|last| {
                    held.iter()
                        .enumerate()
                        .filter(|(_, h)| h.key < last)
                        .max_by_key(|(_, h)| h.key)
                        .map(|(i, _)| i)
                })
                .unwrap_or_else(|| {
                    held.iter()
                        .enumerate()
                        .max_by_key(|(_, h)| h.key)
                        .map(|(i, _)| i)
                        .unwrap_or(0)
                }),
            ContinuumOrder::Played => {
                let idx = if self.next_idx >= self.len {
                    0
                } else {
                    self.next_idx
                };
                self.next_idx = idx + 1;
                idx
            }
            ContinuumOrder::Random => {
                let avoid = self
                    .last_key
                    .and_then(|last| held.iter().position(|h| h.key == last));
                match avoid {
                    Some(p) if self.len > 1 => {
                        let r = self.rng.random_range(0..self.len - 1);
                        if r >= p { r + 1 } else { r }
                    }
                    _ => self.rng.random_range(0..self.len),
                }
            }
        };
        held[idx]
    }
}

impl Effect for Continuum {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                if let Some(i) = self.find(ch, key) {
                    self.held[i].vel = vel;
                } else if self.len < MAX_HELD {
                    self.held[self.len] = Held { ch, key, vel };
                    self.len += 1;
                    if self.len == 1 {
                        self.next_due = Some(ev.time);
                        self.last_key = None;
                        self.next_idx = 0;
                    }
                }
            }
            EventKind::NoteOff { ch, key, .. } => {
                if let Some(i) = self.find(ch, key) {
                    self.remove(i);
                    if self.len == 0 {
                        self.next_due = None;
                    }
                }
            }
            _ => push(out, cx, *ev),
        }
    }

    fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        let mut emitted = 0;
        while let Some(due) = self.next_due {
            if due > now {
                break;
            }
            if emitted == MAX_CATCHUP {
                // Resynchronize: skip the missed periods and pick the
                // cycle back up at the first grid point past `now`.
                let missed = (now - due) / self.period_ns + 1;
                self.next_due = Some(due + missed * self.period_ns);
                break;
            }
            let h = self.select();
            self.last_key = Some(h.key);
            let strike = EventKind::NoteOn {
                ch: h.ch,
                key: h.key,
                vel: h.vel,
            };
            let release = EventKind::NoteOff {
                ch: h.ch,
                key: h.key,
                vel: 0,
            };
            cx.push_pair(
                out,
                Event::new(due, strike),
                Event::new(due.saturating_add(self.gate_ns), release),
            );
            emitted += 1;
            self.next_due = Some(due + self.period_ns);
        }
    }

    fn flush(&mut self, _out: &mut EventBuf, _cx: &ProcCx) {
        // Every emitted note carries its own off; only the state clears.
        self.len = 0;
        self.next_due = None;
        self.last_key = None;
        self.next_idx = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, flush, off, on, run_timed, tick};

    const PERIOD: u64 = 100_000_000;

    fn von(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    /// A 10 Hz machine with a 50% gate: period 100ms, gate 50ms.
    fn machine(order: ContinuumOrder) -> Continuum {
        Continuum::new(10.0, order, 0.5, 7)
    }

    /// The note-on keys of an output slice.
    fn ons(out: &[Event]) -> Vec<u8> {
        out.iter()
            .filter_map(|ev| match ev.kind {
                EventKind::NoteOn { key, .. } => Some(key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn note_events_are_consumed_and_others_pass() {
        let mut fx = machine(ContinuumOrder::Up);
        assert_eq!(run_timed(&mut fx, 0, on(60)), vec![]);
        assert_eq!(run_timed(&mut fx, 1, off(60)), vec![]);
        // An off for a key the machine never held is consumed too.
        assert_eq!(run_timed(&mut fx, 2, off(99)), vec![]);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 3, pedal), vec![at(3, pedal)]);
    }

    #[test]
    fn the_cycle_emits_pairs_on_its_grid() {
        let mut fx = machine(ContinuumOrder::Up);
        run_timed(&mut fx, 1_000, on(60));
        // The first emission lands at the note-on time, gate 50ms.
        assert_eq!(
            tick(&mut fx, 1_000),
            vec![at(1_000, on(60)), at(50_001_000, off(60))]
        );
        // The same instant again owes nothing.
        assert_eq!(tick(&mut fx, 1_000), vec![]);
        assert_eq!(
            tick(&mut fx, 1_000 + PERIOD),
            vec![at(1_000 + PERIOD, on(60)), at(50_001_000 + PERIOD, off(60))]
        );
    }

    #[test]
    fn up_order_cycles_ascending() {
        let mut fx = machine(ContinuumOrder::Up);
        for key in [64, 60, 67] {
            run_timed(&mut fx, 0, on(key));
        }
        let mut keys = Vec::new();
        for k in 0..6 {
            keys.extend(ons(&tick(&mut fx, k * PERIOD)));
        }
        assert_eq!(keys, vec![60, 64, 67, 60, 64, 67]);
    }

    #[test]
    fn down_order_cycles_descending() {
        let mut fx = machine(ContinuumOrder::Down);
        for key in [64, 60, 67] {
            run_timed(&mut fx, 0, on(key));
        }
        let mut keys = Vec::new();
        for k in 0..6 {
            keys.extend(ons(&tick(&mut fx, k * PERIOD)));
        }
        assert_eq!(keys, vec![67, 64, 60, 67, 64, 60]);
    }

    #[test]
    fn played_order_follows_arrival() {
        let mut fx = machine(ContinuumOrder::Played);
        for key in [67, 60, 64] {
            run_timed(&mut fx, 0, on(key));
        }
        let mut keys = Vec::new();
        for k in 0..6 {
            keys.extend(ons(&tick(&mut fx, k * PERIOD)));
        }
        assert_eq!(keys, vec![67, 60, 64, 67, 60, 64]);
    }

    #[test]
    fn random_order_avoids_immediate_repeats() {
        let mut fx = machine(ContinuumOrder::Random);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 0, on(64));
        let mut keys = Vec::new();
        for k in 0..12 {
            keys.extend(ons(&tick(&mut fx, k * PERIOD)));
        }
        assert_eq!(keys.len(), 12);
        for w in keys.windows(2) {
            assert_ne!(w[0], w[1], "immediate repeat in {keys:?}");
        }
    }

    #[test]
    fn velocity_follows_the_player() {
        let mut fx = machine(ContinuumOrder::Up);
        run_timed(&mut fx, 0, von(60, 33));
        assert_eq!(
            tick(&mut fx, 0),
            vec![at(0, von(60, 33)), at(50_000_000, off(60))]
        );
        // A retrigger updates the stored velocity in place.
        run_timed(&mut fx, 1, von(60, 77));
        assert_eq!(ons(&tick(&mut fx, PERIOD)), vec![60]);
        let out = tick(&mut fx, 2 * PERIOD);
        assert_eq!(out[0].kind, von(60, 77));
    }

    #[test]
    fn release_stops_the_cycle_and_a_new_key_restarts_it() {
        let mut fx = machine(ContinuumOrder::Up);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 0).len(), 2);
        run_timed(&mut fx, 10_000_000, off(60));
        assert_eq!(tick(&mut fx, PERIOD), vec![]);
        assert_eq!(tick(&mut fx, 10 * PERIOD), vec![]);
        // A fresh key restarts the cycle at its own arrival time.
        run_timed(&mut fx, 1_050_000_000, on(72));
        assert_eq!(
            ons(&tick(&mut fx, 1_050_000_000)),
            vec![72],
            "the restart lands at the note-on time"
        );
    }

    #[test]
    fn a_late_tick_catches_up_at_most_four_pairs() {
        let mut fx = machine(ContinuumOrder::Up);
        run_timed(&mut fx, 0, on(60));
        // Ten periods late: four pairs, stamped on the original grid.
        let out = tick(&mut fx, 10 * PERIOD);
        assert_eq!(out.len(), 8);
        let times: Vec<u64> = out
            .iter()
            .filter(|ev| matches!(ev.kind, EventKind::NoteOn { .. }))
            .map(|ev| ev.time)
            .collect();
        assert_eq!(times, vec![0, PERIOD, 2 * PERIOD, 3 * PERIOD]);
        // The schedule resynchronized to the first grid point past now.
        assert_eq!(tick(&mut fx, 10 * PERIOD), vec![]);
        let out = tick(&mut fx, 11 * PERIOD);
        assert_eq!(out[0].time, 11 * PERIOD);
    }

    #[test]
    fn a_seventeenth_key_is_ignored() {
        let mut fx = machine(ContinuumOrder::Up);
        for key in 0..16 {
            run_timed(&mut fx, 0, on(key));
        }
        run_timed(&mut fx, 0, on(100));
        let mut keys = Vec::new();
        for k in 0..34 {
            keys.extend(ons(&tick(&mut fx, k * PERIOD)));
        }
        assert!(!keys.contains(&100), "the 17th key crept in: {keys:?}");
        assert_eq!(&keys[..16], (0..16).collect::<Vec<u8>>().as_slice());
    }

    #[test]
    fn releasing_mid_cycle_keeps_the_played_cursor() {
        let mut fx = machine(ContinuumOrder::Played);
        for key in [60, 64, 67] {
            run_timed(&mut fx, 0, on(key));
        }
        assert_eq!(ons(&tick(&mut fx, 0)), vec![60]);
        run_timed(&mut fx, 1, off(60));
        assert_eq!(ons(&tick(&mut fx, PERIOD)), vec![64]);
        assert_eq!(ons(&tick(&mut fx, 2 * PERIOD)), vec![67]);
        assert_eq!(ons(&tick(&mut fx, 3 * PERIOD)), vec![64]);
    }

    #[test]
    fn same_seed_replays_the_random_walk() {
        let mut a = Continuum::new(10.0, ContinuumOrder::Random, 0.5, 42);
        let mut b = Continuum::new(10.0, ContinuumOrder::Random, 0.5, 42);
        for key in [60, 64, 67, 70] {
            run_timed(&mut a, 0, on(key));
            run_timed(&mut b, 0, on(key));
        }
        for k in 0..20 {
            assert_eq!(tick(&mut a, k * PERIOD), tick(&mut b, k * PERIOD));
        }
    }

    #[test]
    fn rate_and_gate_clamp() {
        // Rate 0.1 clamps to 2 Hz (500ms period), gate -1 to 0.1 (50ms).
        let mut fx = Continuum::new(0.1, ContinuumOrder::Up, -1.0, 1);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(
            tick(&mut fx, 0),
            vec![at(0, on(60)), at(50_000_000, off(60))]
        );
        assert_eq!(tick(&mut fx, 499_999_999), vec![]);
        assert_eq!(tick(&mut fx, 500_000_000)[0].time, 500_000_000);
        // Rate 1000 clamps to 30 Hz, gate 5 to 0.9.
        let mut fx = Continuum::new(1_000.0, ContinuumOrder::Up, 5.0, 1);
        run_timed(&mut fx, 0, on(60));
        let first = tick(&mut fx, 0);
        let second = tick(&mut fx, 40_000_000);
        let period = second[0].time - first[0].time;
        assert!((33_000_000..=34_000_000).contains(&period), "{period}");
        let gate = first[1].time - first[0].time;
        assert!((29_000_000..=31_000_000).contains(&gate), "{gate}");
    }

    #[test]
    fn flush_clears_without_emitting() {
        let mut fx = machine(ContinuumOrder::Up);
        run_timed(&mut fx, 0, on(60));
        assert_eq!(tick(&mut fx, 0).len(), 2);
        assert_eq!(flush(&mut fx), vec![]);
        assert_eq!(tick(&mut fx, 10 * PERIOD), vec![]);
    }
}
