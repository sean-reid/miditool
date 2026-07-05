# miditool

A MIDI mixing layer that sits between your keyboard and your DAW. It reads what you play, transforms it in real time, and hands the DAW a single altered stream: scrambled keys, stochastic note clouds, serial rows, velocity fields, and whatever else you can compose out of its effect graph.

Built for live performance on an 88-key piano: the processing path is allocation-free and adds no audible latency, every random effect is seeded and reproducible, and note-off correctness is guaranteed even when a mapping changes while a note is held.

## Status

Early. The engine core, the first effects, and the CLI work. Much more is planned: scenes, delay and echo lines, a phone remote, scriptable effects, and a large catalog of effects drawn from 20th century composition techniques.

## Quick start

```sh
cargo install --path crates/cli
miditool ports                  # find your keyboard
miditool run examples/scrambled.kdl
```

A config is a KDL file describing an effect chain:

```kdl
input "Roland"                  // substring of your keyboard's port name
output virtual="miditool Out"   // the port your DAW listens to

shuffle-lock seed=42            // scramble the keys, deterministically
velocity-curve gamma=0.8
```

Point your DAW at the `miditool Out` port and play.

### The GarageBand problem

GarageBand listens to every MIDI source at once, so it will hear both your raw keyboard and miditool's output. Options:

- Use a DAW with per-track MIDI input selection (Logic Pro, Reaper, Ableton, Cubase) and pick `miditool Out`.
- Run miditool on a Raspberry Pi between the keyboard and the computer, so the computer only ever sees one device. Setup guide coming with a later release.
- On macOS there is a promising trick to hide the raw keyboard from other apps while miditool keeps reading it; see `spikes/hidekb.swift`. It will be built into miditool once verified.

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
