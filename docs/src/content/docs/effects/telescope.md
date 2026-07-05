---
title: telescope
description: Scale every interval from a reference key. Stretch the keyboard apart or squeeze it toward a pivot.
---

`telescope` scales every note's distance from a reference key: intervals stretch or squeeze.

The mapping is `out = reference + round((key - reference) * factor)`. Above 1 the keyboard pulls apart: at `factor=2` a fifth up from the reference becomes a ninth, and a modest hand span covers four octaves. Below 1 it collapses inward: at `factor=0.5` an octave shrinks to a tritone, wide arpeggios crowd into clusters, and neighboring keys start landing on the same note. The reference key always maps to itself.

The mapping is deterministic, no seed. A result outside the MIDI range drops the note, and its note-off with it; everything that sounds releases correctly.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `factor` | number | required | `0.1..=8.0` |
| `reference` | key number | `60` (C4) | `0..=127` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

telescope factor=2.0          // every interval from middle C doubled
```

## Try this

Play against your own stretched shadow:

```kdl
fork {
    pass
    telescope factor=2.0
}
```

Every gesture sounds at true size and at double magnification simultaneously, agreeing only on middle C. Then invert the idea with `factor=0.25`: whatever you play, the shadow barely moves, a drone that leans the way you lean.
