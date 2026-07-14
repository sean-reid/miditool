---
title: continuator
description: A pocket Markov continuator. It learns your intervals, pace, and touch while you play, then answers your silence in your own manner; any input silences it instantly.
---

`continuator` listens while you play and answers when you stop.

Trading phrases with an improviser in the improviser's own style was the point of Pachet's Continuator; this is that idea in pocket form. While you play, it learns: a histogram of the intervals between your successive notes (only intervals within two octaves either way are recorded) and running averages of your pace and your velocity. Fall silent for `idle` with no keys held and it continues from your last note on your last channel: intervals drawn from the histogram in proportion to how often you played them, keys reflected back into the piano's range, one note per learned inter-onset time at a velocity jittered a few steps around yours. The continuation ends after `max` notes and then waits; until it has heard two successive notes within two octaves of each other it has no intervals to draw from and stays silent.

Any input silences it instantly. The machine's sounding note is cut off before your event goes through, so you can always interrupt mid-phrase and take back the floor; learning resumes immediately. It never touches your own notes: nothing is consumed, all input passes through unchanged, and the continuation is purely an added voice.

Its trigger is your silence, but the continuator is a [generator](/miditool/how-it-works/#generators) like the others: it runs on its own seeded clock, the same seed and the same playing producing the same answer, and cleans up on a scene switch.

## Parameters

`idle=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `idle` | duration string | `"2s"` | at least `500ms` |
| `beats` | number | none (instead of `idle`) | finite, greater than 0 |
| `max` | integer | `64` | `1..=1000` notes per continuation |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

continuator seed=23 idle="2s" max=64
```

Play a phrase, lift your hands, count two seconds: it picks up where you left off, in your intervals at your pace, until you touch a key again.

## Try this

Make it eager and brief, a call-and-response partner:

```kdl
continuator seed=31 idle="800ms" max=8
```

Every pause of under a second earns an eight-note reply. Feed it stepwise playing and it answers in steps; feed it wide leaps and it leaps. Because the histogram keeps accumulating, its vocabulary is everything you have played this session, weighted toward your habits.
