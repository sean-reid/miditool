---
title: Routing and filters
description: Chains, forks and their merge semantics, and the filters that carve the keyboard into zones, channels, and velocity bands.
---

Effects form a graph. Two structural nodes build it, and a handful of filters decide which events reach which branch.

## `chain`

Effects in series. The top level of a config is already an implicit chain, so you only write `chain { }` explicitly inside a `fork` to group several effects into one branch:

```kdl
shuffle-lock seed=42       // these two lines
velocity-curve gamma=0.8   // are an implicit chain
```

Order matters: each effect feeds the next. An effect that emits nothing for an event swallows it for the rest of the chain.

## `fork`

Effects in parallel. Each branch gets a copy of the incoming event, and the branch outputs are merged in order. The merge drops exact duplicates, so the classic octave doubler

```kdl
fork {
    pass
    transpose 12
}
```

doubles notes without doubling everything else: a control change, passed unchanged by both branches, comes out once, while a note comes out at two pitches because the copies differ.

## Filters

Filters pass some events and drop the rest; they have no state and no parameters beyond their ranges. All ranges are inclusive.

| Filter | Passes |
| --- | --- |
| `only-channels 1 2 ...` | events on the listed channels (1-16, at least one) |
| `key-range lo=21 hi=108` | notes with keys in the range (keys 0-127, defaults 0 and 127); non-note events flow through |
| `velocity-range lo=1 hi=127` | note-ons with velocities in the range (1-127, defaults 1 and 127); everything else flows through |
| `notes-only` | note and poly-pressure events |
| `controllers-only` | controller events |
| `pass` | everything (the identity) |
| `discard` | nothing |

`pass` and `discard` earn their keep inside forks: `pass` keeps the dry signal as one branch, and `/-` plus `discard` let you audition branches while [editing live](/miditool/guides/live-editing/).

Note that `key-range` and `velocity-range` let non-matching *non-note* traffic through on purpose, so pedals and mod wheels keep working in every zone. To cut controllers from a branch, add `notes-only`.

## A split keyboard, worked

Lower half humanized into a bass, upper half doubled an octave up, each on its own channel for [multitimbral routing in the DAW](/miditool/guides/daws/#tip-one-port-many-instruments):

```kdl title="miditool.kdl"
input "Arturia"
output virtual="miditool Out"

only-channels 1                  // the keyboard's main channel only

fork {
    // Bass half: notes wander a few keys around what was played.
    chain {
        key-range lo=21 hi=59
        loose-keys seed=7 sigma=3.5
        channelize 2
    }

    // Treble half: doubled an octave up, velocities tamed.
    chain {
        key-range lo=60 hi=108
        fork {
            pass
            transpose 12
        }
        velocity-curve gamma=1.4 ceiling=100
        channelize 3
    }
}
```

The implicit top-level chain runs `only-channels` before the fork, so both branches see only channel 1. Inside, each branch filters to its half of the keyboard, transforms, and stamps its output onto its own channel. Forks nest: the treble branch contains its own little doubling fork.
