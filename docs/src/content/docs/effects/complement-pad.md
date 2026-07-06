---
title: complement-pad
description: A quiet pad that sounds every pitch class you are not holding, so player plus pad always complete the twelve-tone aggregate.
---

`complement-pad` sustains the chromatic complement of whatever you hold.

This is the aggregate thinking of Carter and Schoenberg, the twelve pitch classes as a total to be completed, turned into a drone. While at least one of your notes is held, the pad sounds the lowest key in `lo..=hi` for every pitch class absent from your held set, softly, at velocity `vel` on channel 1. Press a new pitch class and the pad gives that class up; release the last note holding a class and the pad takes it back. The pad always sounds exactly what you are not holding, so you and the pad together always spell the full aggregate. When you hold nothing the pad is silent; the complement of silence would be all twelve classes droning forever.

Your own notes pass through unchanged, any channel and velocity. On every note-on and note-off the pad is diffed rather than restruck: only the classes that changed hands move, pad releases come before your event and pad attacks after it, so a pad note that collides with one of your keys is handed over cleanly. There is no randomness anywhere; the pad is deterministic, no seed. A scene switch or shutdown releases the whole pad and every held note, so nothing is left sounding.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `lo` | key number | `60` | `0..=127` |
| `hi` | key number | `84` | `0..=127`, at least `lo` |
| `vel` | velocity | `18` | `1..=127` |

Each missing pitch class is voiced at the lowest key in `lo..=hi` that carries it; a pitch class with no key in the range is skipped, so a range narrower than an octave thins the pad.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

complement-pad lo=60 hi=84 vel=18
```

Hold a C major triad and the other nine pitch classes hover quietly above it; move to F major and only the classes that changed hands shift, the rest of the pad holding still.

## Try this

Push the pad up into one high octave and make it a shimmer:

```kdl
complement-pad lo=72 hi=83 vel=12
```

Now the complement sits in a tight cluster overhead, more halo than harmony, and single held bass notes light up eleven soft keys at once. For the opposite discipline, where your own notes are rationed until the aggregate completes, see [aggregate-gate](/miditool/effects/aggregate-gate/).
