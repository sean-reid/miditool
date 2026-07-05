# miditool

A MIDI mixing layer that sits between your keyboard and your DAW. It reads what you play, transforms it in real time, and hands the DAW a single altered stream: scrambled keys, stochastic note clouds, serial rows, velocity fields, and whatever else you can compose out of its effect graph.

Documentation: https://sean-reid.github.io/miditool/

Built for live performance on an 88-key piano: the processing path is allocation-free and adds no audible latency, every random effect is seeded and reproducible, and note-off correctness is guaranteed even when a mapping changes while a note is held.

## Status

Early but real: the engine, nine effects, scenes, hot reload, and the phone remote all work. Much more is planned, including scriptable effects and a large catalog drawn from 20th century composition techniques.

## Quick start

```sh
curl -fsSL https://github.com/sean-reid/miditool/releases/latest/download/miditool-installer.sh | sh
miditool ports                  # find your keyboard
```

Windows and from-source instructions are in the [docs](https://sean-reid.github.io/miditool/getting-started/).

A config is a KDL file describing an effect chain. Save this as `miditool.kdl`, then `miditool run`:

```kdl
input "Roland"                  // substring of your keyboard's port name
output virtual="miditool Out"   // the port your DAW listens to

shuffle-lock seed=42            // scramble the keys, deterministically
velocity-curve gamma=0.8
```

Point your DAW at the `miditool Out` port and play.

### The GarageBand problem

GarageBand listens to every MIDI source at once, so out of the box it hears both your raw keyboard and miditool's output. On macOS, miditool solves this directly: mark the input with `hide=true`

```kdl
input "Roland" hide=true
```

and while miditool runs, the raw keyboard is hidden from every other app; GarageBand sees only `miditool Out`. Start miditool before the DAW (or restart the DAW once). Visibility is restored when miditool exits; if a run is ever killed hard, `miditool unhide` puts things back.

Elsewhere, or if you prefer not to hide the port, use a DAW with per-track MIDI input selection (Logic Pro, Reaper, Ableton, Cubase) and pick `miditool Out`. A Raspberry Pi middle-box mode, where the computer only ever sees one device, is planned for a later release.

## Platforms

- macOS: CoreMIDI, virtual output ports, input hiding.
- Linux (including Raspberry Pi): ALSA, virtual output ports.
- Windows: WinMM. Windows has no native virtual MIDI ports; install loopMIDI, create a port there, and point `output device="loopMIDI"` at it.

## Effects

Run `miditool effects` for the current list with parameters. The founding pair:

- `shuffle-lock`: a seeded permutation of the keys. The keyboard is scrambled but stable, so you can learn the scrambled instrument. Same seed, same scramble, forever.
- `loose-keys`: every press draws a fresh note. The same key twice gives different notes, uniform over a range or Gaussian around what you played.

Effects compose with `chain { }` and `fork { }` blocks and filters like `key-range` and `only-channels`, so a split keyboard with different treatments per hand is a few lines of config.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
```

The workspace is organized as small crates: `core` (event model, wire codec, effect graph), `effects`, `io` (MIDI ports), `engine` (the realtime loop), `config` (KDL), and `cli`.
