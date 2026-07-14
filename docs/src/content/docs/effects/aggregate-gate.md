---
title: aggregate-gate
description: Each pitch class sounds once until all twelve have arrived, then the slate wipes. Chromatic discipline as a playable game.
---

`aggregate-gate` lets each pitch class through once; repeats are dropped until all twelve have sounded.

Schoenberg ran his rows on this discipline: no pitch class returns before the chromatic collection is complete. Here it is a gate on your own playing, and the effect on tonal habits is immediate: repeat a note too soon and the gate swallows it, so vamping turns into circulation whether you meant it or not.

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

// Vamp two chords. The gate lets each pitch class speak once per
// cycle, the scatter throws the survivors across six octaves: the
// leftovers of a I-IV vamp turn Webern.
aggregate-gate
registral-scatter seed=12 lo=30 hi=102
```

## Try this

Let one repeat in four through:

```kdl
aggregate-gate leak=0.25 seed=6
```

Improvise something tonal and listen to it get rationed. At `leak=0` the gate is a hard teacher; around `leak=0.5` it is a colleague with opinions; by `leak=0.9` it only occasionally clears its throat.
