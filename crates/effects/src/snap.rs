//! Adaptive quantization: a grid that listens for the player's pulse.

use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx};

use crate::defer::DeferTracker;
use crate::router::push;

/// Two note-ons closer than this are one chord: the later ones neither
/// retune the pulse nor move the phase, they just fall to the grid.
const CHORD_NS: u64 = 30_000_000;

/// Non-chord onsets that pass unquantized while the estimator seeds.
const LEARN_ONSETS: u32 = 3;

/// A silence longer than this many beats re-anchors the grid.
const PAUSE_BEATS: u64 = 4;

/// The beat subdivisions the grid supports.
const DIVISIONS: [u8; 8] = [1, 2, 3, 4, 6, 8, 12, 16];

/// Infer the player's pulse and snap onsets to the grid it implies. Where
/// `Quantize` holds a fixed grid, this one listens: each interval between
/// note-ons is folded by octaves into the tempo window and blended into
/// the running beat estimate, and the grid's phase slides toward the
/// player, so the grid breathes with rubato instead of fighting it.
/// `division` splits each inferred beat into 1, 2, 3, 4, 6, 8, 12, or 16
/// grid steps, `strength` blends between the played time and the grid,
/// and `follow` sets how fast the period and phase chase the player.
///
/// One pulse tracker hears every channel: this is a solo-instrument
/// effect, and two independent players on separate channels will fight
/// over the beat. Note-ons under 30ms apart count as one chord: they do
/// not retune the pulse and fall to the chord's own grid point. A silence
/// longer than four beats keeps the tempo but re-anchors the grid on the
/// next onset, the player's new downbeat. The first three onsets pass
/// unquantized while the estimator seeds.
///
/// Live quantization can only delay: the target is the nearest grid point
/// to the arrival (ties toward earlier), clamped forward, so when the
/// nearest point is already in the past the arrival stands unchanged. The
/// emitted on-time is `arrival + strength * (target - arrival)`, and the
/// matching off carries the same nudge, preserving the played duration.
///
/// Deferred ons follow the ordering rule: the matching off is held to at
/// least 10ms past the emitted on, a retrigger during deferral cuts the
/// pending note first, and `flush` releases whatever sounds. Note-offs
/// with nothing sounding are dropped. Poly pressure follows the sounding
/// note and is dropped otherwise; non-note events pass unchanged.
///
/// Fanout bound: at most 2 outputs per input (a retrigger cut plus the
/// note-on), well under `MAX_FANOUT`.
pub struct Snap {
    division: u8,
    strength: f32,
    follow: f32,
    /// Shortest period the folder accepts: a beat at the upper BPM bound.
    min_period_ns: u64,
    /// Longest period the folder accepts: a beat at the lower BPM bound.
    max_period_ns: u64,
    /// The inferred beat, `None` until the first foldable interval.
    period_ns: Option<u64>,
    /// A timestamp lying on the grid; the whole grid slides with it.
    anchor_ns: u64,
    /// Non-chord onsets seen so far, saturating; the first few learn only.
    onset_count: u32,
    last_onset_ns: Option<u64>,
    tracker: DeferTracker,
    /// The forward nudge applied to each active note-on, so its off can
    /// keep the played duration.
    delta: PerNote<u64>,
}

fn clamp_bpm(bpm: f32) -> f32 {
    if bpm.is_finite() {
        bpm.clamp(30.0, 300.0)
    } else {
        120.0
    }
}

impl Snap {
    /// `division` clamps to the nearest of 1, 2, 3, 4, 6, 8, 12, or 16,
    /// ties toward the coarser grid (5 becomes 4); `strength` and
    /// `follow` clamp to 0.0..=1.0; the BPM bounds clamp to 30.0..=300.0
    /// and swap when reversed.
    pub fn new(division: u8, strength: f32, follow: f32, bpm_lo: f32, bpm_hi: f32) -> Self {
        let division = DIVISIONS
            .into_iter()
            .min_by_key(|d| (d.abs_diff(division), *d))
            .expect("DIVISIONS is non-empty");
        let lo = clamp_bpm(bpm_lo);
        let hi = clamp_bpm(bpm_hi);
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        Self {
            division,
            strength: strength.clamp(0.0, 1.0),
            follow: follow.clamp(0.0, 1.0),
            min_period_ns: (60e9 / f64::from(hi)) as u64,
            max_period_ns: (60e9 / f64::from(lo)) as u64,
            period_ns: None,
            anchor_ns: 0,
            onset_count: 0,
            last_onset_ns: None,
            tracker: DeferTracker::new(),
            delta: PerNote::new(),
        }
    }

    /// The grid step, at least 1ns so the arithmetic never divides by
    /// zero even for degenerate windows.
    fn spacing(&self, period: u64) -> u64 {
        (period / u64::from(self.division)).max(1)
    }

    /// Fold an inter-onset interval into the tempo window by octaves:
    /// double while below it, halve while above it. `None` when no octave
    /// lands inside (the window is narrower than an octave and the
    /// interval misses it) and the estimate stands.
    fn fold(&self, ioi_ns: u64) -> Option<u64> {
        if ioi_ns == 0 {
            return None;
        }
        let mut v = ioi_ns;
        while v < self.min_period_ns {
            v = v.saturating_mul(2);
        }
        while v > self.max_period_ns {
            v /= 2;
        }
        (self.min_period_ns..=self.max_period_ns)
            .contains(&v)
            .then_some(v)
    }

    /// The nearest grid point to `t`, ties toward earlier. The grid is
    /// `anchor + k * spacing` for any integer `k`, negative included.
    fn nearest_grid(&self, t: u64, spacing: u64) -> u64 {
        let spacing = spacing as i64;
        let rel = t as i64 - self.anchor_ns as i64;
        let k = rel.div_euclid(spacing);
        let r = rel.rem_euclid(spacing);
        let k = if 2 * r <= spacing { k } else { k + 1 };
        (self.anchor_ns as i64 + k * spacing).max(0) as u64
    }

    /// Teach the estimator one non-chord onset: adopt or blend the folded
    /// inter-onset interval, then slide the grid's phase toward the
    /// player.
    fn observe_onset(&mut self, t: u64) {
        let Some(last) = self.last_onset_ns else {
            // The first onset anchors the grid on the player's downbeat.
            self.anchor_ns = t;
            return;
        };
        let ioi = t.saturating_sub(last);
        if let Some(period) = self.period_ns
            && ioi > PAUSE_BEATS * period
        {
            // A pause: the pulse survives, but the new phrase starts on
            // the player's downbeat.
            self.anchor_ns = t;
            return;
        }
        if let Some(folded) = self.fold(ioi) {
            self.period_ns = Some(match self.period_ns {
                None => folded,
                Some(p) => {
                    let blended = p as f64 + f64::from(self.follow) * (folded as f64 - p as f64);
                    blended.round() as u64
                }
            });
        }
        if let Some(period) = self.period_ns {
            let spacing = self.spacing(period);
            let nearest = self.nearest_grid(t, spacing);
            let error = t as i64 - nearest as i64;
            let shift = (f64::from(self.follow) * error as f64).round() as i64;
            let mut anchor = self.anchor_ns as i64 + shift;
            if anchor < 0 {
                // The grid is anchor + k * spacing: wrap one step forward.
                anchor += spacing as i64;
            }
            self.anchor_ns = anchor.max(0) as u64;
            // Hop the anchor to the grid point nearest this onset: an
            // identity on the grid, but it keeps the anchor beside the
            // music, so a period correction bends the grid locally
            // instead of levering it around a distant origin.
            self.anchor_ns = self.nearest_grid(t, spacing);
        }
    }

    /// The emitted on-time for a note-on arriving at `t`, teaching the
    /// estimator along the way.
    fn on_time(&mut self, t: u64) -> u64 {
        // Whether this onset snaps is decided before it teaches the
        // estimator: the learning onsets pass while they seed the pulse.
        let learning = self.onset_count < LEARN_ONSETS;
        let chord = self
            .last_onset_ns
            .is_some_and(|last| t.saturating_sub(last) < CHORD_NS);
        if !chord {
            self.observe_onset(t);
            self.last_onset_ns = Some(t);
            self.onset_count = self.onset_count.saturating_add(1);
        }
        let Some(period) = self.period_ns else {
            return t;
        };
        if learning {
            return t;
        }
        let spacing = self.spacing(period);
        let target = self.nearest_grid(t, spacing).max(t);
        let delta = (f64::from(self.strength) * (target - t) as f64).round() as u64;
        t.saturating_add(delta)
    }
}

impl Effect for Snap {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match ev.kind {
            EventKind::NoteOn { ch, key, .. } => {
                let on_time = self.on_time(ev.time);
                self.delta.set(ch, key, on_time - ev.time);
                self.tracker.note_on(ev, Some(on_time), out, cx);
            }
            EventKind::NoteOff { ch, key, .. } => {
                let extra = self.delta.take(ch, key);
                self.tracker.note_off(ev, extra, out, cx);
            }
            EventKind::PolyPressure { .. } => self.tracker.poly_pressure(ev, out, cx),
            _ => push(out, cx, *ev),
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        self.delta = PerNote::new();
        self.tracker.flush(out, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run_timed};

    const MS: u64 = 1_000_000;

    /// Feed note-ons at the given times, each on its own key so no
    /// retrigger cuts appear, and collect the emitted events.
    fn train(fx: &mut Snap, times: &[u64]) -> Vec<Event> {
        let mut all = Vec::new();
        for (i, &t) in times.iter().enumerate() {
            all.extend(run_timed(fx, t, on(40 + (i % 48) as u8)));
        }
        all
    }

    /// Establish a locked pulse: onsets one `period_ms` apart from zero,
    /// enough to leave the learning phase with the grid anchored at zero.
    /// Each note is released after 100ms, so nothing lingers for flush.
    fn lock_pulse(fx: &mut Snap, period_ms: u64) {
        for i in 0..4u64 {
            let t = i * period_ms * MS;
            run_timed(fx, t, on(40 + i as u8));
            run_timed(fx, t + 100 * MS, off(40 + i as u8));
        }
    }

    /// Distance from `t` to the nearest multiple of `grid`.
    fn grid_dist(t: u64, grid: u64) -> u64 {
        let r = t % grid;
        r.min(grid - r)
    }

    #[test]
    fn the_first_onsets_pass_while_the_pulse_seeds() {
        let mut fx = Snap::new(2, 1.0, 0.5, 60.0, 240.0);
        let times = [0, 480 * MS, 1010 * MS];
        let out = train(&mut fx, &times);
        assert_eq!(out.len(), 3);
        for (ev, &t) in out.iter().zip(&times) {
            assert_eq!(ev.time, t, "a learning onset must pass unmoved");
        }
    }

    #[test]
    fn steady_jitter_locks_onto_the_grid() {
        // Onsets every 500ms with alternating +-15ms jitter, division 2.
        // Early arrivals lock to within a few ms of the 250ms grid, far
        // tighter than the 15ms input jitter; late arrivals pass
        // unchanged, since a live quantizer can only delay, and are
        // never made worse.
        let mut fx = Snap::new(2, 1.0, 0.25, 60.0, 240.0);
        for i in 0..24u64 {
            let jitter: i64 = match i {
                0 => 0,
                odd if odd % 2 == 1 => 15,
                _ => -15,
            };
            let t = ((i * 500) as i64 + jitter) as u64 * MS;
            let out = run_timed(&mut fx, t, on(40 + i as u8));
            assert_eq!(out.len(), 1);
            let emitted = out[0].time;
            assert!(emitted >= t, "forward only: {emitted} < {t}");
            if i >= 8 {
                let dist = grid_dist(emitted, 250 * MS);
                assert!(
                    dist <= 15 * MS,
                    "onset {i}: emitted {emitted} is {dist}ns off grid, worse than the input"
                );
                if i % 2 == 0 {
                    assert!(
                        dist <= 5 * MS,
                        "early onset {i}: emitted {emitted} is {dist}ns off grid"
                    );
                }
            }
        }
    }

    #[test]
    fn the_grid_follows_rubato() {
        // The player accelerates from 500ms to 450ms intervals over
        // eight notes; the grid must chase the new pulse rather than
        // hold the player to the old one.
        let mut fx = Snap::new(1, 1.0, 0.5, 60.0, 240.0);
        let mut times = vec![0u64];
        for ioi in [500, 493, 486, 479, 472, 465, 458, 450u64] {
            times.push(times.last().unwrap() + ioi * MS);
        }
        let out = train(&mut fx, &times);
        assert_eq!(out.len(), times.len());
        for (ev, &t) in out.iter().zip(&times) {
            let delta = ev.time - t;
            assert!(delta <= 20 * MS, "a frozen grid would defer far more");
        }
        let last_ioi = out[out.len() - 1].time - out[out.len() - 2].time;
        assert!(
            (440 * MS..480 * MS).contains(&last_ioi),
            "the emitted pulse must approach the new 450ms tempo, got {last_ioi}"
        );
    }

    #[test]
    fn sixteenth_runs_fold_into_the_beat() {
        // Quarters establish 120 BPM, then a sixteenth run at 125ms
        // intervals. Folding doubles each 125ms interval into the
        // 90-180 BPM window, so the period stays a 500ms beat; had the
        // sixteenths retuned the beat to their own rate, the early probe
        // would sit almost on the finer grid and barely move.
        let mut fx = Snap::new(4, 1.0, 0.25, 90.0, 180.0);
        let times: Vec<u64> = [0, 500, 1000, 1500, 1625, 1750, 1875, 2000, 2125]
            .iter()
            .map(|&t| t * MS)
            .collect();
        let out = train(&mut fx, &times);
        for (ev, &t) in out.iter().zip(&times) {
            assert_eq!(ev.time, t, "on-grid onsets pass unmoved");
        }
        let probe = run_timed(&mut fx, 2325 * MS, on(60));
        assert_eq!(probe, vec![at(2325 * MS + 28_125_000, on(60))]);
    }

    #[test]
    fn chord_members_share_their_grid_point() {
        // follow 0 freezes the learned grid at multiples of 250ms. An
        // early chord lands as one block on the grid point ahead.
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        let head = run_timed(&mut fx, 1960 * MS, on(60));
        let second = run_timed(&mut fx, 1966 * MS, on(64));
        let third = run_timed(&mut fx, 1971 * MS, on(67));
        assert_eq!(head, vec![at(2000 * MS, on(60))]);
        assert_eq!(second, vec![at(2000 * MS, on(64))]);
        assert_eq!(third, vec![at(2000 * MS, on(67))]);
    }

    #[test]
    fn a_chord_does_not_retune_the_pulse() {
        // Three on-grid note-ons within 10ms: the tiny intervals must
        // not fold into the tempo window and crash the beat estimate.
        let mut fx = Snap::new(2, 1.0, 0.5, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        run_timed(&mut fx, 2000 * MS, on(60));
        run_timed(&mut fx, 2004 * MS, on(64));
        run_timed(&mut fx, 2008 * MS, on(67));
        // One beat after the chord's onset: still exactly on the pulse.
        let next = run_timed(&mut fx, 2500 * MS, on(72));
        assert_eq!(next, vec![at(2500 * MS, on(72))]);
    }

    #[test]
    fn a_pause_reanchors_the_grid_on_the_new_downbeat() {
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        // Five seconds of silence, then a phrase far off the old grid:
        // its first onset passes as the new downbeat.
        let first = run_timed(&mut fx, 7137 * MS, on(60));
        assert_eq!(first, vec![at(7137 * MS, on(60))]);
        // The kept 500ms pulse now runs from 7137ms: 7627ms is nearest
        // the 7637ms grid point.
        let second = run_timed(&mut fx, 7627 * MS, on(62));
        assert_eq!(second, vec![at(7637 * MS, on(62))]);
    }

    #[test]
    fn division_three_makes_triplet_spacing() {
        // A 600ms beat divided by 3: grid steps of 200ms.
        let mut fx = Snap::new(3, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 600);
        let probe = run_timed(&mut fx, 1950 * MS, on(60));
        assert_eq!(probe, vec![at(2000 * MS, on(60))]);
    }

    #[test]
    fn division_clamps_to_the_nearest_supported() {
        // 5 ties between 4 and 6 and takes the coarser 4: steps of
        // 150ms on a 600ms beat, so 2030ms rounds up to 2100ms. A
        // division of 6 would leave it (nearest step 2000ms, behind).
        let mut fx = Snap::new(5, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 600);
        let probe = run_timed(&mut fx, 2030 * MS, on(60));
        assert_eq!(probe, vec![at(2100 * MS, on(60))]);
        // 0 clamps to 1: whole 600ms beats, so 2150ms rounds to 2400ms.
        let mut fx = Snap::new(0, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 600);
        let probe = run_timed(&mut fx, 2150 * MS, on(60));
        assert_eq!(probe, vec![at(2400 * MS, on(60))]);
        // 100 clamps to 16: steps of 37.5ms, so 2020ms rounds to 2025ms.
        let mut fx = Snap::new(100, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 600);
        let probe = run_timed(&mut fx, 2020 * MS, on(60));
        assert_eq!(probe, vec![at(2025 * MS, on(60))]);
    }

    #[test]
    fn strength_blends_toward_the_target() {
        // 1650ms is 100ms short of the 1750ms grid point.
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        let probe = run_timed(&mut fx, 1650 * MS, on(60));
        assert_eq!(probe, vec![at(1750 * MS, on(60))]);
        let mut fx = Snap::new(2, 0.5, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        let probe = run_timed(&mut fx, 1650 * MS, on(60));
        assert_eq!(probe, vec![at(1700 * MS, on(60))]);
        // Out-of-range strength clamps into 0..=1.
        let mut fx = Snap::new(2, 7.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        let probe = run_timed(&mut fx, 1650 * MS, on(60));
        assert_eq!(probe, vec![at(1750 * MS, on(60))]);
    }

    #[test]
    fn strength_zero_never_moves_but_still_tracks() {
        // The estimator runs on the same code path either way; with
        // strength 0 the correction is scaled to nothing and every
        // onset passes at its own time, jitter and all.
        let mut fx = Snap::new(2, 0.0, 0.5, 60.0, 240.0);
        let times: Vec<u64> = (0..12u64)
            .map(|i| i * 500 * MS + (i % 3) * 11 * MS)
            .collect();
        let out = train(&mut fx, &times);
        for (ev, &t) in out.iter().zip(&times) {
            assert_eq!(ev.time, t);
        }
        let probe = run_timed(&mut fx, 6650 * MS, on(60));
        assert_eq!(probe, vec![at(6650 * MS, on(60))]);
    }

    #[test]
    fn the_off_carries_the_note_ons_delta() {
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        // The on defers 1710ms -> 1750ms (40ms); a 300ms-held note ends
        // 340ms after the on's arrival, its played duration intact.
        let probe = run_timed(&mut fx, 1710 * MS, on(60));
        assert_eq!(probe, vec![at(1750 * MS, on(60))]);
        let release = run_timed(&mut fx, 2010 * MS, off(60));
        assert_eq!(release, vec![at(2050 * MS, off(60))]);
    }

    #[test]
    fn the_off_never_beats_the_deferred_on() {
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        // The on defers 1710ms -> 1750ms; the player releases almost
        // immediately. The off is held to 10ms past the emitted on.
        run_timed(&mut fx, 1710 * MS, on(60));
        let release = run_timed(&mut fx, 1712 * MS, off(60));
        assert_eq!(release, vec![at(1760 * MS, off(60))]);
    }

    #[test]
    fn retrigger_during_deferral_cuts_first() {
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        // Pending at 1750ms when the key strikes again at 1715ms: the
        // cut lands 10ms past the pending on, the new on raised to it.
        run_timed(&mut fx, 1710 * MS, on(60));
        assert_eq!(
            run_timed(&mut fx, 1715 * MS, on(60)),
            vec![at(1760 * MS, off(60)), at(1760 * MS, on(60))]
        );
    }

    #[test]
    fn no_emitted_time_precedes_its_arrival() {
        let mut fx = Snap::new(2, 1.0, 0.4, 60.0, 240.0);
        for i in 0..24u64 {
            let jitter = (i * 37) % 23; // 0..22ms, deterministic
            let t = (i * 500 + jitter) * MS;
            let out = run_timed(&mut fx, t, on(40 + i as u8));
            for ev in &out {
                assert!(ev.time >= t, "emitted {} before arrival {t}", ev.time);
            }
        }
    }

    #[test]
    fn orphan_note_off_is_dropped() {
        let mut fx = Snap::new(2, 1.0, 0.5, 60.0, 240.0);
        assert_eq!(run_timed(&mut fx, 0, off(60)), vec![]);
    }

    #[test]
    fn flush_releases_past_the_pending_on() {
        let mut fx = Snap::new(2, 1.0, 0.0, 60.0, 240.0);
        lock_pulse(&mut fx, 500);
        run_timed(&mut fx, 1710 * MS, on(60));
        let cx = ProcCx::at(1715 * MS);
        let mut out = EventBuf::new();
        fx.flush(&mut out, &cx);
        assert_eq!(out.as_slice(), &[at(1760 * MS, off(60))]);
    }

    #[test]
    fn non_note_events_pass_unchanged() {
        let mut fx = Snap::new(2, 1.0, 0.5, 60.0, 240.0);
        let pedal = EventKind::ControlChange {
            ch: 0,
            cc: 64,
            value: 127,
        };
        assert_eq!(run_timed(&mut fx, 42, pedal), vec![at(42, pedal)]);
    }
}
