---
title: Troubleshooting
description: Doubled notes, stuck notes, invisible keyboards, and latency questions, each with the command that answers it.
---

## Every note plays twice

Your DAW is hearing both the raw keyboard and miditool's output. This is GarageBand's default behavior and the fix is one config property: [Using miditool with GarageBand](/miditool/guides/garageband/). In DAWs with input selection, disable the raw keyboard as an input or pin the track to `miditool Out`: [Other DAWs](/miditool/guides/daws/).

## Stuck notes

miditool itself guarantees note-off delivery, but the rest of a rig can misbehave. Two ways out:

- Hit **PANIC** on the [web remote](/miditool/guides/remote/); it releases every sounding note immediately.
- Stop miditool (Ctrl-C). It winds down by releasing all held notes before it exits.

## The keyboard disappeared

A run that used `hide=true` was killed hard (force quit, crash, power loss) before it could restore the input. Recovery:

```sh
miditool unhide
```

It restores every hidden source and prints what it touched. This is macOS only, because hiding is.

## How much latency is this adding?

Measure it on your machine:

```sh
miditool bench
```

It runs 500 note pairs through a live engine and prints round-trip percentiles, typically well under a millisecond. See [the CLI reference](/miditool/reference/cli/#miditool-bench) for reading the table.

## Something else is wrong

```sh
miditool doctor
```

One line per check: the MIDI backend and its ports, the config file, hidden sources, running DAWs (macOS), the ALSA sequencer (Linux), loopMIDI (Windows). Every check runs even if an early one fails, so one report shows everything that needs attention. `doctor` exits nonzero only on hard failures: an unreachable MIDI backend or a config that does not parse.

## The config will not parse

Config errors name the node and the constraint, and `miditool run` points at the offending line. The rules, ranges, and defaults for every node are on the [configuration](/miditool/configuration/config-files/) and [effect](/miditool/effects/shuffle-lock/) pages.
