---
title: poisson-cloud
description: Every note-on sprays a seeded grain cloud around itself, Poisson in time, Gaussian in pitch and velocity, dying away over its duration.
---

`poisson-cloud` scatters a decaying swarm of grains behind every note you play.

One key press becomes a small stochastic texture: grain onsets follow a Poisson process at `density` grains per second, thinning out to nothing across `duration`, and each grain lands near your note (Gaussian, `sigma` semitones wide) with a velocity that fades as the cloud dies plus its own noise (`vel-sigma`). This is composition by distribution law in the manner of Xenakis's *Pithoprakta* and *Achorripsis*: not which notes, but how many per second, spread how far. You play the center of mass; the cloud does the rest.

The played note passes through unchanged and releases when you release it. Every grain carries its own note-off (a grain lasts until the next arrival, at most 120ms), so note-ons and note-offs always balance and nothing hangs; a grain that wanders off the keyboard is dropped whole. The cloud is seeded: the same seed and the same playing spray the same grains, forever; see [Seeds](/miditool/configuration/seeds/).

## Parameters

`duration=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `density` | number | `8.0` | `0.1..=50.0` grains per second |
| `duration` | duration string | `"2s"` | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | none (instead of `duration`) | finite, greater than 0 |
| `sigma` | number | `7.0` | `0..=24` semitones |
| `vel-sigma` | number | `10.0` | `0..=40` velocity steps |
| `max` | integer | `16` | `1..=24` grains per note |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

poisson-cloud seed=17 density=12 duration="1.5s" sigma=9.0 vel-sigma=12.0 max=20
```

Play single staccato notes and each one bursts into a second and a half of scattered aftermath. This is the `clouds` scene from `examples/clouds.kdl`, which follows it with [`velocity-dice`](/miditool/effects/velocity-dice/) to reroll every grain's dynamics.

## Try this

Set `sigma=0.0` and the whole cloud stays on the played key: a granular tremolo dying away.

```kdl
tempo 84

poisson-cloud seed=5 density=4 beats=4 sigma=0.0 vel-sigma=20.0 max=24
```

Then widen it back out: `sigma=14.0` at this low density is sparse pointillism, stray notes flickering one at a time around whatever you touched.
