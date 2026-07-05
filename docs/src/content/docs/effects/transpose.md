---
title: transpose
description: Shift notes up or down by a fixed number of semitones. Notes leaving the MIDI range are dropped.
---

`transpose` shifts every note by a fixed number of semitones.

Alone it is a transposition; inside a [`fork`](/miditool/configuration/routing/#fork) it is an interval machine. A branch with `transpose 12` doubles in octaves, `transpose 7` shadows you in fifths, and stacking branches builds organ-style registrations from a single keyboard. Notes shifted past either end of the MIDI range (0 to 127) are dropped rather than folded back.

## Parameters

`transpose` takes one bare argument, not a named property.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| semitones | integer argument | required | `-127..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

transpose -12       // play everything an octave lower
```

## Try this

A drone of open fifths under everything you play:

```kdl
fork {
    pass
    transpose 7
    transpose -12
}
```

The [fork merge](/miditool/configuration/routing/#fork) drops exact duplicates, so pedal and mod-wheel traffic still comes through once, not three times.
