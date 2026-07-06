---
title: mode-lock
description: Lock the keyboard onto one of Messiaen's seven modes of limited transposition. Off-mode notes snap to a member or drop.
---

`mode-lock` forces every note onto one of Messiaen's modes of limited transposition.

These are the seven scales Messiaen catalogued as *modes of limited transposition*: pitch-class sets so symmetrical that shifting them up a few semitones reproduces the same set, which is why each mode has only a handful of distinct transpositions rather than twelve. Mode 1 is the whole-tone scale of *Voiles*; mode 2 is the octatonic scale jazz players call half-whole diminished. Notes already in the (transposed) mode pass untouched; everything else snaps per `snap`.

The mapping is deterministic, no seed. Note-offs map the same way as their note-ons, so held notes always release correctly; a dropped note-on takes its note-off with it.

## The seven modes

Pitch classes are given at `transposition=0`, with 0 as C.

| Mode | Pitch classes | Shape |
| --- | --- | --- |
| 1 | 0 2 4 6 8 10 | whole tone |
| 2 | 0 1 3 4 6 7 9 10 | octatonic (half-whole diminished) |
| 3 | 0 2 3 4 6 7 8 10 11 | tone plus two semitones, three times |
| 4 | 0 1 2 5 6 7 8 11 | two semitones, a minor third, a semitone, twice |
| 5 | 0 1 5 6 7 11 | semitone, major third, semitone, twice |
| 6 | 0 2 4 5 6 8 10 11 | two whole tones and two semitones, twice |
| 7 | 0 1 2 3 5 6 7 8 9 11 | three semitones, a whole tone, a semitone, twice |

`transposition` rotates the whole set upward by that many semitones. Because the modes are symmetrical, only a few values sound distinct (2 for mode 1, 3 for mode 2, 4 for mode 3, 6 for the rest); higher values simply land on a set the mode already produced.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `mode` | integer | required | `1..=7` |
| `transposition` | integer | `0` | `0..=11` semitones |
| `snap` | string | `"nearest"` | `"nearest"`, `"up"`, `"down"`, `"drop"` |

The `snap` semantics are the same as [sieve](/miditool/effects/sieve/)'s: `"nearest"` breaks ties downward, and since every mode repeats within the octave it always finds a member. `"up"` and `"down"` drop the note only past the last member at the very top or bottom of the keyboard. `"drop"` passes members and silences the rest.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

mode-lock mode=2 transposition=0 snap="nearest"
```

Everything you play lands on the octatonic scale on C; runs come out with that even, rootless Messiaen glitter no matter what your fingers meant.

## Try this

Thin the keyboard to mode 5, the sparsest of the seven, and keep only what is already there:

```kdl
mode-lock mode=5 snap="drop"
```

Only six pitch classes survive per octave, so most keys go silent and chords arrive with holes in them. Then switch to `snap="up"` and the dead keys climb to the nearest member above instead, turning clusters into tight mode-5 voicings.
