---
title: crippled-looper
description: Feldman's crippled symmetry as a pedal looper. Hold the pedal, play a phrase, release; it circles forever, each pass altered by exactly one seeded change, most of which stick.
---

`crippled-looper` turns the pedal into a phrase looper that never repeats itself exactly.

Late Feldman is built from patterns that recur but never quite line up, each return slightly and permanently askew; he called it crippled symmetry, and this is that principle as a looper. Hold the pedal (CC `pedal`, the sustain pedal by default; a value of 64 or higher counts as down, on any channel) and play; your notes are captured with their keys, velocities, onsets, and durations. Release, and the phrase starts circling: the loop runs as long as the pedal was down (50ms longer when the last note ends right at pedal-up) and repeats until you pedal again.

Every pass, the first included, is altered by exactly one seeded change, drawn uniformly among four: nudge one note's onset by a tenth of the loop length, step one note's velocity by 12 up or down, silence one note for that pass only, or swap the onsets of two adjacent notes. Every change except the silence sticks, so the phrase slowly warps as it circles; after a few minutes the loop is a distant relative of what you played.

The pedal CC is consumed, so nothing sustains while this effect is in the chain; point `pedal=` at another CC to keep your sustain, or give the looper a scene of its own ([Performing](/miditool/guides/performing/)). Your own notes always pass through unchanged, pedal down or up: the machine adds a voice and never consumes a note. Pedal down again to silence the loop and capture anew; a note still held at pedal-up has its duration capped there.

The looper is a [generator](/miditool/how-it-works/#generators): it runs on its own seeded clock, the same seed and the same playing giving the same slow deformation, and cleans up on a scene switch.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `pedal` | integer | `64` (the sustain pedal) | a CC number, `0..=127` |
| `max` | integer | `16` | `2..=32` notes per capture |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

crippled-looper seed=17 pedal=64 max=16
```

Hold the sustain pedal, play a short phrase, release. It comes back at once and keeps coming back, one detail askew per pass. Pedal again whenever you want to teach it a new phrase.

## Try this

Sink the whole room, your playing and the loop alike, into a Feldman whisper:

```kdl
crippled-looper seed=17 max=12
feldman-field seed=5 floor=8 ceiling=30 jitter=3
```

Twelve notes is a comfortable phrase to hold in the ear while it deforms. Reroll `seed=` for a different deformation of the same phrase; the seed decides which detail warps on which pass, so it is as much a part of the piece as the notes are.
