---
title: resonance-halo
description: Ghost sympathetic resonance under the sustain pedal. While the pedal is down, each note deposits a quiet halo of neighbors that decays on its own.
---

`resonance-halo` makes the sustain pedal mean something before the piano: while it is down, every note excites a quiet halo of its neighbors.

The model is the held-pedal haze of Ligeti's piano writing, where the pedal turns single strikes into a standing wash of adjacent resonance. The effect tracks the pedal itself (CC64 per channel, a value of 64 or higher is down; the pedal message passes through to your DAW as usual). It only deposits while the pedal is down: each note-on then passes unchanged and deposits the `width` nearest keys on each side (the nearest sieve members when `sieve=` is given; the played key is never its own neighbor), each sounding at `level` times the note's velocity and fading out after `decay`. Lifting the pedal does nothing retroactive: deposited halos decay on their own schedule. With the pedal up the effect is a pure pass.

The played note releases when you release it, and every halo note is deposited together with its own scheduled note-off, so note-ons and note-offs always balance and nothing hangs. The halo is deterministic, no seed.

## Parameters

`decay=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `width` | integer | `3` | `1..=6` neighbors per side |
| `level` | number | `0.25` | `0..=1` fraction of the note's velocity |
| `decay` | duration string | `"3s"` | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | none (instead of `decay`) | finite, greater than 0 |
| `sieve` | string | none | non-empty [sieve](/miditool/effects/sieve/) expression |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

resonance-halo width=2 level=0.2 decay="2s"
```

Play with the pedal up and nothing changes; press it and every note arrives inside a two-second shimmer of its four nearest keys.

## Try this

Tune the resonance. Confined to a pentatonic sieve, the halo rings like sympathetic strings tuned to a scale:

```kdl
resonance-halo width=3 level=0.3 decay="4s" sieve="12@0|12@2|12@4|12@7|12@9"
```

Whatever you play, its haze is C pentatonic. Then put it after [`cluster-fist`](/miditool/effects/cluster-fist/), as the `cowell` scene of `examples/clouds.kdl` does, and every fistful rings inside its own halo.
