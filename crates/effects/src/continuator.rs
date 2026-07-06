//! A pocket continuator: learn the player's habits, then keep playing.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx, Timestamp};
use rand::Rng;

use crate::router::push;

/// The continuation never begins sooner than this after the last input.
const MIN_IDLE_NS: u64 = 500_000_000;

/// Inter-onset samples are clamped into this range before the EMA.
const IOI_LO_NS: f32 = 60_000_000.0;
const IOI_HI_NS: f32 = 2_000_000_000.0;

/// Weight of the newest sample in the running averages.
const EMA_ALPHA: f32 = 0.25;

/// Continuation keys reflect into the piano's range.
const KEY_LO: u8 = 21;
const KEY_HI: u8 = 108;

/// Continuation velocity jitters by up to this many steps either way.
const VEL_JITTER: i32 = 5;

/// A very late tick emits at most this many continuation notes before the
/// schedule resynchronizes past `now`.
const MAX_CATCHUP: usize = 2;

/// Fold a key into `KEY_LO..=KEY_HI` by reflecting off both edges.
fn reflect(pos: i32) -> u8 {
    let (lo, hi) = (KEY_LO as i32, KEY_HI as i32);
    let range = hi - lo;
    let m = (pos - lo).rem_euclid(2 * range);
    let folded = if m > range { 2 * range - m } else { m };
    (lo + folded) as u8
}

/// A pocket Markov continuator in the spirit of Pachet's: while the
/// player plays, it learns an interval histogram over -24..=+24 semitones
/// between successive note-ons plus exponential moving averages of
/// inter-onset time (samples clamped to 60ms..=2s) and of velocity. Once
/// no input of any kind has arrived for `idle_ns` and the player holds
/// nothing, it continues from the last played key on the last played
/// channel: intervals drawn from the histogram in proportion to their
/// counts (an empty histogram stays silent), keys reflected into
/// 21..=108, one note every EMA inter-onset time with velocity jittered a
/// few steps around the EMA. Each note-on is stamped on the grid; its
/// note-off comes 80% of an inter-onset later, held back in the machine's
/// own bookkeeping rather than emitted ahead, because ANY input event
/// instantly silences the continuation: the pending off is emitted at the
/// input's time, before the input passes, and learning resumes. The
/// continuation also ends after `max_notes` notes and then stays quiet
/// until fresh input restarts the cycle.
///
/// Input passes through unchanged, notes included: this effect adds a
/// voice and never consumes.
///
/// Ticks may be late: notes fire once `now` reaches their target and are
/// stamped with the target, not `now`. A very late tick emits at most 2
/// catch-up notes (with their due offs), then jumps the schedule to the
/// first grid point past `now`, dropping the missed slots.
///
/// Randomness (interval draws, velocity jitter) comes from `rng::seeded`,
/// so the same seed and the same event and tick sequence replay the same
/// continuation.
///
/// `flush` releases the machine's sounding note plus one note-off per
/// outstanding pass-through note-on, leaving nothing sounding.
///
/// Fanout bound: `process` emits at most 2 events (the silencing off plus
/// the pass-through); a tick emits at most 2 note-ons and 3 note-offs.
pub struct Continuator {
    rng: Prng,
    idle_ns: u64,
    max_notes: u16,
    /// Interval counts for -24..=+24 semitones.
    hist: [u32; 49],
    hist_total: u64,
    /// EMA of inter-onset nanoseconds; 0 until the first interval.
    ema_ioi_ns: f32,
    ema_vel: f32,
    last_key: Option<u8>,
    last_ch: u8,
    last_on_time: Timestamp,
    /// Time of the last input event of any kind; idleness counts from
    /// here, so a continuation never restarts on the heels of the event
    /// that silenced it.
    last_input_time: Timestamp,
    /// Held note-on count per pass-through (channel, key).
    held: PerNote<u8>,
    total_held: u32,
    running: bool,
    /// A finished continuation stays quiet until fresh input arrives.
    spent: bool,
    emitted: u16,
    cur_key: u8,
    next_on: Timestamp,
    /// The machine's sounding note: (channel, key, off due).
    sounding: Option<(u8, u8, Timestamp)>,
}

impl Continuator {
    /// `idle_ns` is raised to at least 500ms and `max_notes` is clamped
    /// to 1..=1000.
    pub fn new(seed: u64, idle_ns: u64, max_notes: u16) -> Self {
        Self {
            rng: seeded(seed, 0),
            idle_ns: idle_ns.max(MIN_IDLE_NS),
            max_notes: max_notes.clamp(1, 1000),
            hist: [0; 49],
            hist_total: 0,
            ema_ioi_ns: 0.0,
            ema_vel: 0.0,
            last_key: None,
            last_ch: 0,
            last_on_time: 0,
            last_input_time: 0,
            held: PerNote::new(),
            total_held: 0,
            running: false,
            spent: false,
            emitted: 0,
            cur_key: 60,
            next_on: 0,
            sounding: None,
        }
    }

    /// One interval draw, proportional to the histogram counts.
    /// `hist_total` must be non-zero.
    fn draw_interval(&mut self) -> i32 {
        let mut r = self.rng.random_range(0..self.hist_total);
        for (i, &count) in self.hist.iter().enumerate() {
            let count = count as u64;
            if r < count {
                return i as i32 - 24;
            }
            r -= count;
        }
        0
    }
}

impl Effect for Continuator {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        // Any input instantly silences a running continuation, the off
        // first so the machine's note ends before the player's event.
        if self.running {
            self.running = false;
            if let Some((ch, key, _)) = self.sounding.take() {
                let cut = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(ev.time, cut));
            }
        }
        self.spent = false;
        self.last_input_time = ev.time;
        push(out, cx, *ev);
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                if let Some(prev) = self.last_key {
                    let interval = key as i32 - prev as i32;
                    if (-24..=24).contains(&interval) {
                        self.hist[(interval + 24) as usize] += 1;
                        self.hist_total += 1;
                    }
                    let ioi = (ev.time.saturating_sub(self.last_on_time) as f32)
                        .clamp(IOI_LO_NS, IOI_HI_NS);
                    self.ema_ioi_ns = if self.ema_ioi_ns == 0.0 {
                        ioi
                    } else {
                        EMA_ALPHA * ioi + (1.0 - EMA_ALPHA) * self.ema_ioi_ns
                    };
                    self.ema_vel = EMA_ALPHA * vel as f32 + (1.0 - EMA_ALPHA) * self.ema_vel;
                } else {
                    self.ema_vel = vel as f32;
                }
                self.last_key = Some(key);
                self.last_ch = ch;
                self.last_on_time = ev.time;
                let n = self.held.get(ch, key);
                self.held.set(ch, key, n.saturating_add(1));
                self.total_held += 1;
            }
            EventKind::NoteOff { ch, key, .. } => {
                let n = self.held.get(ch, key);
                if n > 0 {
                    self.held.set(ch, key, n - 1);
                    self.total_held -= 1;
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        if !self.running
            && !self.spent
            && self.total_held == 0
            && self.hist_total > 0
            && self.last_key.is_some()
            && now >= self.last_input_time.saturating_add(self.idle_ns)
        {
            self.running = true;
            self.emitted = 0;
            self.cur_key = self.last_key.unwrap_or(60);
            // The continuation begins on the idle threshold itself, not
            // at whatever moment the tick noticed it.
            self.next_on = self.last_input_time + self.idle_ns;
        }
        if !self.running {
            return;
        }
        let ioi = self.ema_ioi_ns as u64;
        let mut notes = 0;
        loop {
            // The pending off (80% of an inter-onset) always precedes the
            // next on (a full one), so it goes first when due.
            if let Some((ch, key, due)) = self.sounding
                && due <= now
            {
                let off = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(due, off));
                self.sounding = None;
                continue;
            }
            if self.emitted >= self.max_notes {
                if self.sounding.is_none() {
                    // The continuation ran its course; stay quiet until
                    // the player returns.
                    self.running = false;
                    self.spent = true;
                }
                break;
            }
            if self.next_on > now {
                break;
            }
            if notes == MAX_CATCHUP {
                // Missed slots are dropped, not bunched: jump to the
                // first grid point past now.
                let missed = (now - self.next_on) / ioi + 1;
                self.next_on += missed * ioi;
                break;
            }
            let interval = self.draw_interval();
            self.cur_key = reflect(self.cur_key as i32 + interval);
            let jitter = self.rng.random_range(-VEL_JITTER..=VEL_JITTER) as f32;
            let vel = (self.ema_vel + jitter).round().clamp(1.0, 127.0) as u8;
            let on = EventKind::NoteOn {
                ch: self.last_ch,
                key: self.cur_key,
                vel,
            };
            push(out, cx, Event::new(self.next_on, on));
            self.sounding = Some((self.last_ch, self.cur_key, self.next_on + ioi * 4 / 5));
            self.emitted += 1;
            self.next_on += ioi;
            notes += 1;
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        if let Some((ch, key, _)) = self.sounding.take() {
            let cut = EventKind::NoteOff { ch, key, vel: 0 };
            push(out, cx, Event::new(cx.now, cut));
        }
        self.running = false;
        self.spent = true;
        // The player's note-ons passed through unchanged, so wind them
        // down too: one note-off per outstanding pass-through note-on.
        let held = std::mem::take(&mut self.held);
        held.for_each(|ch, key, n| {
            for _ in 0..n {
                let off = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(cx.now, off));
            }
        });
        self.total_held = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, flush, off, on, run_timed, tick};

    const SEC: u64 = 1_000_000_000;

    fn von(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    /// Teach the machine one rising second: 60 then 62, 500ms apart, both
    /// released. The idle threshold lands at 1.6s.
    fn taught(seed: u64) -> Continuator {
        let mut fx = Continuator::new(seed, SEC, 100);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 100_000_000, off(60));
        run_timed(&mut fx, 500_000_000, on(62));
        run_timed(&mut fx, 600_000_000, off(62));
        fx
    }

    #[test]
    fn input_passes_through_unchanged() {
        let mut fx = Continuator::new(1, SEC, 100);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        for (t, kind) in [(0, on(60)), (10, pedal), (20, off(60))] {
            assert_eq!(run_timed(&mut fx, t, kind), vec![at(t, kind)]);
        }
    }

    #[test]
    fn an_unlearned_machine_stays_silent() {
        // No input at all: nothing to continue.
        let mut fx = Continuator::new(1, SEC, 100);
        assert_eq!(tick(&mut fx, 100 * SEC), vec![]);
        // One lone note leaves the histogram empty: still silent.
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 100_000_000, off(60));
        assert_eq!(tick(&mut fx, 100 * SEC), vec![]);
    }

    #[test]
    fn continuation_starts_on_the_idle_grid() {
        let mut fx = taught(1);
        // Not idle yet at 1.5s (last input at 0.6s, idle 1s).
        assert_eq!(tick(&mut fx, 1_500_000_000), vec![]);
        // At 1.7s the first note is due, stamped on the threshold: the
        // only learned interval is +2, so 62 continues to 64, and the
        // velocity hugs the EMA of 100.
        let out = tick(&mut fx, 1_700_000_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].time, 1_600_000_000);
        let EventKind::NoteOn {
            ch: 0,
            key: 64,
            vel,
        } = out[0].kind
        else {
            panic!("expected a note-on of 64, got {:?}", out[0].kind);
        };
        assert!((95..=105).contains(&vel), "vel {vel}");
        // The off lands 80% of the 500ms EMA inter-onset later.
        assert_eq!(tick(&mut fx, 1_990_000_000), vec![]);
        assert_eq!(tick(&mut fx, 2_000_000_000), vec![at(2 * SEC, off(64))]);
        // The next note a full inter-onset after the first.
        let out = tick(&mut fx, 2_100_000_000);
        assert_eq!(out[0].time, 2_100_000_000);
        assert_eq!(out[0].kind.key(), Some(66));
    }

    #[test]
    fn held_keys_block_the_continuation() {
        let mut fx = Continuator::new(1, SEC, 100);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 500_000_000, on(62));
        run_timed(&mut fx, 600_000_000, off(62));
        // 60 is still down: no continuation, however idle.
        assert_eq!(tick(&mut fx, 50 * SEC), vec![]);
        // Releasing it restarts the idle clock from the release.
        run_timed(&mut fx, 51 * SEC, off(60));
        assert_eq!(tick(&mut fx, 51 * SEC + SEC - 1), vec![]);
        let out = tick(&mut fx, 52 * SEC + 1);
        assert_eq!(out[0].time, 52 * SEC);
    }

    #[test]
    fn any_note_on_silences_the_continuation_instantly() {
        let mut fx = taught(1);
        assert_eq!(tick(&mut fx, 1_700_000_000).len(), 1);
        // The machine holds 64; the player's note cuts it first.
        let out = run_timed(&mut fx, 1_750_000_000, on(50));
        assert_eq!(
            out,
            vec![at(1_750_000_000, off(64)), at(1_750_000_000, on(50))]
        );
        // No restart until a fresh idle span (and 50 is held anyway).
        assert_eq!(tick(&mut fx, 1_800_000_000), vec![]);
    }

    #[test]
    fn a_controller_also_silences() {
        let mut fx = taught(1);
        assert_eq!(tick(&mut fx, 1_700_000_000).len(), 1);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        let out = run_timed(&mut fx, 1_750_000_000, pedal);
        assert_eq!(
            out,
            vec![at(1_750_000_000, off(64)), at(1_750_000_000, pedal)]
        );
        // The pedal reset the idle clock too: quiet until 2.75s.
        assert_eq!(tick(&mut fx, 2_700_000_000), vec![]);
        assert_eq!(tick(&mut fx, 2_800_000_000).len(), 1);
    }

    #[test]
    fn max_notes_caps_the_continuation_for_good() {
        let mut fx = Continuator::new(1, SEC, 2);
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 100_000_000, off(60));
        run_timed(&mut fx, 500_000_000, on(62));
        run_timed(&mut fx, 600_000_000, off(62));
        assert_eq!(tick(&mut fx, 1_700_000_000).len(), 1);
        assert_eq!(tick(&mut fx, 2_050_000_000), vec![at(2 * SEC, off(64))]);
        assert_eq!(tick(&mut fx, 2_200_000_000).len(), 1);
        // The second note's off ends it; nothing follows, ever.
        assert_eq!(
            tick(&mut fx, 2_600_000_000),
            vec![at(2_500_000_000, off(66))]
        );
        assert_eq!(tick(&mut fx, 1_000 * SEC), vec![]);
    }

    #[test]
    fn keys_reflect_into_the_piano_range() {
        let mut fx = Continuator::new(1, SEC, 100);
        // Teach a +24 leap from high up: 84 then 108.
        run_timed(&mut fx, 0, on(84));
        run_timed(&mut fx, 100_000_000, off(84));
        run_timed(&mut fx, 500_000_000, on(108));
        run_timed(&mut fx, 600_000_000, off(108));
        let mut keys = Vec::new();
        for k in 0..30u64 {
            let now = 1_700_000_000 + k * 250_000_000;
            for ev in tick(&mut fx, now) {
                if let EventKind::NoteOn { key, .. } = ev.kind {
                    keys.push(key);
                }
            }
        }
        assert!(!keys.is_empty());
        // From 108 a +24 reflects back down to 84, then up again.
        assert!(keys.iter().all(|k| (21..=108).contains(k)), "{keys:?}");
        assert!(keys.iter().all(|k| *k == 84 || *k == 108), "{keys:?}");
    }

    #[test]
    fn a_late_tick_emits_at_most_two_notes() {
        let mut fx = taught(1);
        // Waking 20s late: two notes with their due offs, then a resync.
        let out = tick(&mut fx, 20 * SEC);
        assert_eq!(out.len(), 4, "{out:?}");
        assert_eq!(
            out.iter().map(|ev| ev.time).collect::<Vec<_>>(),
            vec![1_600_000_000, 2_000_000_000, 2_100_000_000, 2_500_000_000]
        );
        // Resynchronized: the next note lands on the grid past now.
        let out = tick(&mut fx, 20_200_000_000);
        assert_eq!(out[0].time, 20_100_000_000);
    }

    #[test]
    fn the_ioi_ema_clamps() {
        let mut fx = Continuator::new(1, SEC, 100);
        // Two onsets 1ms apart: the sample clamps to 60ms.
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 1_000_000, on(62));
        run_timed(&mut fx, 2_000_000, off(60));
        run_timed(&mut fx, 3_000_000, off(62));
        // Threshold at 3ms + 1s; two notes fit before 1.1s on a 60ms grid.
        let out = tick(&mut fx, 1_100_000_000);
        let ons: Vec<u64> = out
            .iter()
            .filter(|ev| matches!(ev.kind, EventKind::NoteOn { .. }))
            .map(|ev| ev.time)
            .collect();
        assert_eq!(ons, vec![1_003_000_000, 1_063_000_000]);
    }

    #[test]
    fn same_seed_same_continuation() {
        let mut a = taught(9);
        let mut b = taught(9);
        for k in 0..30u64 {
            let now = 1_600_000_000 + k * 333_000_000;
            assert_eq!(tick(&mut a, now), tick(&mut b, now));
        }
    }

    #[test]
    fn flush_releases_the_voice_and_the_held_passthrough() {
        // A held pass-through note winds down.
        let mut fx = Continuator::new(1, SEC, 100);
        run_timed(&mut fx, 0, von(60, 90));
        assert_eq!(flush(&mut fx), vec![off(60)]);
        assert_eq!(flush(&mut fx), vec![]);
        // The machine's own sounding note winds down too, and the spent
        // machine stays quiet afterward.
        let mut fx = taught(1);
        assert_eq!(tick(&mut fx, 1_700_000_000).len(), 1);
        assert_eq!(flush(&mut fx), vec![off(64)]);
        assert_eq!(tick(&mut fx, 100 * SEC), vec![]);
    }
}
