---
title: note-roulette
description: Every note-on gambles on a seeded coin, passed through, replaced by a random key, or silenced. Chance operations between your hands and the sound.
---

`note-roulette` spins a seeded wheel for every note: keep it, replace it, or lose it.

A coin decides each note, which is the chance operation as Cage practiced it: pass, replace, or silence, drawn fresh at every note-on, with your rhythm and touch left exactly as played.

The outcome is remembered per held note, so the matching note-off lands on whatever the wheel chose, and a silenced note takes its note-off with it: nothing hangs. The wheel is seeded: the same seed and the same playing land on the same outcomes, forever; see [Seeds](/miditool/configuration/seeds/).

## Parameters

`pass` and `replace` must sum to at most 1; the leftover share is the probability of silence.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `pass` | number | `0.6` | `0..=1` |
| `replace` | number | `0.3` | `0..=1`, `pass + replace` at most 1 |
| `lo` | key number | `21` (A0) | `0..=127`, at most `hi` |
| `hi` | key number | `108` (C8) | `0..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

note-roulette seed=23 pass=0.5 replace=0.25 lo=36 hi=96
```

Half your notes survive, a quarter mutate into something from the middle five octaves, and a quarter vanish.

## Try this

Make silence the main event. Only one note in five sounds at all, and half of those are not yours:

```kdl
note-roulette seed=13 pass=0.1 replace=0.1
```

Then set `replace=0` and raise `pass` for pure thinning: no substitutions, just gaps eaten out of your lines. For thinning that responds to how fast you play instead of a fixed share, reach for [`density-governor`](/miditool/effects/density-governor/).
