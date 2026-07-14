---
title: restrike
description: Re-touch every note on a jittered interval, fading toward a floor velocity. Strike a chord once and it keeps returning, softer each time.
---

`restrike` re-touches every note you play, on a loose pulse, each strike softer than the last.

Strike a chord and it does not just decay: it returns, slightly off the grid (`jitter`), a little quieter each time (`decay`), settling toward a whisper until it fades below `floor` or its strikes run out (`max`). The whole series is set the moment the note sounds, each return a short self-contained touch; your release ends only the original note, and the returns keep arriving as they fade, whether or not you are still holding. The lineage is Morton Feldman: soft attacks repeating irregularly at the edge of audibility, patient rather than rhythmic.

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

Touch a note and let it go; it comes back every two seconds or so, dying away.

## Try this

Slower, softer, more patient. Strikes drift up to half the interval off the pulse and fade nearly to silence:

```kdl
restrike seed=9 interval="3.5s" jitter=0.5 decay=0.55 floor=4 max=8
```

Pair it with [`echo`](/miditool/effects/echo/) in one chain (as in the `echoes` example config) and held notes both trail and breathe.
