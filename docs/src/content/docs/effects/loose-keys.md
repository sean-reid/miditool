---
title: loose-keys
description: Every press draws a fresh note, uniform over a range or Gaussian around the played key. The same key twice gives different notes.
---

`loose-keys` redraws every note you play: the same key twice gives two different notes.

Where [`shuffle-lock`](/miditool/effects/shuffle-lock/) is a fixed remapping, `loose-keys` is a note cloud. In uniform form the keyboard becomes a trigger surface: any key fires a random note from a range, and what you control is rhythm and dynamics. In Gaussian form the notes scatter *around* what you play, so your line is still audibly there, just smeared; small `sigma` is a nervous vibrato of pitch, large `sigma` is a cloud with your melody as its center of mass.

The draws are seeded, so a recorded take is reproducible: the same seed and the same playing give the same notes. Note-offs always match their note-ons, whatever was drawn.

## Parameters

`loose-keys` has two forms. Giving `sigma` selects the Gaussian form; otherwise `lo`/`hi` select the uniform form. If both are given, `sigma` wins.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `lo` | key number | `21` (A0) | `0..=127`, at most `hi` |
| `hi` | key number | `108` (C8) | `0..=127` |
| `sigma` | number | none | finite, greater than 0; in semitones |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

loose-keys seed=7 lo=48 hi=84      // uniform: three octaves around middle C
```

And the Gaussian form:

```kdl title="miditool.kdl"
input "Roland"

loose-keys seed=7 sigma=3.5        // notes wander a few keys from what you play
```

## Try this

Humanize only the accents. Loud hits fire random high notes while normal playing passes untouched:

```kdl
fork {
    pass
    chain {
        notes-only
        velocity-range lo=100 hi=127
        loose-keys seed=11 lo=72 hi=96
    }
}
```

Then shrink `sigma` in a Gaussian patch to `0.5` and listen to it hover at the edge of wrong.
