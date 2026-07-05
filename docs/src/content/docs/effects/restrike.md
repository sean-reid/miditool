---
title: restrike
description: Re-strike held notes on a jittered interval, fading toward a floor velocity. Hold a chord and it breathes.
---

`restrike` re-strikes the notes you hold, on a loose pulse, each strike softer than the last.

Hold a chord and it does not just sit there: it returns, slightly off the grid (`jitter`), a little quieter each time (`decay`), settling toward a whisper (`floor`) until its strikes run out (`max`) or you release. Long tones become slow pulsations; a held cluster becomes a texture that breathes. The lineage is Morton Feldman: soft attacks repeating irregularly at the edge of audibility, patient rather than rhythmic.

The jitter is seeded, so a take's timing is [reproducible](/miditool/configuration/seeds/).

## Parameters

Exactly one of `interval=` or `beats=` must be given; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `interval` | duration string | required (or `beats`) | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | required (or `interval`) | finite, greater than 0 |
| `jitter` | number | `0.15` | `0..=0.9`, fraction of the interval |
| `decay` | number | `0.7` | greater than 0, less than 1 |
| `floor` | velocity | `8` | `1..=127` |
| `max` | integer | `12` | `1..=24` strikes per note |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

restrike seed=9 interval="2s" jitter=0.2
```

Hold anything for more than two seconds and it starts to pulse.

## Try this

Slower, softer, more patient. Strikes drift up to half the interval off the pulse and fade nearly to silence:

```kdl
restrike seed=9 interval="3.5s" jitter=0.5 decay=0.55 floor=4 max=8
```

Pair it with [`echo`](/miditool/effects/echo/) in one chain (as in the `echoes` example config) and held notes both trail and breathe.
