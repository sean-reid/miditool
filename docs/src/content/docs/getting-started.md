---
title: Getting started
description: Install miditool, find your keyboard, write a first config, and point your DAW at the transformed stream.
---

miditool sits between your MIDI keyboard and your DAW. You describe a transformation in a small config file, miditool applies it live, and your DAW records the result.

## Install

On macOS or Linux (including a Raspberry Pi):

```sh
curl -fsSL https://github.com/sean-reid/miditool/releases/latest/download/miditool-installer.sh | sh
```

On Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/sean-reid/miditool/releases/latest/download/miditool-installer.ps1 | iex"
```

Or build from source with a Rust toolchain ([rustup.rs](https://rustup.rs) if you need one):

```sh
git clone https://github.com/sean-reid/miditool
cd miditool
cargo install --path crates/cli
```

Check `miditool --version` to confirm the install. To verify a download out of band, every [GitHub release](https://github.com/sean-reid/miditool/releases) lists per-file sha256 sums alongside the binaries.

## Find your keyboard

```sh
miditool ports
```

```text
inputs:
  Roland FP-30 MIDI IN
outputs:
  Roland FP-30 MIDI OUT
```

Any unique substring of the input name is enough to select it: `"Roland"` will do.

## Watch what it sends

```sh
miditool monitor --input Roland
```

Play a few notes. Each event prints with a timestamp, channel, note name, and velocity:

```text
     0.000  ch1  note-on   C4   (60) vel 96
     0.412  ch1  note-off  C4   (60) vel 0
```

This is the raw stream miditool will transform. Ctrl-C to stop.

## Write a first config

Save this as `miditool.kdl`:

```kdl title="miditool.kdl"
input "Roland"                  // substring of your keyboard's port name
output virtual="miditool Out"   // the port your DAW listens to

shuffle-lock seed=42            // scramble the keys, deterministically
velocity-curve gamma=0.8
```

Swap `"Roland"` for a substring of your own keyboard's name.

## Run it

```sh
miditool run
```

`run` reads `./miditool.kdl` by default; pass a path to use another file. miditool opens your keyboard, creates a virtual output port named `miditool Out`, and starts transforming.

## Point the DAW at miditool

In your DAW, select `miditool Out` as the MIDI input and play. Every key now sounds as some other key, and the mapping holds: seed 42 is the same scramble every time.

Two things worth knowing before you go deeper:

- **GarageBand** listens to every MIDI source at once, so it hears both the raw keyboard and miditool. One config property fixes it: see [Using miditool with GarageBand](/miditool/guides/garageband/).
- You can **edit the config while playing**. Saves apply on the next note, and a broken edit never interrupts sound: see [Live editing](/miditool/guides/live-editing/).

## Next steps

- [How it works](/miditool/how-it-works/): the event path in one page.
- [Config files](/miditool/configuration/config-files/): the full file shape, scenes, and where miditool looks.
- [Effects](/miditool/effects/shuffle-lock/): every effect, every parameter.
