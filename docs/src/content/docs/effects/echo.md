---
title: echo
description: Fading repeats after each note, each one softer and optionally shifted in pitch, which turns the echo into a canon cascade.
---

`echo` gives each note fading repeats, like a delay pedal for notes rather than audio.

Each repeat arrives one `time` later and `decay` times softer, so a struck chord trails away in steps. Because these are notes, not audio, every repeat retriggers the instrument in the DAW, and each repeat can be *transposed*: set `transpose` and the echo becomes a canon cascade, every note chased by copies of itself climbing or sinking by a fixed interval as they fade.

## Parameters

Exactly one of `time=` or `beats=` must be given; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `repeats` | integer | `3` | `1..=16` |
| `time` | duration string | required (or `beats`) | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | required (or `time`) | finite, greater than 0 |
| `decay` | number | `0.6` | greater than 0, at most 1 |
| `transpose` | integer | `0` | `-24..=24` semitones |

`decay=1` repeats at constant volume. Repeats that would transpose past the MIDI range are dropped.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

echo repeats=4 beats=0.5 decay=0.7      // half-beat repeats, fading
```

## Try this

The canon cascade. Every note is chased up in fourths as it fades:

```kdl
tempo 84

echo repeats=5 beats=0.75 transpose=5 decay=0.75
```

Then flip it: `transpose=-12 decay=0.85` sinks each note through the octaves below, almost a piano's sustain pedal made of notes.
