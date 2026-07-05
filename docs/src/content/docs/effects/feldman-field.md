---
title: feldman-field
description: Compress every velocity into a whisper-quiet band with a seeded jitter. Your shaping survives in miniature; the room never gets loud.
---

`feldman-field` sinks every velocity into a whisper.

Morton Feldman kept whole evening-length pieces at the edge of audibility, a dynamic world so compressed that a mezzo-piano counts as an event; this is that field as a compressor for touch. Each note-on velocity is mapped linearly from the full `1..=127` down into `floor..=ceiling`, then nudged by a seeded jitter of up to `jitter` either way so the surface still breathes. Your shaping survives in miniature: what you played loud is still the loudest thing in the room, but the room now tops out at a murmur.

The jitter follows its seed: the same seed gives the same field forever; see [Seeds](/miditool/configuration/seeds/). Note-offs and everything else pass untouched.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | `0` | any unsigned 64-bit value |
| `floor` | velocity | `8` | `1..=127`, at most `ceiling` |
| `ceiling` | velocity | `28` | `1..=127` |
| `jitter` | integer | `4` | `0..=20` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

feldman-field seed=6 floor=6 ceiling=24 jitter=3
```

Play however you like; it all comes back between a breath and a murmur, faintly alive.

## Try this

Build the whole late-Feldman room. Long dealt durations, the whisper field, and one permitted disturbance a minute:

```kdl
duration-lottery seed=5 mean="2s" min="500ms" max="8s"
feldman-field seed=6 floor=6 ceiling=22 jitter=3
```

Then set `jitter=0` and `floor=15 ceiling=15` to flatten the field completely: every note identical, and all the music left in *where* you place them. Or follow the field with [`anti-accent`](/miditool/effects/anti-accent/) at a level below its band, so that within the whisper one note per window is allowed to stand slightly taller than the rest.
