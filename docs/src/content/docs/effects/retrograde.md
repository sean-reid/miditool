---
title: retrograde
description: Webern's palindrome as a pedal. Hold to record a phrase, release to hear it once more, exactly mirrored in time, at a speed factor of your choosing.
---

`retrograde` plays your phrase back once, backwards.

Music that reads exactly the same reversed, the Webern palindrome, becomes something you can perform. Hold the pedal (CC `pedal`, the sustain pedal by default; a value of 64 or higher counts as down, on any channel) and play up to 32 notes; that capacity is fixed, and a 33rd note passes through but is not recorded. Release, and the phrase comes back exactly once, mirrored in time: the last note you played sounds first, every inter-onset gap and every duration is reversed, and the whole run is scaled by `speed` (2.0 plays the mirror in half the recorded time, 0.5 stretches it to double). A note that occupied `[a, b]` in a capture of length `L` returns over `[(L - b) / speed, (L - a) / speed]` after the pedal lifts. When the run ends, the machine falls silent until the next capture.

The pedal CC is consumed, so nothing sustains while this effect is in the chain; point `pedal=` at another CC to keep your sustain, or give the mirror a scene of its own ([Performing](/miditool/guides/performing/)). Your own notes always pass through unchanged, pedal down or up, and playing during the run does not disturb it: the machine adds a voice and never consumes a note. Pedal down again at any time to silence the playback and begin a new capture; a note still held at pedal-up has its duration capped there, and since it ended the capture, its mirror opens the run.

There is nothing random here; the mirror is exact, so the effect takes no seed. Like the [generators](/miditool/how-it-works/#generators), it runs its playback on its own clock and cleans up when you leave the scene.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `pedal` | integer | `64` (the sustain pedal) | a CC number, `0..=127` |
| `speed` | number | `1.0` | `0.25..=4.0` times the recorded pace |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

retrograde pedal=64 speed=1.0
```

Pedal down, play a gesture, pedal up: the gesture walks back out of the instrument, end first, at the pace you played it. Keep alternating and every phrase you offer is answered by its own reflection.

## Try this

Give the mirror a scene of its own and slow it down:

```kdl
scene "mirror" switch="kill" {
    retrograde pedal=64 speed=0.5
    velocity-curve gamma=1.1 ceiling=100
}
```

At half speed the palindrome becomes an echo with the causality reversed, and `switch="kill"` cuts the playback the moment you leave the scene.
