---
title: duration-lottery
description: Each note's length is drawn from a seeded lottery and your release is ignored. Durations become material fixed by the draw, not by the hand.
---

`duration-lottery` draws how long each note lasts. You choose when notes start; the lottery decides when they end.

Durations stop being something you perform and become something dealt, the way a Feldman page fixes them on paper regardless of the pianist's instinct, or a Xenakis screen draws them from a law. Each note-on is emitted together with a note-off already scheduled at the drawn length: exponential around `mean` by default (most notes short, the occasional one that stays), or flat across the range with `spread="uniform"`; either way the draw is clamped into `min..max`.

Your own release is ignored: letting go of the key does nothing, the drawn duration ends the note. Because every note-on leaves with its note-off scheduled in the same breath, note-ons and note-offs always balance and nothing can hang, however you play; retriggering a held key simply stacks another drawn note on it. The lottery is seeded: the same seed and the same playing deal the same lengths, forever; see [Seeds](/miditool/configuration/seeds/).

## Parameters

`mean=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/). `min=` and `max=` are plain duration strings only. The mean must sit within `min..max`.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `mean` | duration string | `"500ms"` | positive, at least `min`, at most `max` |
| `beats` | number | none (instead of `mean`) | finite, greater than 0 |
| `min` | duration string | `"30ms"` | positive |
| `max` | duration string | `"4s"` | positive, at least `min` |
| `spread` | string | `"exp"` | `"exp"`, `"uniform"` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

duration-lottery seed=31 mean="400ms" min="50ms" max="2s"
```

Play an even line and it comes back with its lengths dealt: mostly clipped, now and then a note that refuses to leave.

## Try this

Flatten the draw and stretch it. With `spread="uniform"` every length between one and six seconds is equally likely (the `mean` is not used by the uniform draw, but it still has to sit inside the range):

```kdl
duration-lottery seed=2 spread="uniform" mean="3.5s" min="1s" max="6s"
```

Touch the keyboard staccato and it answers in slow overlapping tones. Tighten `min="2ms"` `max="60ms"` `mean="20ms"` instead and everything you hold is snatched away.
