---
title: wedge-mirror
description: Reflect notes around an axis key, always or with seeded probability. Lines above the axis answer below it in contrary motion.
---

`wedge-mirror` reflects notes around an axis key: what you play above it sounds below, and the other way around.

The mapping is `out = 2 * axis - key`, the contrary-motion wedge of a Bartok mirror or an inversion canon. A rising line falls, a falling line rises, and the axis key maps to itself. With `probability` below 1 each note tosses a seeded coin and only that share of them mirrors, so your line frays into two voices leaning away from each other.

The same seed gives the same coin tosses forever; see [Seeds](/miditool/configuration/seeds/). Each note-off lands on whichever side its note-on chose, so held notes always release correctly. A reflection that leaves the MIDI range drops the note, and its note-off with it.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `axis` | key number | `60` (C4) | `0..=127` |
| `probability` | number | `1.0` | `0..=1` |
| `seed` | integer | `0` | any unsigned 64-bit value; only matters below `probability=1` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

// A mirror canon in one line. Around D, the white keys map onto
// themselves, so diatonic playing answers itself in the key.
fork {
    pass
    wedge-mirror axis=62
}
```

## Try this

Mirror only half your notes:

```kdl
wedge-mirror axis=66 probability=0.5 seed=7
```

Or keep the original and add its inversion, an instant mirror canon in rhythmic unison:

```kdl
fork {
    pass
    wedge-mirror axis=60
}
```

The [fork merge](/miditool/configuration/routing/#fork) plays both voices; the axis note itself comes through once, not doubled.
