---
title: metronome-swarm
description: Ligeti's Poeme symphonique scaled to the keyboard. Every key winds up an independent metronome at a seeded tempo, ticking softer until it dies or you release it.
---

`metronome-swarm` winds up a metronome every time you touch a key.

A hundred metronomes wound up together and left to run down out of phase made Ligeti's *Poeme symphonique*; this swarm is scaled to 16 and wound by your fingers. Each note-on winds an independent metronome on the played key: its tempo is a seeded uniform draw between `bpm-lo` and `bpm-hi`, its first strike sounds immediately at your velocity, and every later strike lands `fade` times softer than the last, never below 1. A metronome runs down after `max` strikes, or the moment your note-off for that key arrives, whichever comes first: your release stops that key's metronomes. When a 17th metronome starts, it steals the slot of the oldest, which is already silent between strikes, so the steal makes no sound.

Note-ons and note-offs are consumed; only the swarm's strikes reach the output, and everything else passes.

The swarm is a [generator](/miditool/how-it-works/#generators): it runs on its own seeded clock, the same seed and the same playing winding up the same swarm, and cleans up on a scene switch.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `seed` | integer | required | any unsigned 64-bit value |
| `bpm-lo` | number | `40.0` | `20..=400` bpm, at most `bpm-hi` |
| `bpm-hi` | number | `208.0` | `20..=400` bpm |
| `max` | integer | `24` | `1..=64` strikes per metronome |
| `fade` | number | `0.97` | `0.5..=1.0` velocity factor per strike |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

metronome-swarm seed=17 bpm-lo=40 bpm-hi=208 max=24 fade=0.96
feldman-field seed=5 floor=6 ceiling=26 jitter=3
```

Tap a handful of keys and walk away: each one ticks at its own tempo, the whole room slowly running down, with [`feldman-field`](/miditool/effects/feldman-field/) sinking everything toward a whisper. This is the `poeme` scene of `examples/machines.kdl`, where `switch="kill"` silences the swarm the moment you leave.

## Try this

Narrow the tempo band and let the strikes live longer:

```kdl
metronome-swarm seed=8 bpm-lo=112 bpm-hi=126 max=64 fade=0.99
```

Metronomes this close in tempo start almost together and drift apart audibly, the phasing patterns of process music emerging on their own. Hold the keys down only as long as you want each layer to survive; release is the off switch.
