---
title: delay
description: Hold everything back by a fixed time. One repeat-free line, mostly useful inside forks against the dry signal.
---

`delay` holds every event back by a fixed time.

On its own it just makes the whole instrument late, which is rarely the point. Its musical life is inside a [`fork`](/miditool/configuration/routing/#fork), against the dry signal: one branch now, one branch later, and you are playing a canon with yourself at whatever interval and distance you choose. Unlike [`echo`](/miditool/effects/echo/) it repeats nothing; each event happens exactly once, just displaced.

## Parameters

Exactly one of `time=` or `beats=` must be given; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `time` | duration string | required (or `beats`) | positive, `"250ms"` or `"1.5s"` form |
| `beats` | number | required (or `time`) | finite, greater than 0 |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

fork {
    pass
    chain {
        delay time="500ms"
        transpose 12
    }
}
```

Everything you play answers itself an octave up, half a second later.

## Try this

A strict two-voice canon at the fifth, one measure behind, locked to the tempo:

```kdl
tempo 90

fork {
    pass
    chain {
        delay beats=4
        transpose 7
        channelize 2
    }
}
```

Change `tempo` and the canon distance follows.
