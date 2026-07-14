---
title: ring-mod
description: Ring modulation for notes. Each key becomes its sum and difference tones with a fixed carrier; consonances survive, everything else clangs.
---

`ring-mod` ring-modulates every note against a fixed carrier key.

In *Mantra*, Stockhausen fed two pianos through ring modulators; this effect is that electronics reduced to notes: the played frequency and a carrier combine, and what comes out is their sum and difference, metallic where the originals were plain.

Every emitted component gets its own note-off, and a retrigger cuts the whole set first, so nothing sticks. Components that fall off the keyboard are dropped; components of one note that land on the same key sound once.

## Parameters

At least one of `sum`, `diff`, and `dry` must be true.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `carrier` | key number | required | `0..=127` |
| `sum` | boolean | `true` | `true`, `false` |
| `diff` | boolean | `true` | `true`, `false` |
| `dry` | boolean | `false` | `true`, `false` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

ring-mod carrier=60          // sum and difference tones against middle C
```

Play octaves and fifths of C and the output stays harmonic; drift a tritone away and it turns to metal.

## Try this

Drop the carrier low and keep the dry note:

```kdl
ring-mod carrier=33 dry=true
```

Against A1 the sum and difference hug the played note, so every key arrives with an upper and lower neighbor, a cluster halo around your line. Then push `carrier=96` and the halo tears apart into gong partials.
