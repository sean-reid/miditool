---
title: snap
description: Quantize to a pulse inferred from your own playing, a grid that follows your tempo and phase instead of a metronome.
---

`snap` quantizes your onsets to a beat it learns from you: play, and the grid finds your tempo, leans into your rubato, and pulls each note onto the nearest subdivision.

Where [quantize](/miditool/effects/quantize/) measures you against a fixed metronome, `snap` measures the metronome against you. A phase-locked loop follows your note onsets: each interval between notes updates the tempo estimate, each onset tugs the grid's phase toward where you actually played, and `follow` sets how hard you pull. Low values behave like a patient drummer who holds the tempo while you push against it; high values shadow your rubato so closely that only the small unevenness disappears.

Nothing here is random and there is no seed: the same playing locks the same way every time. Snapping only ever delays a note (live audio cannot rewind), the release moves by the same amount as its note-on so articulation lengths survive, and every note still ends. Chords are treated as one onset, so voicing a chord never bends the tempo. After a long pause the learned tempo is kept but the grid re-anchors on your next note: every phrase starts on your own downbeat.

## Parameters

| Parameter | Type | Default | Range |
|---|---|---|---|
| `division` | integer | `2` | one of `1`, `2`, `3`, `4`, `6`, `8`, `12`, `16` subdivisions per beat |
| `strength` | number | `1.0` | `0..=1`, how far each onset moves toward the grid |
| `follow` | number | `0.35` | `0..=1`, how hard your playing pulls the grid |
| `bpm-lo` | number | `50` | `30..=300`, below `bpm-hi` |
| `bpm-hi` | number | `180` | `30..=300` |

The bpm bounds define what counts as the beat: a run of sixteenths folds into the beat inside the window instead of quadrupling the tempo estimate.

## Example

```kdl
input "Roland"
output virtual="miditool Out"

// Eighth-note snap that keeps a third of your rubato: it reads as
// feel, not correction, and the grid breathes with the phrase.
snap division=2 strength=0.7 follow=0.4
```

## Try this

Meet the two ends of `follow`. At 0.1 the grid is a stubborn drummer: it finds your tempo once and holds it while you push against it. Flip to 0.9 and it breathes with every phrase, erasing only the smallest unevenness. Play the same passage through both and you will hear what your rubato is made of.

```kdl
snap division=2 strength=1.0 follow=0.1

// ...then edit to:
// snap division=2 strength=1.0 follow=0.9
```
