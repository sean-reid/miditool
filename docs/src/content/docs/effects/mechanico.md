---
title: mechanico
description: Ligeti's clockwork looms. Keys latch onto a relentless pulse grid and are re-struck in lockstep, dying after their count, with seeded jams that skip a pulse and lurch early.
---

`mechanico` latches your keys into a machine that re-strikes them in lockstep.

Obsessive equal pulses hammered until the mechanism wears out: that is the clockwork loom of Ligeti's mechanico writing, *Continuum*'s harder sibling. Each note-on latches the played key into the loom, up to 12 keys; from then on the whole loom is re-struck together every `pulse`, each strike sounding for half the pulse at the velocity you latched with. Each key dies after `repeats` strikes. Re-striking a latched key resets its count, velocity, and age. The first pulse lands on the note-on that wakes the empty loom; later keys join at the next pulse, always in lockstep. A 13th key evicts the oldest, silently. And the machine jams: with probability `jam` per pulse the loom stutters, skipping that pulse in silence and lurching the next one in 50% early, half a pulse instead of a whole one.

Note-ons are consumed into the loom, and note-offs are consumed and ignored: the loom owns every duration, and your release changes nothing. Everything else passes.

The loom is a [generator](/miditool/how-it-works/#generators): it runs on its own seeded clock, the same seed and the same playing grinding through the same jams, and cleans up on a scene switch.

## Parameters

`pulse=` takes a duration string like `"250ms"` or `"1.5s"`, or `beats=` against the tempo, never both; see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `pulse` | duration string | `"150ms"` | at least `50ms` |
| `beats` | number | none (instead of `pulse`) | finite, at least `50ms` once resolved |
| `repeats` | integer | `16` | `1..=64` strikes per key |
| `jam` | number | `0.1` | `0..=0.5` probability per pulse |
| `seed` | integer | `0` | any unsigned 64-bit value |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

mechanico pulse="150ms" repeats=16 jam=0.1 seed=7
```

Stab a chord and let go: the machine hammers it 16 times at 400 strikes a minute, jamming about every tenth pulse, then falls silent. Add keys while it runs and they join the grid mid-stride.

## Try this

Lock the pulse to the tempo and turn the jams up:

```kdl
tempo 140

mechanico beats=0.25 repeats=32 jam=0.3 seed=2
```

Sixteenth notes at 140, but nearly a third of the pulses seize up, so the grid keeps skipping and lurching without ever losing its underlying clock. Set `jam=0.0` for the pure loom, or follow it with [`accent-groups`](/miditool/effects/accent-groups/) to press an additive meter onto the hammering.
