---
title: continuum
description: Ligeti's Continuum as a machine you steer. Held keys dissolve into a fast mechanical cycle; the direct sound is consumed and the stream replaces it.
---

`continuum` turns whatever you hold into a stream faster than fingers.

Ligeti built *Continuum* and *Coulee*, for harpsichord and organ, from one figure repeated past the threshold where notes fuse into texture; this machine does the repeating for you. Your held keys become the machine's material: while any keys are down, the machine cycles through them at `rate` notes per second, each note sounding for `gate` of its slot at the velocity you pressed that key with. `order` picks the path through the held set: `"up"` and `"down"` cycle by pitch, `"played"` follows the order of arrival, and `"random"` takes a seeded walk that never repeats a key back to back while more than one is held. The stream starts on the note-on that wakes the empty machine, retriggering a held key updates its velocity in place, and releasing the last key stops the cycle.

Note-ons and note-offs are consumed: the cycle replaces direct sounding, so your own keystrokes never reach the output, only the machine's notes do. Everything else passes. The machine tracks up to 16 held keys; further keys are consumed but ignored until a slot frees up.

`continuum` is a [generator](/miditool/how-it-works/#generators): it runs on its own seeded clock and cleans up on a scene switch, and the seed only matters for `order="random"`, the other orders being already deterministic.

The machine's notes travel down the chain like anything else, which is the point of chaining: put [`velocity-curve`](/miditool/effects/velocity-curve/) or [`mode-lock`](/miditool/effects/mode-lock/) after `continuum` and the whole stream is reshaped.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `rate` | number | `12.0` | `2..=30` notes per second |
| `order` | string | `"played"` | `"up"`, `"down"`, `"played"`, `"random"` |
| `gate` | number | `0.5` | `0.1..=0.9` fraction of each slot |
| `seed` | integer | `0` | any unsigned 64-bit value |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

continuum rate=15.0 order="played" gate=0.5 seed=3
velocity-curve gamma=0.7 ceiling=110
```

Hold a chord and it becomes the pattern: the held keys cycle at 15 notes a second in the order you pressed them, and the curve underneath lifts the blur so the stream stays present at speed. This is the `continuum` scene of `examples/machines.kdl`.

## Try this

Shorten the gate and let the order wander:

```kdl
continuum rate=24.0 order="random" gate=0.2 seed=11
```

At 24 notes a second with a fifth of each slot sounding, single keys turn into a dry ticking and chords into a shimmer whose internal order never settles. Swap `seed=` until the walk sings; the number you keep is the piece.
