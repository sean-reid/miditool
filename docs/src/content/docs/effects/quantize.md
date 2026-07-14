---
title: quantize
description: Pull your onsets onto a time grid, live. A stream that only flows forward can be delayed but never rewound, and strength blends between your timing and the grid's.
---

`quantize` pulls your onsets onto a time grid as you play.

For a grid that follows your tempo instead of holding one, see [snap](/miditool/effects/snap/).

It is the metronomic snap of the drum machine and the DAW piano roll, brought to a live stream, with one honest difference: a recording can be nudged backwards and forwards, but live quantization can only wait, never rewind. Each note-on is aimed at the nearest grid point, and the correction is clamped forward; a note whose nearest point is already behind it passes unchanged, while a note leaning toward the next point is held until that point arrives. The grid is anchored at your first note, which passes untouched and becomes the origin: you set the downbeat, the grid follows from there.

`strength` blends between you and the grid. The emitted time is `arrival + strength * (target - arrival)`: at `1` notes land square on the grid, at `0.5` they move halfway there (tighter, still human), and at `0` the effect is a bypass. A note held to a future grid point keeps its release ordered after it (the note-off waits at least 10ms past the emitted note-on), so nothing sticks.

## Parameters

`grid=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; left out entirely, the grid is a quarter of a beat. See [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `grid` | duration string | a quarter beat | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | none (instead of `grid`) | finite, greater than 0 |
| `strength` | number | `1.0` | `0..=1` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

tempo 120

// Full snap is a drum machine. At 0.7 a third of your rubato
// survives, and the grid reads as feel rather than correction.
quantize beats=0.25 strength=0.7
```

## Try this

Tighten without robotizing. At `strength=0.6` your rushing and dragging survive at reduced size, which reads as intent rather than error:

```kdl
tempo 96

quantize beats=0.5 strength=0.6
```

Then push it the other way: a coarse grid at full strength (`beats=1 strength=1`) turns any keyboard noodling into a solemn processional, one event per beat at most as far as onsets go.
