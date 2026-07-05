---
title: velocity-dice
description: Reroll every note-on velocity with seeded dice, uniform over a range or Gaussian around the played velocity. The notes stay; the touch is redrawn.
---

`velocity-dice` rerolls the velocity of every note you play: the key stays put, the loudness is drawn fresh.

It decouples dynamics from touch the way chance procedures decouple a parameter from the hand that plays it, dynamics assigned by lot rather than instinct. In uniform form every note's loudness comes entirely from the dice, so an even line comes back restless and shaded. In Gaussian form your dynamics are still audibly there, just roughened: small `sigma` is humanization, large `sigma` is weather. Where [`velocity-curve`](/miditool/effects/velocity-curve/) reshapes your touch deterministically, `velocity-dice` ignores it or smears it.

Only note-on velocities are touched; the key never changes and note-offs pass straight through, so every note releases exactly as played. The dice are seeded: the same seed and the same playing roll the same dynamics, forever; see [Seeds](/miditool/configuration/seeds/).

## Parameters

`velocity-dice` has two forms. Giving `sigma` selects the Gaussian form, drawn around the played velocity; otherwise `lo`/`hi` select the uniform form. If both are given, `sigma` wins.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `lo` | velocity | `1` | `1..=127`, at most `hi` |
| `hi` | velocity | `127` | `1..=127` |
| `sigma` | number | none | `0.1..=40.0` velocity steps |

Gaussian draws are rounded and clamped into `1..=127`, so even a wide `sigma` never silences a note or slips off the top.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

velocity-dice seed=4 lo=30 hi=110      // uniform: dynamics from the dice
```

And the Gaussian form:

```kdl title="miditool.kdl"
input "Roland"

velocity-dice seed=4 sigma=6.0         // your dynamics, roughened
```

## Try this

Reroll a grain cloud's dynamics, the `clouds` scene from `examples/clouds.kdl`:

```kdl
poisson-cloud seed=17 density=12 duration="1.5s" sigma=9.0 vel-sigma=12.0 max=20
velocity-dice seed=4 lo=30 hi=110
```

Then, on plain playing, try `sigma=3.0`: just enough grain to stop repeated notes from sounding mechanical, at the edge of noticing.
