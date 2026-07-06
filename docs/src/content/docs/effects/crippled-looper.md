---
title: crippled-looper
description: Feldman's crippled symmetry as a pedal looper. Hold the pedal, play a phrase, release; it circles forever, each pass altered by exactly one seeded change, most of which stick.
---

`crippled-looper` turns the pedal into a phrase looper that never repeats itself exactly.

The model is late Feldman's crippled symmetry, patterns that recur but never quite line up, each return slightly and permanently askew. Hold the pedal (CC `pedal`, the sustain pedal by default; a value of 64 or higher counts as down, on any channel) and play. Up to `max` notes are captured with their keys, velocities, onsets, and durations; the recorder holds at most 32 notes, and `max` can only narrow that. Release the pedal and the phrase starts circling. The loop is as long as the pedal was down, or the phrase plus a 50ms tail when a note outlasts the stroke, and it repeats until you pedal again.

Every pass, the first included, is altered by exactly one seeded change, drawn uniformly among four: nudge one note's onset by a tenth of the loop length in a drawn direction (clamped so the note stays inside the loop), step one note's velocity by 12 up or down (clamped to `1..=127`), silence one note for that pass only, or swap the onsets of two adjacent notes. Every change except the silence sticks, so the phrase slowly warps as it circles; after a few minutes the loop is a distant relative of what you played. A single-note phrase skips the swap.

The pedal CC is consumed: the DAW never sees it, so nothing sustains while this effect is in the chain. Point `pedal=` at another CC to keep your sustain, or give the looper a scene of its own; see [Performing](/miditool/guides/performing/). Your own notes always pass through unchanged, pedal down or up: the machine adds a voice and never consumes a note. Pedal down again to capture anew; the running loop is silenced, note-off for note-off, and the new capture replaces the phrase. Notes beyond `max` still pass but are not recorded, a note still held at pedal-up has its duration capped there, and a capture with no notes leaves the machine silent.

The warp is seeded: the same seed and the same playing give the same slow deformation; see [Seeds](/miditool/configuration/seeds/). If ticks run late, the looper finishes the pass in progress and at most one more, then jumps to the next loop boundary; the skipped passes draw no mutation, so the schedule never bunches. Note-offs are never skipped: every machine note carries its own, a new strike cuts the previous instance of the same note, and a scene switch or shutdown releases everything still sounding, your held notes included.

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

Hold the sustain pedal, play a short phrase, release. It comes back at once and keeps coming back, each pass with one detail moved, softened, silenced, or swapped, and all but the silences kept. Pedal again whenever you want to teach it a new phrase.

## Try this

Sink the whole room, your playing and the loop alike, into a Feldman whisper:

```kdl
crippled-looper seed=17 max=12
feldman-field seed=5 floor=8 ceiling=30 jitter=3
```

Twelve notes is a comfortable phrase to hold in the ear while it deforms. Reroll `seed=` for a different deformation of the same phrase; the seed decides which detail warps on which pass, so it is as much a part of the piece as the notes are.
