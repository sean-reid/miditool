---
title: spectral-halo
description: Grisey's instrumental synthesis. Every played note is surrounded by the upper partials of its harmonic series, each partial exactly tuned by a per-note pitch bend.
---

`spectral-halo` surrounds every note you play with the upper partials of its harmonic series, each one exactly in tune.

This effect needs an MPE-style setup on the receiving instrument; see [Microtonality](/miditool/guides/microtonality/) before wiring it in.

This is the instrumental synthesis of Grisey's *Partiels*, where an orchestra plays the analyzed spectrum of a low trombone E: one note fanned out into the overtones that make up its timbre. Here the played note passes through dry as the fundamental, and partials 2 through `partials` are added above it, each on its own member channel. Partial `k` sits `12 * stretch * log2(k)` semitones above the fundamental; the nearest key takes the note-on and the remainder rides as a pitch bend in cents, so the partials land on the harmonic series rather than on the equal-tempered keys nearest to it. At `stretch=1.0` the offsets are:

| Partial | Key offset | Residual detune |
| --- | --- | --- |
| 2 | +12 | 0.00 cents |
| 3 | +19 | +1.96 cents |
| 4 | +24 | 0.00 cents |
| 5 | +28 | -13.69 cents |
| 6 | +31 | +1.96 cents |
| 7 | +34 | -31.17 cents |
| 8 | +36 | 0.00 cents |

`stretch` is an inharmonicity control: `1.0` is the natural series, values above 1 stretch the partials apart the way stiff piano strings do, and values below 1 compress them toward the fundamental. Velocity rolls off geometrically, partial `k` playing at `round(vel * rolloff^(k-1))`, floored at 1, and a partial whose key would leave the keyboard is skipped. The mapping is deterministic, no seed.

Your note-off releases the fundamental and every partial it spawned, and a retrigger or scene switch cuts them first and resets the bends; if very dense playing forces the voice pool to steal, the stolen note's later note-off is silently ignored rather than cutting the newcomer.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `partials` | integer | `4` | `2..=8` |
| `rolloff` | number | `0.7` | `0..=1` |
| `stretch` | number | `1.0` | `0.5..=2.0` |
| `channels` | channel span | `"2-16"` | `"1"`..`"16"`, low end first |
| `bend-range` | semitones | `48` | `1..=96` |

`channels` and `bend-range` are the MPE tail shared by all four microtonal effects; the [Microtonality guide](/miditool/guides/microtonality/) explains how to set them.

## Example

The spectral scene from `examples/microtonal.kdl`, every note blooming into its first five overtones under a Feldman hush:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Microtonal"

scene "spectral" {
    feldman-field seed=5 floor=8 ceiling=26 jitter=3
    spectral-halo partials=5 rolloff=0.6 stretch=1.0 channels="2-16" bend-range=48
}
```

Play a low note and hold it: the octave, the pure twelfth, the just major seventeenth all stack above it, each a little quieter than the last, and single notes start sounding like chords of nature.

## Try this

Detune the series itself:

```kdl
spectral-halo partials=6 rolloff=0.5 stretch=1.08
```

A slight over-stretch pushes every partial sharp of the true series, the sound of an upright piano's inharmonic strings. Sweep `stretch` down toward `0.85` while playing and the halo squeezes into a dense, dark cluster around the octave.
