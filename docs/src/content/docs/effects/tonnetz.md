---
title: tonnetz
description: Walk the neo-Riemannian Tonnetz one triad per keypress. Each note applies the next P, L, or R transform and sounds the triad it lands on.
---

`tonnetz` turns the keyboard into a walk across the neo-Riemannian Tonnetz.

The Tonnetz is the lattice of triads that neo-Riemannian theory uses to read late Romantic harmony, Wagner and Liszt drifting between chords a single tone apart rather than moving by functional progressions. Triads step along it by three transforms:

- `p` (parallel) keeps the root and flips the quality: C major to C minor and back.
- `l` (leittonwechsel) exchanges a triad with the one a major third away: C major to E minor and back.
- `r` (relative) exchanges a triad with its relative: C major to A minor and back.

Every keypress steps the sequence: a note-on first applies the next letter of the cycling `sequence`, then sounds the triad it arrives at. The starting triad itself is never heard; your first note is already one step in. Each triad tone is voiced at the key inside `lo..=hi` nearest your played key (ties break downward, and a pitch class with no key in the range is skipped), all at your velocity; `include-played=true` sounds the key you actually pressed as well.

The walk is deterministic, no seed: the same keypresses always trace the same path. The emitted set is remembered per note, so note-offs, retriggers, and scene switches release exactly the triad that is sounding.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `start` | note name | required | `"c"`, `"f#"`, `"bb"`, or `"0"`..`"11"` |
| `minor` | boolean | `false` | `true`, `false` |
| `sequence` | string | `"rl"` | letters `p`, `l`, `r`, case-insensitive |
| `lo` | key number | `48` | `0..=127` |
| `hi` | key number | `79` | `0..=127`, at least `lo` |
| `include-played` | boolean | `false` | `true`, `false` |

`start` takes a note name, a letter `a`..`g` with an optional `#` or `b` (`"c"`, `"f#"`, `"bb"`, case does not matter), or a pitch class written `"0"`..`"11"`. With `minor` it names the triad the walk begins from. `sequence` is read left to right and cycles forever.

## Example

The drift scene from `examples/harmony.kdl`:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

tonnetz start="c" minor=false sequence="rl" lo=48 hi=79
velocity-curve gamma=1.2 ceiling=96
```

`"rl"` alternates relative and leading-tone exchange, so from C major each keypress slides one station through A minor, F major, D minor, B flat major, and on around the diatonic circle of thirds.

## Try this

The hexatonic drift:

```kdl
tonnetz start="c" sequence="pl"
```

Alternating parallel and leittonwechsel traces one of Cohn's hexatonic cycles: from C major the keypresses land on C minor, A flat major, A flat minor, E major, E minor, C major, then around again. Six notes close the loop, and the whole ride never leaves six pitch classes. Add `include-played=true` and your own key cuts through the cycle as a free voice against the drifting triads.
