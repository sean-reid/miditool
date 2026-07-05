---
title: mass-crescendo
description: A slow periodic envelope under all your dynamics, minutes long. Ramp rises and resets; arch swells and subsides. Dynamics become the architecture.
---

`mass-crescendo` puts a slow tide under your dynamics: form measured in minutes, not bars.

Dynamics can *be* the architecture, the way Ravel's Bolero is nothing but one quarter-hour crescendo. This effect scales every note-on velocity by a periodic envelope anchored at your first note: with `shape="arch"`, the default, it rises to full strength at the period's midpoint and falls away again, a long swell; with `shape="ramp"` it climbs across the whole period and resets, a sawtooth that keeps beginning again. The scale runs from `1 - depth` at the envelope's floor up to full strength at its peak, so `depth=0` changes nothing and `depth=1` sinks the troughs to a whisper.

Your local shaping rides on top: accents stay accents, the tide just decides how high the whole sea is. Nothing here is random and there is no seed; the same period gives the same architecture every time. Note-offs and everything else pass untouched.

## Parameters

`period=` takes a duration string like `"120s"`, or `beats=` against the tempo, never both; it must come to at least one second. See [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `period` | duration string | `"120s"` | at least `1s` |
| `beats` | number | none (instead of `period`) | finite, at least `1s` once resolved |
| `depth` | number | `0.6` | `0..=1` |
| `shape` | string | `"arch"` | `"arch"`, `"ramp"` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

mass-crescendo period="120s" depth=0.6      // a two-minute swell and release
```

Improvise for a few minutes and listen back: whatever you played, it now has a shape.

## Try this

The Bolero engine. A deep ramp in beats keeps the architecture locked to the tempo:

```kdl
tempo 120

mass-crescendo beats=128 depth=0.9 shape="ramp"
```

Everything climbs out of near-silence over 64 seconds, then the floor drops out and the climb begins again. Feed it something already patterned, say a [`euclidean-gate`](/miditool/effects/euclidean-gate/) upstream, and the repetition plus the ramp does most of the composing for you.
