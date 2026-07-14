---
title: velocity-curve
description: Reshape touch with a gamma curve rescaled into a floor and ceiling. Lift soft playing, tame hard playing, or flatten dynamics entirely.
---

`velocity-curve` reshapes note-on velocities through a gamma curve.

It is the touch control miditool's random effects often need: a scrambled or scattered keyboard invites harder playing than usual, and a gentle curve keeps the result musical. Gamma below 1 lifts soft playing (quiet notes speak more easily); gamma above 1 tames it (the instrument demands more deliberate weight).

The mapping is `floor + (ceiling - floor) * (v/127)^gamma`, rounded and clamped into `1..=127`. `floor` and `ceiling` are not a clamp: they rescale the whole curve, so the full sweep of your touch spreads across `floor..=ceiling`, and setting them equal flattens dynamics entirely. Only note-on velocities are touched; note-offs and controllers pass through.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `gamma` | number | `1.0` | finite, greater than 0 |
| `floor` | velocity | `1` | `1..=127`, at most `ceiling` |
| `ceiling` | velocity | `127` | `1..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

velocity-curve floor=88 ceiling=88      // every note at 88, however struck
```

A harpsichord's indifference: the key speaks at one level no matter how it is hit, and all the expression left is timing.

## Try this

Terrace the dynamics instead: two fixed levels, chosen by how hard you play.

```kdl
fork {
    chain {
        velocity-range lo=1 hi=79
        velocity-curve gamma=1.0 floor=60 ceiling=60
    }
    chain {
        velocity-range lo=80 hi=127
        velocity-curve gamma=1.0 floor=100 ceiling=100
    }
}
```

Then loosen it again: `velocity-curve gamma=0.8` alone lifts the soft end a little and leaves the rest of your touch as it was.
