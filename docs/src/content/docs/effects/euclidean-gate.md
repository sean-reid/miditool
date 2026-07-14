---
title: euclidean-gate
description: Gate notes through a Euclidean rhythm. You propose the onsets; a grid of k open pulses spread evenly over n steps decides which of them sound, and when.
---

`euclidean-gate` gates your notes through a Euclidean rhythm: you propose the onsets, the grid decides which of them sound.

Godfried Toussaint observed that the Euclidean algorithm, asked to spread `k` pulses as evenly as possible over `n` steps, generates a startling share of the world's rhythms: `k=3 n=8` is the tresillo of Cuban son, `10010010`. The effect runs an endless grid of steps `pulse` long, with `k` open steps spread as evenly as possible over every `n`, the whole pattern shifted by `rotation`. The grid is anchored at the first note-on the effect ever sees: that note lands in step zero and fixes where every later step falls, for good. It never re-anchors, so after a pause you re-enter wherever the running pattern happens to be; with the default `rotation=0` step zero is open, and the anchoring note itself sounds.

A note arriving in a closed step is handled by `mode`. With `"defer"`, the default, it waits and sounds at the start of the next open step, so a steady stream of playing comes back already phrased as the pattern; a note arriving inside an open step sounds immediately. With `"drop"` it simply vanishes, its release swallowed with it. A note deferred into the future keeps its release ordered after it (the note-off is held to at least 10ms past the emitted note-on), so nothing sticks.

## Parameters

`pulse=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; left out entirely, the step is a quarter of a beat. See [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `k` | integer | required | `1..=64`, at most `n` |
| `n` | integer | required | `1..=64` |
| `rotation` | integer | `0` | `0..=n-1` |
| `pulse` | duration string | a quarter beat | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | none (instead of `pulse`) | finite, greater than 0 |
| `mode` | string | `"defer"` | `"defer"`, `"drop"` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

tempo 100

euclidean-gate k=3 n=8 beats=0.5      // the tresillo, an eighth note per step
```

Play even eighths and they come back as the tresillo; play freely and every onset is pulled onto the nearest open step ahead of it.

## Try this

Turn the gate into a sieve in time. With `mode="drop"`, whatever misses the pattern is gone, and your own accuracy becomes the instrument:

```kdl
tempo 110

euclidean-gate k=5 n=16 beats=0.25 mode="drop"
```

Then nudge `rotation=3` to shift where the open steps fall against your downbeat, or follow the gate with [`accent-groups`](/miditool/effects/accent-groups/) so the survivors get phrased as well as placed.
