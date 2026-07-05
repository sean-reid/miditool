---
title: row-snap
description: Snap every note onto the next pitch class of a twelve-tone row. You supply rhythm, register, and touch; the row supplies the pitches.
---

`row-snap` replaces each note's pitch class with the next element of a twelve-tone row.

You keep the rhythm, the octave, and the dynamics; the row keeps the pitches, wrapping around after twelve the way a Schoenberg line spends the row and starts over. Whatever you play becomes strict serial music with your phrasing. Each note stays in the octave you played it (the very top of the keyboard folds down an octave rather than dropping), and the row advances exactly once per note-on, so no keypress is ever lost.

Note-offs follow their note-ons through the row, so held and overlapping notes always release correctly.

## Parameters

`row-snap` takes the row as twelve bare arguments, a permutation of the pitch classes `0..=11` (0 is C).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| row | integer arguments | required | exactly 12 entries, each pitch class exactly once |
| `form` | string | `"p"` | `"p"`, `"i"`, `"r"`, `"ri"` |
| `transpose` | integer | `0` | `-24..=24` semitones |

`form` selects the classical transformation: prime, inversion (reflected about the row's first note), retrograde, and retrograde inversion. `transpose` shifts the whole row's pitch classes.

## Example

The row scene from `examples/serial.kdl`, the row read in inversion and shifted up a fifth:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

row-snap 0 11 3 4 8 7 9 5 6 1 2 10 form="i" transpose=7
velocity-curve gamma=0.8
```

## Try this

Two instances advance independently, so a [fork](/miditool/configuration/routing/#fork) plays prime and inversion together, a mirror canon in rhythmic unison:

```kdl
fork {
    row-snap 0 11 3 4 8 7 9 5 6 1 2 10
    row-snap 0 11 3 4 8 7 9 5 6 1 2 10 form="i"
}
```

Then swap the second `form` to `"r"` and the canon becomes a crab.
