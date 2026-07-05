//! Human-readable event printing for `miditool monitor`, and the effects
//! reference text.

use miditool_core::EventKind;
use miditool_core::wire::{Decoded, Decoder};

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// MIDI key number as scientific pitch, middle C (60) = C4.
pub fn note_name(key: u8) -> String {
    let octave = i32::from(key) / 12 - 1;
    format!("{}{}", NOTE_NAMES[(key % 12) as usize], octave)
}

pub struct EventPrinter {
    decoder: Decoder,
    first_stamp: Option<u64>,
}

impl EventPrinter {
    pub fn new() -> Self {
        Self {
            decoder: Decoder::new(),
            first_stamp: None,
        }
    }

    /// Print every event in one incoming packet. `stamp_us` is the
    /// backend's microsecond timestamp; shown relative to the first packet.
    pub fn print(&mut self, stamp_us: u64, bytes: &[u8]) {
        let base = *self.first_stamp.get_or_insert(stamp_us);
        let t = (stamp_us - base) as f64 / 1e6;
        if bytes.first().is_some_and(|b| (0xF0..0xF8).contains(b)) {
            println!("{t:10.3}  system {bytes:02X?}");
            return;
        }
        self.decoder.feed(bytes, |decoded| match decoded {
            Decoded::Event(kind) => println!("{t:10.3}  {}", describe(&kind)),
            Decoded::Realtime(byte) => println!("{t:10.3}  realtime {byte:#04X}"),
            Decoded::Pending => {}
        });
    }
}

fn describe(kind: &EventKind) -> String {
    match *kind {
        EventKind::NoteOn { ch, key, vel } => {
            format!(
                "ch{:<2} note-on   {:<4} ({key}) vel {vel}",
                ch + 1,
                note_name(key)
            )
        }
        EventKind::NoteOff { ch, key, vel } => {
            format!(
                "ch{:<2} note-off  {:<4} ({key}) vel {vel}",
                ch + 1,
                note_name(key)
            )
        }
        EventKind::PolyPressure { ch, key, value } => {
            format!(
                "ch{:<2} pressure  {:<4} ({key}) {value}",
                ch + 1,
                note_name(key)
            )
        }
        EventKind::ControlChange { ch, cc, value } => {
            format!("ch{:<2} cc{cc} = {value}", ch + 1)
        }
        EventKind::ProgramChange { ch, program } => {
            format!("ch{:<2} program {program}", ch + 1)
        }
        EventKind::ChannelPressure { ch, value } => {
            format!("ch{:<2} pressure {value}", ch + 1)
        }
        EventKind::PitchBend { ch, value } => {
            format!("ch{:<2} bend {value:+}", ch + 1)
        }
    }
}

pub const EFFECTS_HELP: &str = "\
effects
  shuffle-lock seed=<u64> lo=21 hi=108 mode=\"free\"
      Seeded permutation of the keys in lo..hi. The keyboard is scrambled
      but stable: each key keeps its scrambled assignment. Modes: \"free\",
      \"within-octave\", \"within-pitch-class\".
  loose-keys seed=<u64> lo=21 hi=108
  loose-keys seed=<u64> sigma=7.0
      Every press draws a fresh note: uniform over lo..hi, or Gaussian
      around the played key when sigma is given.
  transpose <semitones>
      Shift notes up or down. Notes leaving the MIDI range are dropped.
  velocity-curve gamma=1.0 floor=1 ceiling=127
      Reshape touch: gamma below 1 lifts soft playing, above 1 compresses
      it. Output maps into floor..ceiling.
  channelize <1-16>
      Send everything to one MIDI channel.
  delay time=\"250ms\"
      Hold everything back by a fixed time.
  echo repeats=3 time=\"300ms\" decay=0.6 transpose=0
      Fading repeats after each note, every repeat decay times softer
      and shifted by transpose semitones.
  restrike seed=<u64> interval=\"2s\" jitter=0.15 decay=0.7 floor=8 max=12
      Re-strike held notes on a jittered interval, fading toward the
      floor velocity, at most max strikes per note.
  stutter repeats=6 first=\"30ms\" curve=1.0
      Ratchet each note into a burst: gaps start at first, then stretch
      (curve above 1) or tighten (below 1) as the burst plays out.
  pass / discard
      Identity and mute, mostly useful inside fork branches.

routing
  chain { ... }            effects in series
  fork { ... }             effects in parallel, outputs merged
  only-channels 1 2 ...    keep events on these channels
  key-range lo=.. hi=..    keep notes in a range, defaults 0..127
  velocity-range lo=1 hi=127
  notes-only / controllers-only

config file shape
  input \"Roland\" hide=true        optional; substring, hide=true hides it
  output virtual=\"miditool Out\"    default; or output device=\"IAC\"
  tempo 120                          default; beats per minute for beats=
  remote port=8320 bind=\"0.0.0.0\"  optional; phone/tablet web remote
  ...effects...                      top level is an implicit chain
  scene \"name\" { ...effects... }    or: one or more named scenes

Scenes replace the bare chain; the two styles don't mix. Each scene is
its own chain, and switch=\"kill\" cuts sounding notes when you leave it
(the default, switch=\"let-ring\", lets them ring out). The remote serves
a scene switcher to browsers on the given port; without bind= it stays
on this machine, and bind=\"0.0.0.0\" opens it to the local network.

Time properties (time=, interval=, first=) take \"250ms\" or \"1.5s\", or
beats=0.5 against the tempo. Randomness is deterministic: the same seed
always gives the same result.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(21), "A0");
        assert_eq!(note_name(108), "C8");
        assert_eq!(note_name(0), "C-1");
    }
}
