//! Ligeti halos: under the pedal, every note excites its neighbors.

use miditool_core::event::CC_SUSTAIN;
use miditool_core::{Effect, Event, EventBuf, EventKind, ProcCx, Sieve};

use crate::router::push;

/// Sympathetic resonance under the sustain pedal, after the held-pedal
/// haze of Ligeti's piano writing: CC64 is tracked per channel from the
/// stream (a value of 64 or higher is down; the CC itself passes
/// through). While the pedal is down on a note's channel, each note-on
/// passes unchanged and deposits a halo around itself: the `width`
/// nearest neighbor keys on each side (the nearest sieve members when a
/// sieve is given; the played key itself is never a neighbor), each a
/// self-contained pair via `push_pair`, on at the note's time with
/// velocity `round(vel * level)` (at least 1) and off `decay_ns` later.
/// Neighbors are emitted per distance ring, below before above; a side
/// that runs off the keyboard contributes fewer neighbors.
///
/// Lifting the pedal does nothing retroactive: deposited halos decay on
/// their own schedule. With the pedal up the effect is a pass. Note-offs,
/// poly pressure, and all other events pass untouched, and `flush` emits
/// nothing, since every halo off is scheduled at deposit time.
///
/// Fanout bound: at most `1 + 4 * width` outputs per input (the passed
/// note plus `2 * width` self-contained pairs), and `width` is clamped
/// to 6, so 25 events, well under `MAX_FANOUT`.
pub struct ResonanceHalo {
    width: u8,
    level: f32,
    decay_ns: u64,
    sieve: Option<Sieve>,
    /// Channels whose sustain pedal is down, one bit per channel.
    sustain_down: u16,
}

impl ResonanceHalo {
    /// `width` is clamped to 1..=6 and `level` to 0.0..=1.0.
    pub fn new(width: u8, level: f32, decay_ns: u64, sieve: Option<Sieve>) -> Self {
        Self {
            width: width.clamp(1, 6),
            level: level.clamp(0.0, 1.0),
            decay_ns,
            sieve,
            sustain_down: 0,
        }
    }

    fn is_member(&self, key: u8) -> bool {
        self.sieve.is_none_or(|sieve| sieve.contains(key))
    }

    /// The nearest halo key strictly above `from`, if any.
    fn above(&self, from: u8) -> Option<u8> {
        (from as u16 + 1..=127)
            .map(|k| k as u8)
            .find(|&k| self.is_member(k))
    }

    /// The nearest halo key strictly below `from`, if any.
    fn below(&self, from: u8) -> Option<u8> {
        (0..from).rev().find(|&k| self.is_member(k))
    }

    /// Deposit one halo neighbor as a self-contained pair.
    fn deposit(&self, key: u8, ch: u8, vel: u8, time: u64, out: &mut EventBuf, cx: &ProcCx) {
        let strike = EventKind::NoteOn { ch, key, vel };
        let release = EventKind::NoteOff { ch, key, vel: 0 };
        // Pushed as a pair so truncation can never keep the on and drop
        // the off, which would leave the halo stuck.
        cx.push_pair(
            out,
            Event::new(time, strike),
            Event::new(time.saturating_add(self.decay_ns), release),
        );
    }
}

impl Effect for ResonanceHalo {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        if let EventKind::ControlChange {
            ch,
            cc: CC_SUSTAIN,
            value,
        } = ev.kind
        {
            if value >= 64 {
                self.sustain_down |= 1 << ch;
            } else {
                self.sustain_down &= !(1 << ch);
            }
        }
        push(out, cx, *ev);
        let EventKind::NoteOn { ch, key, vel } = ev.kind else {
            return;
        };
        if self.sustain_down & (1 << ch) == 0 {
            return;
        }
        // Velocity 0 would read as a note-off on the wire; never emit it.
        let halo_vel = (vel as f32 * self.level).round().clamp(1.0, 127.0) as u8;
        let mut down = Some(key);
        let mut up = Some(key);
        for _ in 0..self.width {
            down = down.and_then(|k| self.below(k));
            if let Some(k) = down {
                self.deposit(k, ch, halo_vel, ev.time, out, cx);
            }
            up = up.and_then(|k| self.above(k));
            if let Some(k) = up {
                self.deposit(k, ch, halo_vel, ev.time, out, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, off, on, run, run_timed};

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

    #[test]
    fn pedal_up_is_a_pass() {
        let mut fx = ResonanceHalo::new(2, 0.5, 1_000, None);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn pedal_down_deposits_the_halo() {
        let mut fx = ResonanceHalo::new(2, 0.5, 1_000, None);
        assert_eq!(run(&mut fx, pedal(127)), vec![pedal(127)]);
        assert_eq!(
            run_timed(&mut fx, 100, on(60)),
            vec![
                at(100, on(60)),
                at(100, von(59, 50)),
                at(1_100, off(59)),
                at(100, von(61, 50)),
                at(1_100, off(61)),
                at(100, von(58, 50)),
                at(1_100, off(58)),
                at(100, von(62, 50)),
                at(1_100, off(62)),
            ]
        );
        // The player's note-off passes alone: the halo brings its own.
        assert_eq!(run_timed(&mut fx, 200, off(60)), vec![at(200, off(60))]);
    }

    #[test]
    fn a_sieve_filters_the_neighbors() {
        let sieve = Sieve::parse("12@0").unwrap();
        let mut fx = ResonanceHalo::new(2, 1.0, 500, Some(sieve));
        run(&mut fx, pedal(127));
        // Octaves around middle C, nearest ring first, below before above.
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![
                at(0, on(60)),
                at(0, von(48, 100)),
                at(500, off(48)),
                at(0, von(72, 100)),
                at(500, off(72)),
                at(0, von(36, 100)),
                at(500, off(36)),
                at(0, von(84, 100)),
                at(500, off(84)),
            ]
        );
    }

    #[test]
    fn a_played_key_outside_the_sieve_still_gets_its_halo() {
        let sieve = Sieve::parse("12@0").unwrap();
        let mut fx = ResonanceHalo::new(1, 1.0, 500, Some(sieve));
        run(&mut fx, pedal(127));
        assert_eq!(
            run_timed(&mut fx, 0, on(61)),
            vec![
                at(0, on(61)),
                at(0, von(60, 100)),
                at(500, off(60)),
                at(0, von(72, 100)),
                at(500, off(72)),
            ]
        );
    }

    #[test]
    fn the_pedal_is_tracked_per_channel() {
        let mut fx = ResonanceHalo::new(1, 1.0, 500, None);
        let pedal_ch1 = EventKind::ControlChange {
            ch: 1,
            cc: 64,
            value: 127,
        };
        assert_eq!(run(&mut fx, pedal_ch1), vec![pedal_ch1]);
        // Channel 0's pedal is up: no halo.
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        // Channel 1's is down.
        let on_ch1 = EventKind::NoteOn {
            ch: 1,
            key: 60,
            vel: 100,
        };
        assert_eq!(run(&mut fx, on_ch1).len(), 1 + 2 * 2);
    }

    #[test]
    fn the_pedal_threshold_is_sixty_four() {
        let mut fx = ResonanceHalo::new(1, 1.0, 500, None);
        run(&mut fx, pedal(64));
        assert_eq!(run(&mut fx, on(60)).len(), 5);
        run(&mut fx, pedal(63));
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
    }

    #[test]
    fn pedal_up_is_not_retroactive() {
        let mut fx = ResonanceHalo::new(1, 1.0, 500, None);
        run(&mut fx, pedal(127));
        // The halo pairs are already emitted with their own offs; lifting
        // the pedal emits nothing extra and later notes get no halo.
        assert_eq!(run(&mut fx, on(60)).len(), 5);
        assert_eq!(run(&mut fx, pedal(0)), vec![pedal(0)]);
        assert_eq!(run(&mut fx, on(62)), vec![on(62)]);
    }

    #[test]
    fn halo_velocity_floors_at_one() {
        let mut fx = ResonanceHalo::new(1, 0.0, 500, None);
        run(&mut fx, pedal(127));
        assert_eq!(
            run_timed(&mut fx, 0, on(60)),
            vec![
                at(0, on(60)),
                at(0, von(59, 1)),
                at(500, off(59)),
                at(0, von(61, 1)),
                at(500, off(61)),
            ]
        );
    }

    #[test]
    fn the_keyboard_edge_shrinks_the_halo() {
        let mut fx = ResonanceHalo::new(3, 1.0, 500, None);
        run(&mut fx, pedal(127));
        // Nothing below key 0: only the three neighbors above.
        assert_eq!(
            run_timed(&mut fx, 0, on(0)),
            vec![
                at(0, on(0)),
                at(0, von(1, 100)),
                at(500, off(1)),
                at(0, von(2, 100)),
                at(500, off(2)),
                at(0, von(3, 100)),
                at(500, off(3)),
            ]
        );
    }

    #[test]
    fn width_clamps() {
        let mut fx = ResonanceHalo::new(0, 1.0, 500, None);
        run(&mut fx, pedal(127));
        assert_eq!(run(&mut fx, on(60)).len(), 1 + 4);
        let mut fx = ResonanceHalo::new(20, 1.0, 500, None);
        run(&mut fx, pedal(127));
        assert_eq!(run(&mut fx, on(60)).len(), 1 + 4 * 6);
    }

    #[test]
    fn a_nearly_full_buffer_never_splits_a_pair() {
        use miditool_core::MAX_FANOUT;

        let mut fx = ResonanceHalo::new(6, 1.0, 500, None);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        fx.process(&Event::new(0, pedal(127)), &mut out, &cx);
        out.clear();
        // Three slots left: the pass-through on, one whole pair, and one
        // slot that must not receive a lone halo on.
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
    fn other_events_pass() {
        let mut fx = ResonanceHalo::new(2, 0.5, 1_000, None);
        let bend = EventKind::PitchBend { ch: 0, value: 512 };
        assert_eq!(run(&mut fx, bend), vec![bend]);
    }
}
