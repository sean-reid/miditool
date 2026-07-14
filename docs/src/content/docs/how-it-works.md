---
title: How it works
description: The path a MIDI event takes through miditool, seeded determinism, note-off safety, and hot reload.
---

## The event path

Events flow one way:

```text
keyboard  ->  miditool  ->  virtual port ("miditool Out")  ->  DAW
```

miditool opens your keyboard as an input, runs every event through the effect graph described by your config, and writes the result to a virtual output port. To the DAW, that port looks like any other MIDI device. The processing path is allocation-free and adds no audible latency; [`miditool bench`](/miditool/reference/cli/#miditool-bench) measures the whole round trip on your machine.

## Generators

Six effects generate as well as transform: [`continuum`](/miditool/effects/continuum/), [`mechanico`](/miditool/effects/mechanico/), [`metronome-swarm`](/miditool/effects/metronome-swarm/), [`brownian-walker`](/miditool/effects/brownian-walker/), [`continuator`](/miditool/effects/continuator/), and [`crippled-looper`](/miditool/effects/crippled-looper/) run on their own internal clocks, so their notes keep flowing even when nothing arrives from the keyboard. They share one discipline. Each clock is seeded and deterministic. When ticks run late, the machine catches up a bounded amount and skips or defers the rest, so time never bunches. Every note a generator starts carries its own note-off, and leaving the scene cleans up: `switch="kill"` silences immediately, while the default let-ring switch lets sounding notes drain at their own pace.

## Seeded determinism

Every random effect takes a `seed`, and the same seed produces the same behavior forever: the same scramble, the same draws, the same jitter, across runs and across machines. Randomness in miditool is a compositional choice you can stand behind, not a dice roll per session. [Seeds](/miditool/configuration/seeds/) covers why this matters for actually learning a scrambled instrument.

## Note-off safety

A note that goes on must come off, even if the mapping changes while you hold the key. miditool guarantees this: every note-on is tracked, and its note-off is delivered to whatever key and channel the note-on was sent to, no matter what the graph looks like by then. Stuck notes are not a failure mode you need to plan around. If something else in your rig misbehaves, the [remote's](/miditool/guides/remote/) PANIC button releases everything, and so does stopping miditool.

## Hot reload

`miditool run` watches the config file. Save an edit while playing and the new effect graph swaps in on the next note; notes held through the swap finish under the mapping that started them. If an edit does not parse, miditool reports the error and keeps the old graph running, so a typo never interrupts sound. Input and output changes are the one exception: they need a restart. Details in [Live editing](/miditool/guides/live-editing/).
