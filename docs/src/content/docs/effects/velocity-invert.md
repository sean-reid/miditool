---
title: velocity-invert
description: Mirror velocities around a pivot so loud playing comes out soft and soft playing comes out loud. The pivot is the fixed point.
---

`velocity-invert` mirrors velocities around a pivot: loud becomes soft, soft becomes loud.

The integral serialists treated loudness as a parameter to invert and permute as freely as pitch; this is that operation applied to your own touch, live. Each note-on comes out at `2 * pivot - velocity`, clamped into `1..=127`: the pivot is the fixed point, everything else reflects through it. What you hammer arrives whispered, and the grace note you barely brushed detonates. Played for a while, it retrains the hand in a strange way: to be heard you must hold back, and emphasis becomes a thing you do by restraint.

Only note-on velocities are touched; note-offs and everything else pass untouched.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `pivot` | velocity | `64` | `1..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

velocity-invert pivot=64      // 100 comes out as 28, 28 as 100
```

## Try this

Turn decay into a swell. An [`echo`](/miditool/effects/echo/) makes each repeat softer; inverting after it makes each repeat *louder*, so every note is chased by a rising crescendo of itself, like a tape run backwards:

```kdl
echo repeats=5 beats=0.5 decay=0.7
velocity-invert pivot=64
```

Then move the mirror: `pivot=96` reflects everything into the loud half, where the gentlest touch slams out at full force and even your hardest playing never drops below a mezzo-forte.
