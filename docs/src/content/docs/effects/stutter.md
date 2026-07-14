---
title: stutter
description: Ratchet each note into a burst of rapid hits, with gaps that stretch or tighten as the burst plays out.
---

`stutter` ratchets each note into a burst of rapid hits.

Your note passes as played, and `repeats` re-attacks follow it in quick succession: one press, `repeats + 1` hits. The first gap lasts `first`, and each following gap is scaled by `curve`: above 1 the burst decelerates, like a ball settling; below 1 it accelerates into a buzz; at exactly 1 it is a strict ratchet. On a percussive DAW instrument this is drum-machine ratcheting from a piano keyboard; on sustained sounds it turns lines into tremolo figures.

## Parameters

Exactly one of `first=` or `beats=` must be given; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `repeats` | integer | `6` | `1..=24` |
| `first` | duration string | required (or `beats`) | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | required (or `first`) | finite, greater than 0 |
| `curve` | number | `1.0` | `0.25..=4.0` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"
tempo 100

stutter repeats=6 beats=0.25           // seven hits, a sixteenth apart
velocity-dice seed=3 sigma=8.0         // roughen the ratchet's surface
```

The dice keep the burst from sounding machine-flat: each hit lands a shade harder or softer than the last.

## Try this

The bouncing-ball: hits start fast and spread out as they settle.

```kdl
stutter repeats=10 first="25ms" curve=1.5
```

Then invert it (`first="120ms" curve=0.5`) so every note gathers itself into a buzz, and put a [`velocity-curve`](/miditool/effects/velocity-curve/) after it to keep the bursts from shouting.
