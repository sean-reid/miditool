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
  registral-scatter seed=<u64> lo=21 hi=108
      Keep each note's pitch class but throw it into a seeded random
      octave within lo..hi.
  wedge-mirror axis=60 probability=1.0 seed=0
      Reflect notes around the axis key; probability below 1 mirrors
      only that (seeded) share of them.
  telescope factor=<0.1-8.0> reference=60
      Scale each note's distance from the reference key: factor above
      1 stretches intervals, below 1 squeezes them.
  ring-mod carrier=60 sum=true diff=true dry=false
      Ring modulation for keys: each note becomes its sum and/or
      difference with the carrier key; dry=true keeps the original too.
  row-snap 0 11 3 4 8 7 9 5 6 1 2 10 form=\"p\" transpose=0
      Snap notes onto a twelve-tone row (each pitch class exactly
      once). Forms: \"p\", \"i\", \"r\", \"ri\"; transpose shifts the row.
  sieve \"8@0|8@3|11@5\" snap=\"nearest\"
      Quantize keys onto a Xenakis sieve. Off-sieve notes snap
      \"nearest\", \"up\", or \"down\", or \"drop\" entirely.
  aggregate-gate leak=0.0 seed=0
      Each pitch class sounds once until all twelve have arrived, then
      the slate wipes; leak lets a (seeded) share of repeats through.
  blocked-keys 60 64 67
      Drop the listed keys; with by-class=true they are pitch classes
      0-11, blocked in every octave.
  tintinnabuli root=\"a\" minor=true position=1 direction=\"superior\" level=0.8
      Pärt's tintinnabuli: each melody note brings a companion from the
      tonic triad, the nearest (position=1) or second-nearest triad tone
      \"superior\", \"inferior\", or \"alternating\", at level of the touch.
  mode-lock mode=1 transposition=0 snap=\"nearest\"
      Lock keys to a church mode, 1 (ionian) through 7 (locrian),
      shifted up transposition semitones. Off-mode notes snap
      \"nearest\", \"up\", or \"down\", or \"drop\" entirely.
  negative-harmony tonic=\"c\" mode=\"replace\" level=0.8
      Reflect notes through the tonic's negative-harmony axis: the
      mirror replaces each note, or with mode=\"add\" joins it at level
      of the touch.
  tonnetz start=\"c\" minor=false sequence=\"rl\" lo=48 hi=79 include-played=false
      Walk the neo-Riemannian Tonnetz: each note takes the next p/l/r
      step in the sequence and sounds the arrived-at triad, voiced
      within lo..hi; include-played=true keeps your note too.
  complement-pad lo=60 hi=84 vel=18
      Sound what you are not playing: the missing pitch classes hum
      underneath as a quiet pad at vel, revoiced as the harmony moves.
  poisson-cloud seed=<u64> density=8.0 duration=\"2s\" sigma=7.0 vel-sigma=10.0 max=16
      Each note-on sprays a seeded cloud of grains: density grains per
      second for the duration, pitches and velocities spread Gaussian
      (sigma, vel-sigma) around the note, at most max grains.
  note-roulette seed=<u64> pass=0.6 replace=0.3 lo=21 hi=108
      Seeded roulette per note: passed through, replaced by a random
      key in lo..hi, or dropped (the leftover share). pass and replace
      sum to at most 1.
  velocity-dice seed=<u64> lo=1 hi=127
  velocity-dice seed=<u64> sigma=15.0
      Reroll every note-on velocity with seeded dice: uniform over
      lo..hi, or Gaussian around the played velocity when sigma is
      given.
  duration-lottery seed=<u64> mean=\"500ms\" min=\"30ms\" max=\"4s\" spread=\"exp\"
      Draw each note's length from a seeded lottery around the mean
      (or beats=), clamped to min..max; spread=\"uniform\" flattens the
      draw. min= and max= are plain durations.
  density-governor target=<notes/s> window=\"2s\" seed=0
      Thin the stream toward target notes per second over a sliding
      window; the excess is dropped by seeded lottery.
  cluster-fist width=4 kind=\"chromatic\" anchor=\"center\" rolloff=0.8
      Cowell clusters: each note lands as a fistful of width keys,
      chromatic, white, black, or kind=\"sieve\" sieve=\"8@0|8@3\",
      anchored bottom/center/top, edges fading by rolloff.
  resonance-halo width=3 level=0.25 decay=\"3s\"
      Ghost sympathetic resonance: each note adds a halo of width
      neighbors at level velocity, fading over decay; sieve=\"...\"
      confines the halo to sieve keys.
  velocity-curve gamma=1.0 floor=1 ceiling=127
      Reshape touch: gamma below 1 lifts soft playing, above 1 compresses
      it. Output maps into floor..ceiling.
  accent-groups 3 5 accent=112 rest=64
      Additive accent groups (3+5 and so on): the first note of each
      group lands at the accent velocity, the rest lay back at rest.
  feldman-field seed=0 floor=8 ceiling=28 jitter=4
      Feldman's quiet field: every velocity sinks into floor..ceiling
      with a seeded jitter, so nothing rises above a whisper.
  velocity-invert pivot=64
      Mirror velocities around the pivot: soft playing comes out loud
      and loud playing comes out soft.
  velocity-router low=64 high=96 soft=2 medium=3 loud=4
      Route notes by touch: below low goes to the soft channel, above
      high to the loud one, everything between to medium.
  anti-accent level=30 every=\"30s\" seed=0
      Every note is capped at the level velocity, except one loud note
      allowed through unmodified per every window (at least 1s): the
      rare thunderclap in an otherwise soft field.
  mass-crescendo period=\"120s\" depth=0.6 shape=\"arch\"
      A slow tide under the dynamics: velocities swell by up to depth
      over each period, rising and falling (\"arch\") or rising and
      resetting (\"ramp\").
  channelize <1-16>
      Send everything to one MIDI channel.
  klangfarben 2 3 4 mode=\"cycle\" seed=0
      Deal successive notes across channels, one per note: around the
      list in order, or a seeded random pick with mode=\"random\".
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
  euclidean-gate k=3 n=8 rotation=0 pulse=\"125ms\" mode=\"defer\"
      Gate notes through a Euclidean rhythm: k pulses spread evenly
      over n steps of pulse length (default a quarter beat). Off-step
      notes wait for the next pulse, or vanish with mode=\"drop\".
  quantize grid=\"125ms\" strength=1.0
      Pull events onto a time grid (default a quarter beat): strength
      1 snaps them exactly, lower values move them only part way.
  talea 250 500 250 1000
  talea 1 0.5 0.5 2 beats=true
      Lock notes to a repeating duration cycle, the medieval talea:
      entries are milliseconds, or beats against the tempo with
      beats=true. Each entry resolves to 1ms..60s.
  added-value seed=<u64> unit=\"60ms\" extend=0.3 defer=0.0
      Messiaen's added values: a seeded share of notes stretches one
      unit longer (extend) or arrives one unit late (defer), so the
      meter never quite settles.
  script \"wedge.lua\" seed=0
      Run a Luau script on every event: return nil to pass, false to
      drop, a table (or an array of tables) to emit. The path resolves
      against the config file. `miditool new script` writes a starter.
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

Any time-measuring property (time=, interval=, duration=, and so on)
takes \"250ms\" or \"1.5s\", or beats=0.5 against the tempo. Note-valued
properties (root=, tonic=, start=) take a note name like \"c\", \"f#\",
or \"bb\", or a pitch class 0..11. Randomness is deterministic: the same seed
always gives the same result. The script node's Lua API is documented at
https://sean-reid.github.io/miditool/configuration/scripting/.
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
