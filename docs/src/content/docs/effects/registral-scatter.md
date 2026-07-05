---
title: registral-scatter
description: Keep each note's pitch class but throw it into a seeded random octave. The line stays itself in pitch class and shatters in register.
---

`registral-scatter` keeps every note's pitch class and lands it in a random octave.

Play a stepwise melody and it comes out pointillist, sprayed across the registers the way a Webern line disperses a simple contour over six octaves. Every note-on draws a fresh octave inside `lo..=hi`, so the same key wanders from register to register while the pitch-class content, and with it the harmony, survives intact. A pitch class with no octave inside the range passes unchanged.

The draws are seeded: the same seed gives the same performance forever; see [Seeds](/miditool/configuration/seeds/). Note-offs follow whatever octave their note-on drew, so held and overlapping notes always release correctly.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `lo` | key number | `21` (A0) | `0..=127`, at most `hi` |
| `hi` | key number | `108` (C8) | `0..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

registral-scatter seed=5 lo=36 hi=96      // five middle octaves
```

## Try this

The sieve-cloud scene from `examples/serial.kdl`: keys snap up onto a [sieve](/miditool/effects/sieve/), scatter across the octaves, and trail off in fading fifth-shifted [echoes](/miditool/effects/echo/):

```kdl
tempo 72

sieve "8@0|8@3|11@5" snap="up"
registral-scatter seed=5 lo=36 hi=96
echo repeats=3 beats=1 decay=0.6 transpose=7
```

Then narrow the range to `lo=60 hi=84` and the pointillism calms into a two-octave shimmer.
