---
title: velocity-router
description: Route notes to MIDI channels by how hard they are played. Dynamics become orchestration, and a crescendo walks across instruments.
---

`velocity-router` turns dynamics into orchestration: how hard you play chooses which instrument sounds.

Each note-on is dealt to a channel by its velocity: below `low` it goes to the `soft` channel, from `high` up to the `loud` one, and everything between to `medium`. Orchestrators have always scored dynamics this way, handing a swell from the strings to the brass rather than just asking anyone to play louder; here the handover happens under your fingers, and a crescendo stops being a louder piano and becomes a walk across the stage. Keys and velocities are never rewritten, only channels.

The multitimbral DAW setup is the point: this effect does nothing audible until the three channels point at three different instruments, so set up three tracks receiving channels of your choosing; see [the DAW guide](/miditool/guides/daws/#tip-one-port-many-instruments). Each note's note-off, retrigger cut, and polyphonic aftertouch follow it to whichever channel it was dealt, so nothing sticks; pedals and other non-note events pass on their original channel.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `low` | velocity | `64` | `1..=127`, less than `high` |
| `high` | velocity | `96` | `1..=127` |
| `soft` | channel | required | `1..=16` |
| `medium` | channel | required | `1..=16` |
| `loud` | channel | required | `1..=16` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

velocity-router low=64 high=96 soft=2 medium=3 loud=4
```

In the DAW, put a felt piano on channel 2, strings on channel 3, and brass on channel 4: playing quietly is now a different instrument from playing hard, and one phrase can pass through all three.

## Try this

Reserve the edges for extremes. With the thresholds pushed out, the medium channel owns almost everything and the outer instruments become punctuation:

```kdl
velocity-router low=30 high=110 soft=2 medium=3 loud=4
```

Only a real caress reaches channel 2 and only a real blow reaches channel 4. Then chain a [`velocity-curve`](/miditool/effects/velocity-curve/) *before* the router to move the effective borders without retuning your hands.
