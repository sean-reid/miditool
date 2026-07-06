---
title: brownian-walker
description: The random walks of Xenakis's Mists. Each note plants a wanderer that steps Gaussian distances in legato, reflecting off the range walls, until your release recalls it.
---

`brownian-walker` sends every note you play wandering off on its own.

The model is the pitch random walks of Xenakis's *Mists*, lines that drift by chance instead of by scale. Each note-on plants a walker: it sounds the played key immediately at your velocity, then every `interval` it steps a Gaussian-drawn distance (`sigma` semitones wide, rounded), sounding wherever it lands. Steps are legato, the note-off of the previous key and the note-on of the next stamped together, so a walker is one continuous voice. The walls at `lo` and `hi` reflect: a walker that would step past them folds back into range. Your note-off for the planted key recalls its walker, releasing whatever it is currently sounding. Up to 8 walkers roam at once; a 9th steals the oldest, releasing its current key first.

Note-ons and note-offs are consumed; only the walkers' voices reach the output, and everything else passes.

Like the other generators, the walkers run on their own clock rather than waiting for input, and the walk is seeded: the same seed and the same playing wander the same paths; see [Seeds](/miditool/configuration/seeds/). If ticks run late, each walker takes at most 2 catch-up steps and then skips the rest, so time never bunches. Everything stops cleanly on a scene switch and on shutdown; nothing is left sounding.

## Parameters

`interval=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `interval` | duration string | `"80ms"` | at least `20ms` |
| `beats` | number | none (instead of `interval`) | finite, greater than 0 |
| `sigma` | number | `2.0` | `0.5..=12.0` semitones per step |
| `lo` | integer | `21` | key `0..=127`, at most `hi` |
| `hi` | integer | `108` | key `0..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

brownian-walker seed=29 interval="80ms" sigma=2.0
```

Hold one key and a line snakes away from it in small chromatic steps, twelve and a half notes a second; release and it stops mid-thought. Hold three keys and three independent lines tangle.

## Try this

Slow the stride and widen the steps, penned into two octaves:

```kdl
tempo 90

brownian-walker seed=4 beats=0.5 sigma=6.0 lo=48 hi=72
```

Half a beat per step with a wide sigma makes an angular melody that keeps ricocheting off the walls of the range. Then tighten `sigma=0.5`: the walker mostly stays put, wobbling by semitones, a slow trill that occasionally loses its footing.
