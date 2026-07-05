---
title: channelize
description: Send everything to one MIDI channel. The routing glue for splits, scenes, and multitimbral DAW setups.
---

`channelize` rewrites every event onto one MIDI channel.

Musically it is glue: put it at the end of a [fork branch](/miditool/configuration/routing/#fork) and that branch becomes its own instrument in the DAW. A split keyboard sends bass to channel 2 and keys to channel 3; different scenes stamp different channels so one recorded take separates cleanly onto tracks. Channels are written 1 to 16, as on the keyboard's panel.

## Parameters

`channelize` takes one bare argument, not a named property.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| channel | integer argument | required | `1..=16` |

## Example

```kdl title="miditool.kdl"
input "Arturia"
output virtual="miditool Out"

fork {
    chain {
        key-range lo=21 hi=59
        channelize 2               // left hand to the bass track
    }
    chain {
        key-range lo=60 hi=108
        channelize 3               // right hand to the keys track
    }
}
```

In the DAW, set one instrument track to receive channel 2 and another channel 3; see [the DAW guide](/miditool/guides/daws/#tip-one-port-many-instruments).

## Try this

Give each scene its own channel, so switching scenes from the [remote](/miditool/guides/remote/) also switches which DAW instrument sounds:

```kdl
remote port=8320

scene "keys" {
    channelize 1
}
scene "scrambled organ" {
    shuffle-lock seed=42
    velocity-curve floor=90 ceiling=90
    channelize 2
}
```
