---
title: shuffle-lock
description: A seeded permutation of the keys. The keyboard is scrambled but stable, so you can learn the scrambled instrument.
---

`shuffle-lock` permutes the keyboard: every key plays some other key, and the mapping never changes.

Press C4 and get, say, F#5, every single time. The keyboard becomes a strange but *stable* instrument. Melodies you know come out as lines you would never write, voice leading turns into leaps, and yet the whole thing rewards practice, because the scramble is locked to its seed. Same seed, same scramble, forever; see [Seeds](/miditool/configuration/seeds/).

Note-offs follow the same mapping as their note-ons, so held and overlapping notes always release correctly.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `lo` | key number | `21` (A0) | `0..=127`, at most `hi` |
| `hi` | key number | `108` (C8) | `0..=127` |
| `mode` | string | `"free"` | `"free"`, `"within-octave"`, `"within-pitch-class"` |

Only keys in `lo..=hi` are permuted; the defaults cover the 88 keys of a piano. Keys outside the range pass through unchanged.

`mode` constrains the permutation:

- `"free"`: any key may land anywhere in the range.
- `"within-octave"`: keys stay in their octave. Registers keep their character; lines stay in place but the intervals scramble.
- `"within-pitch-class"`: keys keep their pitch class and move between octaves. Harmony survives; every C is still a C, just not the one you pressed. Chords splay across the keyboard.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

shuffle-lock seed=42 lo=21 hi=108 mode="free"
velocity-curve gamma=0.8
```

## Try this

Keep the harmony and scramble only the registers:

```kdl
shuffle-lock seed=42 mode="within-pitch-class"
```

Play a chorale. The voices explode across seven octaves but the progression is untouched. Then [reroll the seed live](/miditool/guides/live-editing/) until the spacing sits well.
