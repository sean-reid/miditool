---
title: accent-groups
description: Additive accents by count. Note-ons cycle through group lengths and the first of each group lands hard, so meter is built from grouping, not from a clock.
---

`accent-groups` accents the first note of each group, counting notes rather than watching a clock.

Additive rhythm builds its meter by grouping a fast pulse (3+3+2, 3+5) instead of dividing a slow one; Ligeti's *Desordre* drives the idea to its edge, hammering 3+5 and 5+3 accent groups against each other until the barlines shear apart. Here successive note-ons cycle through your group lengths: the first note of each group is raised to at least `accent`, and the rest are held down to at most `rest`. The shaping is a floor and a ceiling, not a rewrite, so a downbeat you genuinely hammer keeps its force and an inner note you genuinely brush stays soft.

No clock is involved: only the order of note-ons matters, so the grouping breathes with your tempo, rubato and all. Note-offs pass untouched and do not advance the count.

## Parameters

The group lengths are bare arguments, at least one; the list cycles in the order written.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| groups | integer arguments | required | each `1..=16` |
| `accent` | velocity | `112` | `1..=127` |
| `rest` | velocity | `64` | `1..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

accent-groups 3 5 accent=112 rest=64      // ONE two three ONE two three four five
```

Run a scale evenly and it comes back phrased: the 3+5 grouping emerges from dynamics alone.

## Try this

Place the accents on a grid first. Gated onto the tresillo, the counted groups land on the surviving onsets and the two patterns interfere:

```kdl
tempo 100

euclidean-gate k=3 n=8 beats=0.5
accent-groups 3 3 2 accent=118 rest=72
```

The [`euclidean-gate`](/miditool/effects/euclidean-gate/) decides *when* notes sound and the groups decide *which* of them speak up; since eight onsets carry a 3+3+2 accent cycle, the loud ones migrate around the pattern instead of nailing it down.
