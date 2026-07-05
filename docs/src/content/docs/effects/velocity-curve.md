---
title: velocity-curve
description: Reshape touch with a gamma curve. Lift soft playing, compress hard playing, and confine velocities to a floor and ceiling.
---

`velocity-curve` reshapes note-on velocities through a gamma curve.

It is the touch control miditool's random effects often need: a scrambled or scattered keyboard invites harder playing than usual, and a gentle curve keeps the result musical. Gamma below 1 lifts soft playing (quiet notes speak more easily); gamma above 1 compresses it (the instrument demands more deliberate weight). `floor` and `ceiling` then confine the result, which is also the quickest way to flatten dynamics entirely for organ- or harpsichord-like behavior.

The mapping is `v -> (v/127)^gamma * 127`, clamped into `floor..=ceiling`. Only note-on velocities are touched; note-offs and controllers pass through.

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

shuffle-lock seed=42
velocity-curve gamma=0.8            // lift the soft end a little
```

## Try this

Terrace the dynamics like a harpsichord: everything sounds at one of two levels, depending on how hard you play.

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
