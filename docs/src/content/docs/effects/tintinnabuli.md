---
title: tintinnabuli
description: Arvo Pärt's tintinnabuli voice. Every melody note carries a companion tone drawn from a fixed tonic triad, above it, below it, or alternating.
---

`tintinnabuli` pairs every note you play with a bell tone from a fixed triad.

This is the technique Arvo Pärt has built on since *Für Alina*: a melody voice (the M-voice) shadowed by a tintinnabuli voice (the T-voice) that only ever sounds tones of the tonic triad, a small bell struck alongside each melody note. Whatever you play is the M-voice. Each note-on also sounds a T-voice: the nearest (`position=1`) or second-nearest (`position=2`) triad tone strictly above the played key (`"superior"`), strictly below it (`"inferior"`), or `"alternating"` sides per note, starting above. Strictly means a played triad tone still gets a neighbor, never a doubling. The T-voice sounds at `level` of the played velocity, floored at 1; when the walk runs off the keyboard, only your note sounds.

The mapping is deterministic, no seed. The emitted pair is remembered per note, so note-offs, retriggers, and scene switches release exactly the two keys that are sounding.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `root` | note name | required | `"c"`, `"f#"`, `"bb"`, or `"0"`..`"11"` |
| `minor` | boolean | `true` | `true`, `false` |
| `position` | integer | `1` | `1..=2` |
| `direction` | string | `"superior"` | `"superior"`, `"inferior"`, `"alternating"` |
| `level` | number | `0.8` | `0..=1` |

`root` takes a note name, a letter `a`..`g` with an optional `#` or `b` (`"c"`, `"f#"`, `"bb"`, case does not matter), or a pitch class written `"0"`..`"11"`. Together with `minor` it fixes the triad the T-voice draws from: root, third, and fifth.

## Example

The Pärt scene from `examples/harmony.kdl`, an A minor bell under a Feldman hush:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

tintinnabuli root="a" minor=true position=1 direction="superior" level=0.7
feldman-field seed=3 floor=6 ceiling=22 jitter=3
```

Play a slow line and every note arrives with the nearest A minor triad tone above it, at a fraction of the touch.

## Try this

Open the interval and let the bell swing over and under the melody:

```kdl
tintinnabuli root="c" minor=false position=2 direction="alternating"
```

Position 2 skips past the nearest triad tone to the second, so the companion sits a fourth to a sixth away, and alternating flips it above and below on successive notes. Drop `level` toward `0.3` and the second voice recedes into resonance.
