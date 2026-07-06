//! Webern's palindrome: a pedal-captured phrase plays back once, reversed.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx, Timestamp};

use crate::router::push;

/// Fixed recording capacity.
const CAPACITY: usize = 32;

/// A pedal value of 64 or higher counts as down.
const PEDAL_DOWN: u8 = 64;

/// A very late tick catches up at most this many note-ons; the rest of
/// the run waits for the next tick.
const MAX_CATCHUP: usize = 8;

/// One recorded note. During capture the times are relative to the
/// capture start; at pedal-up they are rewritten in place to the
/// mirrored, speed-scaled playback timeline.
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

/// Webern's palindrome as a pedal buffer. The configured control pedal
/// (its CC number on any channel; a value of 64 or higher is down) is the
/// capture control and is CONSUMED: the DAW never sees it. Pedal down
/// stops any running playback (note-offs for everything the machine has
/// sounding) and begins a capture; pedal up ends it. While the pedal is
/// down, up to 32 notes are recorded as (key, velocity, onset, duration)
/// relative to the capture start: notes beyond capacity are not recorded,
/// and a note still held at pedal-up gets its duration capped there. The
/// player's notes pass through unchanged at all times, pedal down or up:
/// the machine adds a voice and never consumes notes.
///
/// If at least one note was captured, pedal-up plays the phrase back
/// exactly once in retrograde: the timeline reverses, so a note that
/// occupied `[a, b]` within a capture of length `L` now occupies
/// `[(L - b) / speed, (L - a) / speed]` after pedal-up. The last-played
/// note sounds first, inter-onset gaps and durations are mirrored, and
/// the whole run is scaled by `1 / speed` (nanoseconds truncate). After
/// the run the machine stays silent until the next capture.
///
/// Playback runs from `tick` against target timestamps: a note-on is
/// emitted once `now` reaches its mirrored onset, stamped with the
/// target, and remembered in the machine's bookkeeping so the note-off
/// follows when the scaled duration elapses. Nothing is emitted ahead, so
/// pedal-down and `flush` can always silence exactly what is sounding.
///
/// Ticks may be late: a very late tick catches up at most 8 note-ons (a
/// finite run is deferred, not dropped), so the remainder of the phrase,
/// note-offs included, resumes on the next tick with its original target
/// stamps. A tick that fills the output buffer to within a pair of
/// events also stops there and resumes on the next tick.
///
/// `flush` releases everything the machine has sounding plus one note-off
/// per outstanding pass-through note-on, then forgets the phrase.
///
/// Fanout bound: a tick emits at most 8 note-ons plus the pending
/// note-offs, at most 40 events; `process` emits at most the pass-through
/// event, or up to 32 note-offs on the pedal-down that stops a run.
pub struct RetrogradeBuffer {
    pedal_cc: u8,
    speed: f64,
    pedal_down: bool,
    capturing: bool,
    capture_start: Timestamp,
    notes: [CapturedNote; CAPACITY],
    len: usize,
    playing: bool,
    /// The pedal-up instant playback times are measured from.
    play_start: Timestamp,
    /// Note indices in mirrored-onset order.
    order: [u8; CAPACITY],
    /// Cursor into `order`: the next note-on still owed.
    next_pos: usize,
    /// Per phrase slot: the sounding instance's note-off due time.
    sounding: [Option<Timestamp>; CAPACITY],
    /// Held pass-through note-on counts, wound down by `flush`.
    held: PerNote<u8>,
}

impl RetrogradeBuffer {
    /// `speed` is clamped to 0.25..=4.0 and `pedal_cc` to 0..=127.
    pub fn new(pedal_cc: u8, speed: f32) -> Self {
        Self {
            pedal_cc: pedal_cc.min(127),
            speed: speed.clamp(0.25, 4.0) as f64,
            pedal_down: false,
            capturing: false,
            capture_start: 0,
            notes: [CapturedNote::default(); CAPACITY],
            len: 0,
            playing: false,
            play_start: 0,
            order: [0; CAPACITY],
            next_pos: 0,
            sounding: [None; CAPACITY],
            held: PerNote::new(),
        }
    }

    /// Stop playback, releasing every machine note still sounding.
    fn stop(&mut self, time: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        self.playing = false;
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

    /// Pedal up: cap held notes, mirror and scale the timeline in place,
    /// and start the one reversed run.
    fn end_capture(&mut self, time: Timestamp) {
        self.capturing = false;
        if self.len == 0 {
            return;
        }
        let span = time.saturating_sub(self.capture_start);
        let speed = self.speed;
        let scale = |x: u64| (x as f64 / speed) as u64;
        for note in self.notes[..self.len].iter_mut() {
            if note.open {
                note.duration_ns = span.saturating_sub(note.onset_ns);
                note.open = false;
            }
            // [a, b] reverses to [L - b, L - a], scaled by 1 / speed.
            let end = note.onset_ns + note.duration_ns;
            note.onset_ns = scale(span.saturating_sub(end));
            note.duration_ns = scale(note.duration_ns);
        }
        // Insertion sort of the indices by mirrored onset (ties keep
        // recording order): fixed arrays, no allocation.
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
        self.playing = true;
        self.play_start = time;
        self.next_pos = 0;
        self.sounding = [None; CAPACITY];
    }
}

impl Effect for RetrogradeBuffer {
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
                if self.capturing && self.len < CAPACITY {
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
        if !self.playing {
            return;
        }
        let mut ons = 0;
        loop {
            // Never split a strike across a full buffer; the rest of the
            // run waits for the next tick.
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
            let on_due = (self.next_pos < self.len)
                .then(|| self.play_start + self.notes[self.order[self.next_pos] as usize].onset_ns);
            // Chronological merge; offs win ties.
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
                    if ons == MAX_CATCHUP {
                        // A very late tick: the rest of the run resumes
                        // on the next tick, target stamps intact.
                        break;
                    }
                    let idx = self.order[self.next_pos] as usize;
                    let n = self.notes[idx];
                    let strike = EventKind::NoteOn {
                        ch: n.ch,
                        key: n.key,
                        vel: n.vel,
                    };
                    push(out, cx, Event::new(due, strike));
                    self.sounding[idx] = Some(due.saturating_add(n.duration_ns));
                    self.next_pos += 1;
                    ons += 1;
                }
                Some(_) => break,
                None => {
                    // Every note-on is out; once the offs drain, the run
                    // is over and the machine falls silent.
                    if self.sounding[..self.len].iter().all(Option::is_none) {
                        self.playing = false;
                    }
                    break;
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

    fn machine(speed: f32) -> RetrogradeBuffer {
        RetrogradeBuffer::new(64, speed)
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

    /// Pedal at 0, notes 60 over [100ms, 300ms], 64 over [400ms, 450ms],
    /// and 67 over [500ms, 900ms], pedal up at 1s.
    fn capture_three(speed: f32) -> RetrogradeBuffer {
        let mut fx = machine(speed);
        assert_eq!(run_timed(&mut fx, 0, pedal(127)), vec![]);
        assert_eq!(
            run_timed(&mut fx, 100 * MS, on(60)),
            vec![at(100 * MS, on(60))],
            "notes pass through during capture"
        );
        run_timed(&mut fx, 300 * MS, off(60));
        run_timed(&mut fx, 400 * MS, on(64));
        run_timed(&mut fx, 450 * MS, off(64));
        run_timed(&mut fx, 500 * MS, on(67));
        run_timed(&mut fx, 900 * MS, off(67));
        assert_eq!(run_timed(&mut fx, 1000 * MS, pedal(0)), vec![]);
        fx
    }

    #[test]
    fn the_pedal_is_consumed_and_everything_else_passes() {
        let mut fx = machine(1.0);
        assert_eq!(run_timed(&mut fx, 0, pedal(127)), vec![]);
        // The pedal number matches on any channel.
        let other_ch = EventKind::ControlChange {
            ch: 9,
            cc: 64,
            value: 0,
        };
        assert_eq!(run_timed(&mut fx, 1, other_ch), vec![]);
        let wheel = EventKind::ControlChange {
            ch: 0,
            cc: 1,
            value: 5,
        };
        assert_eq!(run_timed(&mut fx, 2, wheel), vec![at(2, wheel)]);
        // A custom pedal number frees CC 64 to pass through.
        let mut fx = RetrogradeBuffer::new(20, 1.0);
        assert_eq!(run_timed(&mut fx, 0, pedal(127)), vec![at(0, pedal(127))]);
        let capture = EventKind::ControlChange {
            ch: 0,
            cc: 20,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 1, capture), vec![]);
    }

    #[test]
    fn the_mirror_of_a_three_note_phrase_at_speed_one() {
        let mut fx = capture_three(1.0);
        // [100,300] -> [700,900]; [400,450] -> [550,600]; [500,900] ->
        // [100,500]: the last-played note sounds first.
        assert_eq!(
            tick(&mut fx, 2000 * MS),
            vec![
                at(1100 * MS, on(67)),
                at(1500 * MS, off(67)),
                at(1550 * MS, on(64)),
                at(1600 * MS, off(64)),
                at(1700 * MS, on(60)),
                at(1900 * MS, off(60)),
            ]
        );
        // The run plays once; silence follows.
        assert_eq!(tick(&mut fx, 10_000 * MS), vec![]);
    }

    #[test]
    fn speed_two_halves_the_mirrored_timeline() {
        let mut fx = capture_three(2.0);
        assert_eq!(
            tick(&mut fx, 2000 * MS),
            vec![
                at(1050 * MS, on(67)),
                at(1250 * MS, off(67)),
                at(1275 * MS, on(64)),
                at(1300 * MS, off(64)),
                at(1350 * MS, on(60)),
                at(1450 * MS, off(60)),
            ]
        );
    }

    #[test]
    fn playback_lands_on_target_timestamps() {
        let mut fx = capture_three(1.0);
        assert_eq!(tick(&mut fx, 1099 * MS), vec![]);
        assert_eq!(tick(&mut fx, 1100 * MS), vec![at(1100 * MS, on(67))]);
        assert_eq!(tick(&mut fx, 1499 * MS), vec![]);
        // A late tick stamps the target, not now.
        assert_eq!(tick(&mut fx, 1520 * MS), vec![at(1500 * MS, off(67))]);
    }

    #[test]
    fn a_note_held_at_pedal_up_is_capped_and_sounds_first() {
        let mut fx = machine(1.0);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 200 * MS, on(60));
        run_timed(&mut fx, 1000 * MS, pedal(0));
        // [200, 1000] reverses to [0, 800]: it opens the run at pedal-up
        // with its capped 800ms length.
        assert_eq!(tick(&mut fx, 1000 * MS), vec![at(1000 * MS, on(60))]);
        assert_eq!(tick(&mut fx, 1799 * MS), vec![]);
        assert_eq!(tick(&mut fx, 1800 * MS), vec![at(1800 * MS, off(60))]);
    }

    #[test]
    fn notes_during_playback_pass_through_without_disturbing_the_run() {
        let mut fx = capture_three(1.0);
        assert_eq!(
            run_timed(&mut fx, 1050 * MS, on(50)),
            vec![at(1050 * MS, on(50))]
        );
        // The run is unmoved: the mirrored phrase still starts at 1100ms.
        assert_eq!(tick(&mut fx, 1100 * MS), vec![at(1100 * MS, on(67))]);
    }

    #[test]
    fn a_thirty_third_note_is_not_recorded() {
        let mut fx = machine(1.0);
        run_timed(&mut fx, 0, pedal(127));
        for i in 0..33u64 {
            let t = (i + 1) * MS;
            run_timed(&mut fx, t, on(30 + i as u8));
            run_timed(&mut fx, t + 500_000, off(30 + i as u8));
        }
        run_timed(&mut fx, 100 * MS, pedal(0));
        let mut keys = Vec::new();
        for k in 0..200u64 {
            keys.extend(ons(&tick(&mut fx, 100 * MS + k * MS)));
        }
        // The 33rd note (key 62) never made it in; the newest recorded
        // note comes back first.
        assert_eq!(keys.len(), 32);
        assert!(!keys.contains(&62), "{keys:?}");
        assert_eq!(keys[0], 61);
        assert_eq!(keys[31], 30);
    }

    #[test]
    fn pedal_down_mid_playback_silences_cleanly() {
        let mut fx = capture_three(1.0);
        assert_eq!(tick(&mut fx, 1100 * MS), vec![at(1100 * MS, on(67))]);
        // The pedal is consumed; the sounding machine note is cut at it.
        assert_eq!(
            run_timed(&mut fx, 1200 * MS, pedal(127)),
            vec![at(1200 * MS, off(67))]
        );
        assert_eq!(tick(&mut fx, 5000 * MS), vec![]);
        // The new capture replaces the old phrase entirely.
        run_timed(&mut fx, 5100 * MS, on(72));
        run_timed(&mut fx, 5200 * MS, off(72));
        run_timed(&mut fx, 5300 * MS, pedal(0));
        // Capture ran 1200..5300 (L = 4100ms); [3900, 4000] mirrors to
        // [100, 200] after the 5300ms pedal-up.
        assert_eq!(
            tick(&mut fx, 6000 * MS),
            vec![at(5400 * MS, on(72)), at(5500 * MS, off(72))]
        );
    }

    #[test]
    fn flush_releases_the_machine_and_the_passthrough() {
        let mut fx = capture_three(1.0);
        assert_eq!(tick(&mut fx, 1100 * MS), vec![at(1100 * MS, on(67))]);
        // The player holds 50 as well.
        run_timed(&mut fx, 1150 * MS, on(50));
        assert_eq!(flush(&mut fx), vec![off(67), off(50)]);
        assert_eq!(tick(&mut fx, 100_000 * MS), vec![]);
    }

    #[test]
    fn a_late_tick_emits_at_most_eight_notes_and_defers_the_rest() {
        let mut fx = machine(1.0);
        run_timed(&mut fx, 0, pedal(127));
        for i in 0..12u64 {
            let t = (i + 1) * 10 * MS;
            run_timed(&mut fx, t, on(40 + i as u8));
            run_timed(&mut fx, t + 5 * MS, off(40 + i as u8));
        }
        run_timed(&mut fx, 200 * MS, pedal(0));
        // Everything is long overdue: the first tick catches up eight
        // notes with their offs, the next the remaining four; nothing is
        // lost.
        let out = tick(&mut fx, 10_000 * MS);
        assert_eq!(ons(&out).len(), 8);
        assert_eq!(out.len(), 16);
        assert_eq!(ons(&out)[0], 51, "reversed: the last note first");
        let out = tick(&mut fx, 10_000 * MS);
        assert_eq!(ons(&out), vec![43, 42, 41, 40]);
        assert_eq!(out.len(), 8);
        assert_eq!(tick(&mut fx, 10_000 * MS), vec![]);
    }

    #[test]
    fn speed_clamps() {
        // 0.01 clamps to 0.25: four times slower. [800, 900] in a 1s
        // capture mirrors to [100, 200], scaled to [400, 800].
        let mut fx = machine(0.01);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 800 * MS, on(60));
        run_timed(&mut fx, 900 * MS, off(60));
        run_timed(&mut fx, 1000 * MS, pedal(0));
        assert_eq!(tick(&mut fx, 1400 * MS), vec![at(1400 * MS, on(60))]);
        assert_eq!(tick(&mut fx, 1800 * MS), vec![at(1800 * MS, off(60))]);
        // 100 clamps to 4: four times faster, [25, 50].
        let mut fx = machine(100.0);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 800 * MS, on(60));
        run_timed(&mut fx, 900 * MS, off(60));
        run_timed(&mut fx, 1000 * MS, pedal(0));
        assert_eq!(tick(&mut fx, 1025 * MS), vec![at(1025 * MS, on(60))]);
        assert_eq!(tick(&mut fx, 1050 * MS), vec![at(1050 * MS, off(60))]);
    }

    #[test]
    fn an_empty_capture_stays_silent() {
        let mut fx = machine(1.0);
        run_timed(&mut fx, 0, pedal(127));
        run_timed(&mut fx, 1000 * MS, pedal(0));
        assert_eq!(tick(&mut fx, 10_000 * MS), vec![]);
    }

    #[test]
    fn the_machine_keeps_the_players_channel_and_velocity() {
        let mut fx = machine(1.0);
        run_timed(&mut fx, 0, pedal(127));
        let strike = EventKind::NoteOn {
            ch: 5,
            key: 60,
            vel: 90,
        };
        run_timed(&mut fx, 100 * MS, strike);
        run_timed(
            &mut fx,
            200 * MS,
            EventKind::NoteOff {
                ch: 5,
                key: 60,
                vel: 0,
            },
        );
        run_timed(&mut fx, 1000 * MS, pedal(0));
        let out = tick(&mut fx, 2000 * MS);
        assert_eq!(out[0], at(1800 * MS, strike));
        assert_eq!(
            out[1],
            at(
                1900 * MS,
                EventKind::NoteOff {
                    ch: 5,
                    key: 60,
                    vel: 0
                }
            )
        );
    }
}
