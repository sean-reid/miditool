---
title: added-value
description: Messiaen's valeur ajoutee as a seeded effect. A share of notes stretches one unit longer or arrives one unit late, so the meter limps and never settles.
---

`added-value` slips Messiaen's added value into your rhythm.

In *Technique de mon langage musical* Messiaen names the *valeur ajoutee*: a short value added to an otherwise regular rhythm, a dot on a note or a small rest slipped in front of one, so the music limps deliciously and the meter never quite settles. Here each note draws two seeded chances at note-on time: with probability `extend` its release is delayed by one `unit` (the added dot), and with probability `defer` its onset arrives one `unit` late (the added rest). Because both decisions are drawn when the note starts, the same seed replays the same limp no matter how you release; see [Seeds](/miditool/configuration/seeds/).

A note deferred into the future keeps its release ordered after it (the note-off is held to at least 10ms past the emitted note-on), so nothing sticks.

## Parameters

`unit=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; the default is `"60ms"`. See [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `unit` | duration string | `"60ms"` | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | none (instead of `unit`) | finite, greater than 0 |
| `extend` | number | `0.3` | `0..=1` |
| `defer` | number | `0.0` | `0..=1` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

added-value seed=11 unit="80ms" extend=0.4     // two notes in five get the dot
```

Play something square and it comes back subtly lame: most notes as played, a seeded share hanging on just a little too long.

## Try this

Add the rest as well as the dot. With both probabilities up, the pulse survives but the surface never repeats:

```kdl
tempo 100

added-value seed=7 beats=0.25 extend=0.35 defer=0.25
```

A quarter-beat unit reads as a metric hiccup rather than a smear. Then shrink it (`unit="30ms"`) and the same lottery turns into loose, humanized timing instead.
