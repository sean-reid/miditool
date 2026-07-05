---
title: density-governor
description: Thin the note stream toward a target rate in notes per second. You control the gesture; it controls the statistics.
---

`density-governor` holds the note stream near a target density: you decide what to play, it decides how much of it survives.

This is Xenakis's statistical composition run live: in *Achorripsis* the mean event density of each block was fixed first, and the notes fell wherever the law allowed. The governor measures the note-on rate over a sliding `window` and lets each incoming note pass with probability `target / measured`, capped at 1. At or below the target the dice are never rolled and nothing is touched: sparse playing is entirely yours, and only floods are thinned. That makes it the natural last stop after the effects that multiply notes, [`cluster-fist`](/miditool/effects/cluster-fist/), [`poisson-cloud`](/miditool/effects/poisson-cloud/), [`echo`](/miditool/effects/echo/), whose output can otherwise pile up.

A dropped note-on takes its note-off with it, and a passed note releases normally, so nothing hangs. The thinning is seeded: the same seed thins the same flood the same way, forever; see [Seeds](/miditool/configuration/seeds/).

## Parameters

`window=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `target` | number | required | `0.1..=100.0` notes per second |
| `window` | duration string | `"2s"` | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | none (instead of `window`) | finite, greater than 0 |
| `seed` | integer | `0` | any unsigned 64-bit value |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

density-governor target=6 window="1s" seed=3
```

Play a slow melody and nothing changes; unleash a flurry and it is culled toward six notes a second, the shape of your gesture kept, its census taken.

## Try this

Govern a generator. One finger makes a white-key fistful, a halo rings around it, and the governor keeps the whole storm breathable, the `cowell` scene from `examples/clouds.kdl`:

```kdl
cluster-fist width=6 kind="white" anchor="bottom" rolloff=0.7
resonance-halo width=2 level=0.2 decay="2s"
density-governor target=6 window="1s" seed=3
```

Then squeeze hard: `target=2 window="4s"` reduces anything you do, however frantic, to a slow deliberate trickle.
