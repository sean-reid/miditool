---
title: overtone-pedal
description: While the sustain pedal is down, every note snaps to the nearest partial of one fundamental, tuned exactly. Pedal up restores the normal keyboard.
---

`overtone-pedal` confines your playing to the harmonic series of one fundamental while the sustain pedal is down.

This effect needs an MPE-style setup on the receiving instrument; see [Microtonality](/miditool/guides/microtonality/) before wiring it in.

Think of one long string tuned to `fundamental`, and the pedal as your hand deciding whether the keyboard plays notes or harmonics of that string. The sustain pedal (CC64, tracked per channel, 64 or higher is down, and the CC itself still passes through to the receiver) is the switch. While it is down, each note-on is compared against the partials 1 through `partials` of the fundamental, which sit at `12 * log2(k)` semitones above it; the nearest partial wins (the lower partial on a tie), and the note is re-emitted through the MPE voice pool at the nearest key with the remainder as a per-note pitch bend, exactly on the series. So keys near a partial pull onto it, and a cluster under the pedal comes out as a spaced, ringing chord of harmonics.

The effect knows when to give up: a note below the fundamental, a note with no partial within 6 semitones (the gaps between low partials are wide), or a snap that would leave the keyboard passes through dry instead of being wrenched onto the series. Pedal up, everything passes dry and the keyboard is a normal keyboard again. The snapping is deterministic, no seed.

Each note remembers whether it went out dry or tuned, so its note-off releases the right note even when the pedal moved between the press and the release; a retrigger cuts first, and a scene switch releases everything and resets the bends.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `fundamental` | key number | required | `0..=127` |
| `partials` | integer | `16` | `1..=32` |
| `channels` | channel span | `"2-16"` | `"1"`..`"16"`, low end first |
| `bend-range` | semitones | `48` | `1..=96` |

`channels` and `bend-range` are the MPE tail shared by all four microtonal effects; the [Microtonality guide](/miditool/guides/microtonality/) explains how to set them.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

overtone-pedal fundamental=36 partials=16 channels="2-16" bend-range=48
```

Play freely with the pedal up. Press the pedal and everything you touch becomes an overtone of low C: seconds turn into pure ninths and sevenths of the series, and whatever you play belongs to one spectrum until you lift the pedal.

## Try this

Shrink the series and watch the keyboard coarsen:

```kdl
overtone-pedal fundamental=48 partials=6
```

With only six partials the grid is sparse, so whole regions of the keyboard collapse onto the same few harmonics and high notes past the 6-semitone reach pass through dry. Then raise `partials` toward `32` and the top of the keyboard becomes nearly chromatic while the bottom octave over the fundamental stays wide open, the true shape of the harmonic series under your hands.
