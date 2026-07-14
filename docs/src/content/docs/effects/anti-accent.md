---
title: anti-accent
description: Cap every velocity at a quiet level, except that once per window a single loud note is allowed through untouched. Late Feldman as a governor.
---

`anti-accent` keeps everything soft, except one permitted thunderclap per window.

Late Feldman earns its rare fortissimo by withholding it: after long stretches at a murmur, a single loud attack rearranges the whole room. This effect is that economy as a governor. Every note-on above `level` is pressed down to it, except that once per rolling window of `every` one loud note passes untouched: the first candidate to arrive after the window has elapsed since the last allowance (the very first loud note of a performance is allowed, nothing having been spent yet). Notes at or below the cap pass untouched and never spend the allowance.

The window rolls from the last thunderclap, not from a wall clock, so the indulgences space themselves against the music rather than the minute hand. The effect is fully deterministic, the same playing getting the same thunderclaps; `seed` is reserved for future selection modes and currently draws nothing.

## Parameters

`every=` takes a duration string like `"30s"`, or `beats=` against the tempo, never both; it must come to at least one second. See [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `level` | velocity | `30` | `1..=127` |
| `every` | duration string | `"30s"` | at least `1s` |
| `beats` | number | none (instead of `every`) | finite, at least `1s` once resolved |
| `seed` | integer | `0` | any unsigned 64-bit value |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

anti-accent level=30 every="30s"
```

Play as wildly as you like: it all comes out at 30 or below, and roughly twice a minute your loudest impulse actually lands.

## Try this

Stretch the window until the thunderclap becomes an event you wait for:

```kdl
duration-lottery seed=5 mean="2s" min="500ms" max="8s"
anti-accent level=24 every="90s"
```

Long dealt tones at a murmur, and every minute and a half the piece is allowed one outburst. Notice what your hands start doing: knowing the allowance has recharged, you begin *placing* the loud note, which is exactly the discipline the effect is imitating.
