---
title: just
description: 5-limit just intonation. Every note is retuned to the pure ratio for its interval above a root, so thirds and fifths lock the way they did before equal temperament.
---

`just` retunes the whole keyboard to 5-limit just intonation around a root.

This effect needs an MPE-style setup on the receiving instrument; see [Microtonality](/miditool/guides/microtonality/) before wiring it in.

Equal temperament spreads a small error over every interval so that all keys are equally usable; just intonation spends the error unevenly and buys pure intervals in return. This effect uses the classic 5-limit chromatic scale, every ratio built from primes 2, 3, and 5: each note-on is re-emitted at its own key on its own member channel, detuned by the cents deviation for its interval above the root, so thirds stop beating and fifths lock. The full table:

| Semitones above root | Interval | Ratio | Deviation |
| --- | --- | --- | --- |
| 0 | unison | 1/1 | 0.00 cents |
| 1 | minor second | 16/15 | +11.73 cents |
| 2 | major second | 9/8 | +3.91 cents |
| 3 | minor third | 6/5 | +15.64 cents |
| 4 | major third | 5/4 | -13.69 cents |
| 5 | perfect fourth | 4/3 | -1.96 cents |
| 6 | tritone | 45/32 | -9.78 cents |
| 7 | perfect fifth | 3/2 | +1.96 cents |
| 8 | minor sixth | 8/5 | +13.69 cents |
| 9 | major sixth | 5/3 | -15.64 cents |
| 10 | minor seventh | 9/5 | +17.60 cents |
| 11 | major seventh | 15/8 | -11.73 cents |

The root anchors the lattice: deviations are measured by interval above the root pitch class, in every octave, so the tuning favors the keys near the root. With `root="c"`, a C major triad rings pure while a chord rooted far around the circle of fifths picks up the scale's rougher corners; move the root and the sweet spot moves with it. Notes of the root class itself go through the voice pool too, at a bend of zero, so every note follows the same path. The mapping is deterministic, no seed.

Each note-off releases exactly the voice its note-on opened, a retrigger cuts it first, and a scene switch releases everything and resets the bends.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `root` | note name | required | `"c"`, `"f#"`, `"bb"`, or `"0"`..`"11"` |
| `channels` | channel span | `"2-16"` | `"1"`..`"16"`, low end first |
| `bend-range` | semitones | `48` | `1..=96` |

`channels` and `bend-range` are the MPE tail shared by all four microtonal effects; the [Microtonality guide](/miditool/guides/microtonality/) explains how to set them.

## Example

The just scene from `examples/microtonal.kdl`, Pärt bells bent onto pure D minor:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Microtonal"

scene "just" {
    tintinnabuli root="d" minor=true position=1 direction="superior" level=0.6
    just root="d" channels="2-16" bend-range=48
}
```

Hold a D minor triad and listen for the beating to stop: the third sits 15.64 cents high of equal temperament and the chord settles into a single fused sound.

## Try this

Play against the grain of the lattice:

```kdl
just root="c"
```

Hold C major, then move the same shape to F sharp major without changing the root. The C chord is glass; the F sharp chord inherits the tritone's 45/32 and the scale's leftovers, audibly more restless. That unevenness is not a bug, it is what keys sounded like for centuries, and [scordatura](/miditool/effects/scordatura/) lets you build your own version of it by hand.
