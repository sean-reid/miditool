---
title: aggregate-gate
description: Each pitch class sounds once until all twelve have arrived, then the slate wipes. Chromatic discipline as a playable game.
---

`aggregate-gate` lets each pitch class through once; repeats are dropped until all twelve have sounded.

This is Schoenberg's aggregate discipline made into a gate: no pitch class returns before the chromatic collection is complete. The note that completes the aggregate still sounds, then the slate wipes and all twelve are available again. Playing through it becomes a game of spending pitch classes; vamp on three chords and the gate thins them to whichever pitch classes are still unspent, forcing your hands toward the notes you have been avoiding.

`leak` loosens the rule: that fraction of repeats slips through anyway, on a seeded draw, sliding the effect from strict serialism toward free chromaticism. The same seed leaks the same notes forever; see [Seeds](/miditool/configuration/seeds/). A dropped note-on takes its note-off with it, so nothing is left hanging.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `leak` | number | `0.0` | `0..=1` |
| `seed` | integer | `0` | any unsigned 64-bit value; only matters above `leak=0` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

aggregate-gate               // strict: no pitch class returns early
```

## Try this

Let one repeat in four through:

```kdl
aggregate-gate leak=0.25 seed=6
```

Improvise something tonal and listen to it get rationed. At `leak=0` the gate is a hard teacher; around `leak=0.5` it is a colleague with opinions; by `leak=0.9` it only occasionally clears its throat.
