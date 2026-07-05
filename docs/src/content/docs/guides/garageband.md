---
title: Using miditool with GarageBand
description: GarageBand listens to every MIDI source at once. Hide the raw keyboard with input hide=true so it hears only miditool.
---

GarageBand has no per-track MIDI input selection: it listens to every MIDI source on the system at once. Out of the box that means it hears your raw keyboard *and* miditool's output, so every note plays twice, once untransformed. This page fixes that.

## The fix

On macOS, miditool can hide the raw keyboard from every other app while it runs. Add `hide=true` to the `input` node:

```kdl title="miditool.kdl"
input "Roland" hide=true
output virtual="miditool Out"

shuffle-lock seed=42
velocity-curve gamma=0.8
```

While `miditool run` is active, the raw keyboard is invisible to GarageBand and everything else; only `miditool Out` remains. GarageBand hears exactly one stream: the transformed one.

When miditool exits, the keyboard reappears. Nothing about the device or the system is permanently changed.

## Start order matters

GarageBand scans MIDI sources at launch and keeps whatever it found. So:

1. Start miditool first: `miditool run`
2. Then open GarageBand.

If GarageBand was already open, quit and relaunch it once while miditool runs. The same applies after any change to which ports exist.

## If a run is killed hard

miditool restores the hidden keyboard on any normal exit, including Ctrl-C. If the process is ever killed outright (a force quit, a power cut), the keyboard can stay hidden. One command recovers it:

```sh
miditool unhide
```

Without an argument it restores every hidden source; pass a name substring to restore one.

## When something seems off

```sh
miditool doctor
```

`doctor` checks the pieces this page depends on: it lists your MIDI ports, warns about sources that look hidden, validates the config, and warns if GarageBand or Logic Pro is already running (and therefore holding the pre-miditool port list). A typical healthy report:

```text
ok    midi backend: 1 input (Roland FP-30 MIDI IN), 1 output (Roland FP-30 MIDI OUT)
ok    config miditool.kdl: parses, 2 top-level effects
ok    no MIDI sources look hidden
warn  GarageBand is running; apps started before miditool keep hearing the raw keyboard until relaunched
ok    Logic Pro is not running
```

## Elsewhere than GarageBand

Hiding is a CoreMIDI feature, so `hide=true` is macOS only; other platforms ignore it with a note. DAWs with per-track input selection do not need it at all: see [Other DAWs](/miditool/guides/daws/).
