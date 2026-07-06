---
title: scordatura
description: A prepared tuning. Chosen pitch classes are detuned by a fixed number of cents in every octave; everything else passes untouched.
---

`scordatura` detunes the pitch classes you name by a fixed number of cents, in every octave.

This effect needs an MPE-style setup on the receiving instrument; see [Microtonality](/miditool/guides/microtonality/) before wiring it in.

This is a prepared tuning, the keyboard cousin of a scordatura string or a bolt dropped on a piano string: a fixed map from pitch class to detune, applied wherever that class appears. Each argument is a `"note=cents"` pair, the note a name like `"c#"` or a pitch class `"0"`..`"11"`, the cents an integer within `-100..=100` with an optional sign, so `"c#=-30"` pulls every C sharp 30 cents flat and `"f=+20"` pushes every F 20 cents sharp. A pitch class may be listed at most once, and the classes you do not list stay at zero.

Detuned classes are re-emitted through the MPE voice pool at their own key with the mapped cents as a per-note pitch bend. Zero-cents classes pass through completely untouched, on their original channel, with no pool voice spent on them, so a map that only bends two classes leaves the other ten exactly as your keyboard sent them. The map is deterministic, no seed.

Each note-off follows whichever path its note-on took, dry or tuned, so releases stay exact even though the two paths live on different channels; a retrigger cuts first, and a scene switch releases everything and resets the bends.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `"note=cents"` pairs | arguments | at least one required | cents `-100..=100`, each pitch class at most once |
| `channels` | channel span | `"2-16"` | `"1"`..`"16"`, low end first |
| `bend-range` | semitones | `48` | `1..=96` |

`channels` and `bend-range` are the MPE tail shared by all four microtonal effects; the [Microtonality guide](/miditool/guides/microtonality/) explains how to set them.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

scordatura "c#=-30" "f=+20" channels="2-16" bend-range=48
```

Most of the keyboard is a normal keyboard. C sharp and F are somewhere else, the same somewhere else in every octave, and lines that pass through them pick up a permanent, placeable wrongness.

## Try this

Turn the black keys into quarter tones:

```kdl
scordatura "c#=-50" "d#=-50" "f#=-50" "g#=-50" "a#=-50"
```

Every black key now sits exactly halfway between its white neighbors, so the keyboard plays a slice of 24-tone equal temperament: chromatic runs become quarter-tone runs. For a tuning derived from ratios instead of a hand-built map, see [just](/miditool/effects/just/).
