---
title: klangfarben
description: Deal successive notes across MIDI channels, one instrument per note. A single line becomes a Klangfarbenmelodie.
---

`klangfarben` deals successive notes across MIDI channels, one channel per note.

Point each channel at a different instrument in the DAW and a single line becomes a Klangfarbenmelodie, the melody-of-timbres Schoenberg named and Webern perfected in his orchestration of Bach's ricercar: the notes are yours, but each one speaks in a different voice. By default the dealing cycles through the channels in the order written; `mode="random"` draws a seeded channel per note instead.

Random dealing follows its seed: the same seed deals the same channels forever; see [Seeds](/miditool/configuration/seeds/). Note-offs and polyphonic aftertouch follow each note to its dealt channel, so nothing sticks; pedals and other non-note events pass on their original channel.

## Parameters

`klangfarben` takes the channels as bare arguments, at least one, no repeats. The written order is the cycle.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| channels | integer arguments | required | `1..=16`, each listed once |
| `mode` | string | `"cycle"` | `"cycle"`, `"random"` |
| `seed` | integer | `0` | any unsigned 64-bit value; only matters with `"random"` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

klangfarben 2 3 4          // each note to the next instrument, round and round
```

In the DAW, set three instrument tracks to receive channels 2, 3, and 4; see [the DAW guide](/miditool/guides/daws/#tip-one-port-many-instruments).

## Try this

Let the timbre change as each note fades. The [echo](/miditool/effects/echo/) repeats are notes too, so every repeat is dealt onward:

```kdl
echo repeats=4 beats=0.5 decay=0.7
klangfarben 2 3 4 mode="random" seed=11
```

One keypress now dies away through four instruments, in an order the seed decided once and for all.
