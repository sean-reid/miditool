//! Feldman's crippled symmetry: a pedal-captured phrase loops and warps.

use miditool_core::rng::{Prng, seeded};
use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx, Timestamp};
use rand::Rng;

use crate::router::push;

/// Fixed recording capacity; `max_notes` can only narrow it.
const CAPACITY: usize = 32;

/// Breathing room appended after the phrase's final note when the pedal
/// span alone would cut it short.
const TAIL_NS: u64 = 50_000_000;

/// A pedal value of 64 or higher counts as down.
const PEDAL_DOWN: u8 = 64;

/// One recorded note, times relative to the capture start.
#[derive(Debug, Clone, Copy, Default)]
struct CapturedNote {
    ch: u8,
    key: u8,
    vel: u8,
    onset_ns: u64,
    duration_ns: u64,
    /// Still held: the duration stays provisional until the note-off or
    /// the pedal-up arrives.
    open: bool,
}

/// Feldman's crippled symmetry as a pedal looper. The configured control
/// pedal (its CC number on any channel; a value of 64 or higher is down)
/// is the capture control and is CONSUMED: the DAW never sees it. Pedal
/// down stops any running loop (note-offs for everything the machine has
/// sounding) and begins a capture; pedal up ends it. While the pedal is
/// down, up to `max_notes` notes are recorded as (key, velocity, onset,
/// duration) relative to the capture start: notes beyond capacity are not
/// recorded, and a note still held at pedal-up gets its duration capped
/// there. The player's notes pass through unchanged at all times, pedal
/// down or up: the machine adds a voice and never consumes notes.
///
/// If at least one note was captured, the loop starts at pedal-up with
/// length `max(pedal-up - capture-start, last onset + its duration +
/// 50ms)` and repeats forever. Every repetition, the first included,
/// applies exactly one seeded mutation drawn uniformly among: nudge one
/// note's onset by 10% of the loop length (direction drawn, clamped so
/// the note stays inside the loop), step one note's velocity by 12 up or
/// down (clamped 1..=127), drop one note for that repetition only, or
/// swap the onsets of two adjacent notes in onset order. Every mutation
/// except the drop PERSISTS, so the phrase slowly warps as it repeats. A
/// single-note phrase draws only from the first three; the swap needs two
/// notes.
///
/// Playback runs from `tick` against target timestamps: a note-on is
/// emitted once `now` reaches its onset, stamped with the target, and
/// remembered in the machine's bookkeeping so the note-off follows when
/// the duration elapses. Nothing is emitted ahead, so pedal-down and
/// `flush` can always silence exactly what is sounding. If a repetition
/// strikes a note whose previous instance is still sounding, the old
/// instance is cut at the new strike.
///
/// Ticks may be late: a tick finishes the repetition in progress and at
/// most one more, then jumps the schedule to the first repetition
/// boundary past `now`. The skipped repetitions draw no mutation; the
/// jumped-to repetition draws its one mutation as usual. Note-offs are
/// never skipped: a pending off is emitted once due, however late, and a
/// tick that fills the output buffer to within a pair of events stops
/// there and resumes on the next tick.
///
/// Mutations come from `rng::seeded`, one short draw sequence per
/// repetition, so the same seed and the same event and tick sequence
/// replay the same warp.
///
/// `flush` releases everything the machine has sounding plus one note-off
/// per outstanding pass-through note-on, then forgets the phrase.
///
/// Fanout bound: a tick emits at most two repetitions of note-ons plus
/// the pending note-offs and stops two short of a full buffer, deferring
/// the rest; `process` emits at most the pass-through event, or up to 32
/// note-offs on the pedal-down that stops a loop.
pub struct CrippledLooper {
    rng: Prng,
    pedal_cc: u8,
    max_notes: usize,
    pedal_down: bool,
    capturing: bool,
    capture_start: Timestamp,
    notes: [CapturedNote; CAPACITY],
    len: usize,
    looping: bool,
    loop_len: u64,
    /// Start of the repetition currently playing.
    rep_start: Timestamp,
    /// Note indices in onset order for the current repetition.
    order: [u8; CAPACITY],
    /// Cursor into `order`: the next note-on still owed this repetition.
    next_pos: usize,
    /// The note silenced for this repetition only, if the drop was drawn.
    dropped: Option<usize>,
    /// Per phrase slot: the sounding instance's note-off due time.
    sounding: [Option<Timestamp>; CAPACITY],
    /// Held pass-through note-on counts, wound down by `flush`.
    held: PerNote<u8>,
}

impl CrippledLooper {
    /// `max_notes` is clamped to 2..=32 and `pedal_cc` to 0..=127.
    pub fn new(seed: u64, pedal_cc: u8, max_notes: u8) -> Self {
        Self {
            rng: seeded(seed, 0),
            pedal_cc: pedal_cc.min(127),
            max_notes: max_notes.clamp(2, CAPACITY as u8) as usize,
            pedal_down: false,
            capturing: false,
            capture_start: 0,
            notes: [CapturedNote::default(); CAPACITY],
            len: 0,
            looping: false,
            loop_len: TAIL_NS,
            rep_start: 0,
            order: [0; CAPACITY],
            next_pos: 0,
            dropped: None,
            sounding: [None; CAPACITY],
            held: PerNote::new(),
        }
    }

    /// Stop the loop, releasing every machine note still sounding.
    fn stop(&mut self, time: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        self.looping = false;
        for (slot, note) in self.sounding[..self.len].iter_mut().zip(&self.notes) {
            if slot.take().is_some() {
                let cut = EventKind::NoteOff {
                    ch: note.ch,
                    key: note.key,
                    vel: 0,
                };
                push(out, cx, Event::new(time, cut));
            }
        }
    }

    fn begin_capture(&mut self, time: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        self.stop(time, out, cx);
        self.capturing = true;
        self.capture_start = time;
        self.len = 0;
    }

    /// Pedal up: cap held notes, fix the loop length, and start looping.
    fn end_capture(&mut self, time: Timestamp) {
        self.capturing = false;
        let span = time.saturating_sub(self.capture_start);
        let mut last_onset = 0u64;
        let mut last_end = 0u64;
        for note in self.notes[..self.len].iter_mut() {
            if note.open {
                note.duration_ns = span.saturating_sub(note.onset_ns);
                note.open = false;
            }
            if note.onset_ns >= last_onset {
                last_onset = note.onset_ns;
                last_end = note.onset_ns + note.duration_ns;
            }
        }
        if self.len == 0 {
            return;
        }
        self.loop_len = span.max(last_end + TAIL_NS);
        self.looping = true;
        self.rep_start = time;
        self.sounding = [None; CAPACITY];
        self.begin_rep();
    }

    /// Rebuild `order` as the note indices sorted by onset (ties keep
    /// recording order). Insertion sort: fixed arrays, no allocation.
    fn sort_order(&mut self) {
        for (i, slot) in self.order[..self.len].iter_mut().enumerate() {
            *slot = i as u8;
        }
        for i in 1..self.len {
            let mut j = i;
            while j > 0
                && self.notes[self.order[j] as usize].onset_ns
                    < self.notes[self.order[j - 1] as usize].onset_ns
            {
                self.order.swap(j, j - 1);
                j -= 1;
            }
        }
    }

    /// Enter a repetition: draw its one mutation and reset the cursor.
    fn begin_rep(&mut self) {
        self.dropped = None;
        self.sort_order();
        self.mutate();
        self.sort_order();
        self.next_pos = 0;
    }

    /// One seeded mutation, uniform over the kinds that make sense for
    /// the phrase size.
    fn mutate(&mut self) {
        let n = self.len;
        // The swap needs two notes; single-note phrases skip it.
        let kinds: u32 = if n >= 2 { 4 } else { 3 };
        match self.rng.random_range(0..kinds) {
            0 => {
                // Nudge one onset by 10% of the loop, direction drawn,
                // clamped so the whole note stays inside the loop.
                let j = self.rng.random_range(0..n);
                let forward: bool = self.rng.random();
                let delta = (self.loop_len / 10) as i64;
                let delta = if forward { delta } else { -delta };
                let hi = self.loop_len.saturating_sub(self.notes[j].duration_ns) as i64;
                self.notes[j].onset_ns =
                    (self.notes[j].onset_ns as i64 + delta).clamp(0, hi) as u64;
            }
            1 => {
                // Step one velocity by 12, direction drawn.
                let j = self.rng.random_range(0..n);
                let up: bool = self.rng.random();
                let vel = self.notes[j].vel as i32 + if up { 12 } else { -12 };
                self.notes[j].vel = vel.clamp(1, 127) as u8;
            }
            2 => {
                // Drop one note, for this repetition only.
                self.dropped = Some(self.rng.random_range(0..n));
            }
            _ => {
                // Swap the onsets of two adjacent notes in onset order;
                // durations and keys travel with their notes.
                let p = self.rng.random_range(0..n - 1);
                let a = self.order[p] as usize;
                let b = self.order[p + 1] as usize;
                let earlier = self.notes[a].onset_ns;
                let later = self.notes[b].onset_ns;
                self.notes[a].onset_ns = later;
                self.notes[b].onset_ns = earlier;
            }
        }
    }
}

impl Effect for CrippledLooper {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        if let EventKind::ControlChange { cc, value, .. } = ev.kind
            && cc == self.pedal_cc
        {
            // The capture control is consumed; only transitions act.
            let down = value >= PEDAL_DOWN;
            if down && !self.pedal_down {
                self.begin_capture(ev.time, out, cx);
            } else if !down && self.pedal_down {
                self.end_capture(ev.time);
            }
            self.pedal_down = down;
            return;
        }
        push(out, cx, *ev);
        match ev.kind {
            EventKind::NoteOn { ch, key, vel } => {
                let n = self.held.get(ch, key);
                self.held.set(ch, key, n.saturating_add(1));
                if self.capturing && self.len < self.max_notes {
                    self.notes[self.len] = CapturedNote {
                        ch,
                        key,
                        vel,
                        onset_ns: ev.time.saturating_sub(self.capture_start),
                        duration_ns: 0,
                        open: true,
                    };
                    self.len += 1;
                }
            }
            EventKind::NoteOff { ch, key, .. } => {
                let n = self.held.get(ch, key);
                if n > 0 {
                    self.held.set(ch, key, n - 1);
                }
                if self.capturing {
                    let rel = ev.time.saturating_sub(self.capture_start);
                    if let Some(note) = self.notes[..self.len]
                        .iter_mut()
                        .find(|n| n.open && n.ch == ch && n.key == key)
                    {
                        note.duration_ns = rel.saturating_sub(note.onset_ns);
                        note.open = false;
                    }
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        if !self.looping {
            return;
        }
        let mut advanced = false;
        loop {
            // A strike may need a cut plus its note-on; never split it
            // across a full buffer. Whatever is still owed waits for the
            // next tick.
            if out.remaining_capacity() < 2 {
                break;
            }
            // The earliest pending note-off.
            let mut off: Option<(usize, Timestamp)> = None;
            for (i, slot) in self.sounding[..self.len].iter().enumerate() {
                if let Some(due) = *slot
                    && off.is_none_or(|(_, best)| due < best)
                {
                    off = Some((i, due));
                }
            }
            // The next note-on of this repetition; the dropped note is
            // skipped in silence.
            while self.next_pos < self.len
                && self.dropped == Some(self.order[self.next_pos] as usize)
            {
                self.next_pos += 1;
            }
            let on_due = (self.next_pos < self.len)
                .then(|| self.rep_start + self.notes[self.order[self.next_pos] as usize].onset_ns);
            // Chronological merge; offs are never capped and win ties.
            if let Some((i, due)) = off
                && due <= now
                && on_due.is_none_or(|on| due <= on)
            {
                self.sounding[i] = None;
                let n = self.notes[i];
                let kind = EventKind::NoteOff {
                    ch: n.ch,
                    key: n.key,
                    vel: 0,
                };
                push(out, cx, Event::new(due, kind));
                continue;
            }
            match on_due {
                Some(due) if due <= now => {
                    let idx = self.order[self.next_pos] as usize;
                    let n = self.notes[idx];
                    if self.sounding[idx].take().is_some() {
                        // The previous repetition's instance still
                        // sounds: cut it at the new strike.
                        let cut = EventKind::NoteOff {
                            ch: n.ch,
                            key: n.key,
                            vel: 0,
                        };
                        push(out, cx, Event::new(due, cut));
                    }
                    let strike = EventKind::NoteOn {
                        ch: n.ch,
                        key: n.key,
                        vel: n.vel,
                    };
                    push(out, cx, Event::new(due, strike));
                    self.sounding[idx] = Some(due.saturating_add(n.duration_ns));
                    self.next_pos += 1;
                }
                Some(_) => break,
                None => {
                    // This repetition is spent; the next begins on the
                    // loop boundary.
                    let boundary = self.rep_start + self.loop_len;
                    if boundary > now {
                        break;
                    }
                    if advanced {
                        // A very late tick: jump to the first boundary
                        // past now, skipping the missed repetitions
                        // (which draw no mutation).
                        let missed = (now - self.rep_start) / self.loop_len + 1;
                        self.rep_start += missed * self.loop_len;
                    } else {
                        advanced = true;
                        self.rep_start = boundary;
                    }
                    self.begin_rep();
                }
            }
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.stop(cx.now, out, cx);
        self.capturing = false;
        self.pedal_down = false;
        self.len = 0;
        // The player's note-ons passed through unchanged; wind them down
        // too, one note-off per outstanding pass-through note-on.
        let held = std::mem::take(&mut self.held);
        held.for_each(|ch, key, n| {
            for _ in 0..n {
                let off = EventKind::NoteOff { ch, key, vel: 0 };
                push(out, cx, Event::new(cx.now, off));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, flush, off, on, run_timed, tick};

    const MS: u64 = 1_000_000;

    fn pedal(value: u8) -> EventKind {
        EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value,
        }
    }

    fn von(key: u8, vel: u8) -> EventKind {
        EventKind::NoteOn { ch: 0, key, vel }
    }

    fn looper(seed: u64) -> CrippledLooper {
        CrippledLooper::new(seed, 64, 32)
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

    /// The note-on times of an output slice.
    fn on_times(out: &[Event]) -> Vec<u64> {
        out.iter()
            .filter(|ev| matches!(ev.kind, EventKind::NoteOn { .. }))
            .map(|ev| ev.time)
            .collect()
    }

    /// A seed whose leading draws satisfy `pred` (replaying the exact
    /// draw order `mutate` uses).
    fn find_seed(pred: impl Fn(&mut Prng) -> bool) -> u64 {
        (0..20_000u64)
            .find(|&s| pred(&mut seeded(s, 0)))
            .expect("such a seed exists")
    }

    /// Consume one mutation's draws off `rng`, succeeding only if it was
    /// the velocity kind (which never moves the schedule).
    fn draw_vel(rng: &mut Prng, n: usize) -> bool {
        let kinds: u32 = if n >= 2 { 4 } else { 3 };
        if rng.random_range(0..kinds) != 1 {
            return false;
        }
        let _ = rng.random_range(0..n);
        let _: bool = rng.random();
        true
    }

    /// Like `draw_vel`, but the step must also go up.
    fn draw_vel_up(rng: &mut Prng, n: usize) -> bool {
        let kinds: u32 = if n >= 2 { 4 } else { 3 };
        if rng.random_range(0..kinds) != 1 {
            return false;
        }
        let _ = rng.random_range(0..n);
        rng.random::<bool>()
    }

    /// Pedal at 0, note 60 over [100ms, 200ms] and 64 over [300ms,
    /// 350ms], pedal up at 1s: the pedal span (1s) beats the phrase end
    /// plus tail (400ms), so the loop is 1s long.
    fn captured_pair(seed: u64) -> CrippledLooper {
        let mut fx = looper(seed);
        assert_eq!(run_timed(&mut fx, 0, pedal(127)), vec![]);
        run_timed(&mut fx, 100 * MS, on(60));
        run_timed(&mut fx, 200 * MS, off(60));
        run_timed(&mut fx, 300 * MS, on(64));
        run_timed(&mut fx, 350 * MS, off(64));
        assert_eq!(run_timed(&mut fx, 1000 * MS, pedal(0)), vec![]);
        fx
    }

    #[test]
    fn the_pedal_is_consumed_and_everything_else_passes() {
        let mut fx = looper(1);
        assert_eq!(run_timed(&mut fx, 0, pedal(127)), vec![]);
        // Notes pass through during capture; another CC passes too.
        assert_eq!(run_timed(&mut fx, 1, on(60)), vec![at(1, on(60))]);
        let wheel = EventKind::ControlChange {
            ch: 0,
            cc: 1,
            value: 5,
        };
        assert_eq!(run_timed(&mut fx, 2, wheel), vec![at(2, wheel)]);
        assert_eq!(run_timed(&mut fx, 3, off(60)), vec![at(3, off(60))]);
        assert_eq!(run_timed(&mut fx, 4, pedal(0)), vec![]);
        // A custom pedal number frees CC 64 to pass through.
        let mut fx = CrippledLooper::new(1, 20, 4);
        assert_eq!(run_timed(&mut fx, 0, pedal(127)), vec![at(0, pedal(127))]);
        let capture = EventKind::ControlChange {
            ch: 0,
            cc: 20,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 1, capture), vec![]);
    }

    #[test]
    fn the_pedal_span_sets_the_loop_length_when_longer() {
        // Two velocity mutations keep two repetitions of timing pristine.
        let seed = find_seed(|r| draw_vel(r, 2) && draw_vel(r, 2));
        let mut fx = captured_pair(seed);
        // Repetition 1 replays the phrase one second after its onsets.
        let out = tick(&mut fx, 1400 * MS);
        let times: Vec<u64> = out.iter().map(|ev| ev.time).collect();
        assert_eq!(times, vec![1100 * MS, 1200 * MS, 1300 * MS, 1350 * MS]);
        assert_eq!(ons(&out), vec![60, 64]);
        // Repetition 2 begins a full pedal span after the first.
        let out = tick(&mut fx, 2150 * MS);
        assert_eq!(on_times(&out), vec![2100 * MS]);
        assert_eq!(ons(&out), vec![60]);
    }

    #[test]
    fn a_short_pedal_stroke_gets_the_fifty_ms_tail() {
        let seed = find_seed(|r| draw_vel(r, 1) && draw_vel(r, 1));
        let mut fx = looper(seed);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 10 * MS, on(60));
        run_timed(&mut fx, 20 * MS, off(60));
        run_timed(&mut fx, 30 * MS, pedal(0));
        // Loop length = 10ms onset + 10ms duration + 50ms tail = 70ms,
        // longer than the 30ms pedal span.
        assert_eq!(on_times(&tick(&mut fx, 45 * MS)), vec![40 * MS]);
        assert_eq!(tick(&mut fx, 55 * MS), vec![at(50 * MS, off(60))]);
        assert_eq!(on_times(&tick(&mut fx, 115 * MS)), vec![110 * MS]);
    }

    #[test]
    fn a_note_still_held_at_pedal_up_is_capped_there() {
        let seed = find_seed(|r| draw_vel(r, 1));
        let mut fx = looper(seed);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 100 * MS, on(60));
        run_timed(&mut fx, 1000 * MS, pedal(0));
        // Duration capped at 900ms; the loop is 100 + 900 + 50 = 1050ms.
        assert_eq!(on_times(&tick(&mut fx, 1100 * MS)), vec![1100 * MS]);
        assert_eq!(tick(&mut fx, 2000 * MS), vec![at(2000 * MS, off(60))]);
    }

    #[test]
    fn notes_beyond_capacity_pass_through_but_never_loop() {
        // max_notes 0 clamps to 2: the third note passes but is not
        // recorded.
        let seed = find_seed(|r| draw_vel(r, 2));
        let mut fx = CrippledLooper::new(seed, 64, 0);
        run_timed(&mut fx, 0, pedal(127));
        for (t, key) in [(100, 60), (200, 64), (300, 67)] {
            assert_eq!(
                run_timed(&mut fx, t * MS, on(key)),
                vec![at(t * MS, on(key))]
            );
            let t_off = (t + 50) * MS;
            assert_eq!(
                run_timed(&mut fx, t_off, off(key)),
                vec![at(t_off, off(key))]
            );
        }
        run_timed(&mut fx, 1000 * MS, pedal(0));
        assert_eq!(ons(&tick(&mut fx, 1400 * MS)), vec![60, 64]);
    }

    #[test]
    fn only_notes_captured_under_the_pedal_loop() {
        let seed = find_seed(|r| draw_vel(r, 1));
        let mut fx = looper(seed);
        // Played before the pedal: passes, never recorded.
        run_timed(&mut fx, 0, on(70));
        run_timed(&mut fx, 10 * MS, off(70));
        run_timed(&mut fx, 100 * MS, pedal(127));
        run_timed(&mut fx, 200 * MS, on(60));
        run_timed(&mut fx, 300 * MS, off(60));
        run_timed(&mut fx, 500 * MS, pedal(0));
        let mut keys = Vec::new();
        for k in 1..=20u64 {
            keys.extend(ons(&tick(&mut fx, 500 * MS + k * 100 * MS)));
        }
        assert!(!keys.is_empty());
        assert!(keys.iter().all(|&k| k == 60), "{keys:?}");
    }

    #[test]
    fn a_full_thirty_two_note_phrase_loops_and_a_thirty_third_does_not() {
        // max_notes 255 clamps to the fixed capacity of 32.
        let seed = find_seed(|r| draw_vel(r, 32));
        let mut fx = CrippledLooper::new(seed, 64, 255);
        run_timed(&mut fx, 0, pedal(127));
        for i in 0..33u64 {
            let t = (i + 1) * 10 * MS;
            run_timed(&mut fx, t, on(40 + i as u8));
            run_timed(&mut fx, t + 5 * MS, off(40 + i as u8));
        }
        run_timed(&mut fx, 400 * MS, pedal(0));
        // The whole first repetition fits before the 800ms boundary.
        let keys = ons(&tick(&mut fx, 799 * MS));
        assert_eq!(keys, (40..72).collect::<Vec<u8>>());
    }

    #[test]
    fn a_drop_lasts_one_repetition_only() {
        // First mutation: drop; second: velocity.
        let seed = find_seed(|r| {
            r.random_range(0..4u32) == 2 && {
                let _ = r.random_range(0..2usize);
                draw_vel(r, 2)
            }
        });
        // Replicate the draw to learn which note fell silent.
        let mut r = seeded(seed, 0);
        let _ = r.random_range(0..4u32);
        let kept = if r.random_range(0..2usize) == 0 {
            64
        } else {
            60
        };
        let mut fx = captured_pair(seed);
        // Repetition 1: only the kept note sounds.
        assert_eq!(ons(&tick(&mut fx, 1999 * MS)), vec![kept]);
        // Repetition 2: the dropped note is back at its original onset.
        let out = tick(&mut fx, 2400 * MS);
        assert_eq!(ons(&out), vec![60, 64]);
        assert_eq!(on_times(&out), vec![2100 * MS, 2300 * MS]);
    }

    #[test]
    fn a_nudge_persists_across_repetitions() {
        // First mutation: nudge; second: velocity (timing preserved).
        let seed = find_seed(|r| {
            r.random_range(0..4u32) == 0 && {
                let _ = r.random_range(0..2usize);
                let _: bool = r.random();
                draw_vel(r, 2)
            }
        });
        // Replicate the draw: which note moved, and which way.
        let mut r = seeded(seed, 0);
        let _ = r.random_range(0..4u32);
        let j = r.random_range(0..2usize);
        let forward: bool = r.random();
        let mut onsets = [100 * MS, 300 * MS];
        let delta = 100 * MS; // 10% of the 1s loop
        onsets[j] = if forward {
            onsets[j] + delta
        } else {
            onsets[j] - delta
        };
        let mut expected = onsets.to_vec();
        expected.sort_unstable();
        let mut fx = captured_pair(seed);
        // Repetition 1 plays the nudged schedule.
        let rep1: Vec<u64> = expected.iter().map(|o| 1000 * MS + o).collect();
        assert_eq!(on_times(&tick(&mut fx, 1999 * MS)), rep1);
        // Repetition 2 keeps the nudge: the same onsets, one loop later.
        let rep2: Vec<u64> = expected.iter().map(|o| 2000 * MS + o).collect();
        assert_eq!(on_times(&tick(&mut fx, 2999 * MS)), rep2);
    }

    #[test]
    fn a_swap_exchanges_adjacent_onsets() {
        let seed = find_seed(|r| r.random_range(0..4u32) == 3);
        let mut fx = captured_pair(seed);
        // 64 now sits on 60's onset and vice versa; durations travel
        // with their notes (60 keeps 100ms, 64 keeps 50ms).
        assert_eq!(
            tick(&mut fx, 1999 * MS),
            vec![
                at(1100 * MS, on(64)),
                at(1150 * MS, off(64)),
                at(1300 * MS, on(60)),
                at(1400 * MS, off(60)),
            ]
        );
    }

    #[test]
    fn a_velocity_step_is_twelve_and_persists() {
        // Two upward velocity steps on a single-note phrase.
        let seed = find_seed(|r| draw_vel_up(r, 1) && draw_vel_up(r, 1));
        let mut fx = looper(seed);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 10 * MS, on(60));
        run_timed(&mut fx, 20 * MS, off(60));
        run_timed(&mut fx, 30 * MS, pedal(0));
        // Loop 70ms: repetition 1 at 112, repetition 2 at 124.
        assert_eq!(
            tick(&mut fx, 60 * MS),
            vec![at(40 * MS, von(60, 112)), at(50 * MS, off(60))]
        );
        assert_eq!(
            tick(&mut fx, 130 * MS),
            vec![at(110 * MS, von(60, 124)), at(120 * MS, off(60))]
        );
    }

    #[test]
    fn same_seed_same_warp() {
        let mut a = captured_pair(9);
        let mut b = captured_pair(9);
        for k in 1..60u64 {
            let now = 1000 * MS + k * 137 * MS;
            assert_eq!(tick(&mut a, now), tick(&mut b, now));
        }
    }

    #[test]
    fn pedal_down_mid_loop_silences_and_an_empty_capture_stays_quiet() {
        let seed = find_seed(|r| draw_vel(r, 2));
        let mut fx = captured_pair(seed);
        assert_eq!(ons(&tick(&mut fx, 1120 * MS)), vec![60]);
        // The pedal is consumed; the sounding machine note is cut at it.
        assert_eq!(
            run_timed(&mut fx, 1150 * MS, pedal(127)),
            vec![at(1150 * MS, off(60))]
        );
        // Capturing: the machine stays silent however long we wait.
        assert_eq!(tick(&mut fx, 5000 * MS), vec![]);
        // Nothing was captured, so pedal-up starts no loop.
        assert_eq!(run_timed(&mut fx, 6000 * MS, pedal(0)), vec![]);
        assert_eq!(tick(&mut fx, 60_000 * MS), vec![]);
    }

    #[test]
    fn flush_releases_the_machine_and_the_passthrough() {
        let seed = find_seed(|r| draw_vel(r, 2));
        let mut fx = captured_pair(seed);
        assert_eq!(ons(&tick(&mut fx, 1120 * MS)), vec![60]);
        // The player holds 50 as well.
        run_timed(&mut fx, 1130 * MS, on(50));
        assert_eq!(flush(&mut fx), vec![off(60), off(50)]);
        // The phrase is forgotten: the loop never resumes.
        assert_eq!(tick(&mut fx, 100_000 * MS), vec![]);
    }

    #[test]
    fn a_late_tick_runs_at_most_one_extra_repetition_then_jumps() {
        // Three velocity mutations: repetitions 1, 2, and the jump
        // target all keep the pristine schedule.
        let seed = find_seed(|r| draw_vel(r, 1) && draw_vel(r, 1) && draw_vel(r, 1));
        let mut fx = looper(seed);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 0, on(60));
        run_timed(&mut fx, 10 * MS, off(60));
        run_timed(&mut fx, 30 * MS, pedal(0));
        // Loop 60ms from 30ms. Ten repetitions late: the current one and
        // one more play, then the schedule jumps past now.
        let out = tick(&mut fx, 630 * MS);
        let times: Vec<u64> = out.iter().map(|ev| ev.time).collect();
        assert_eq!(times, vec![30 * MS, 40 * MS, 90 * MS, 100 * MS]);
        // Resynchronized: the next strike lands on the 690ms boundary.
        assert_eq!(on_times(&tick(&mut fx, 700 * MS)), vec![690 * MS]);
    }
}
