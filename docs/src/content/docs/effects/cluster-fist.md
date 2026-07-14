---
title: cluster-fist
description: Every note lands as a tone cluster, a fistful of neighboring keys drawn chromatic, white, black, or from a sieve. Cowell's forearm from one finger.
---

`cluster-fist` turns each note into a tone cluster: one finger plays a fistful.

Henry Cowell got these sounds with fists and forearms in *The Tides of Manaunaun*, notating bands of adjacent keys as single gestures; here the band comes from one key. Each note-on becomes `width` member keys drawn from the kind's set: every key (`"chromatic"`), the piano's white keys, its black keys, or the members of a [sieve](/miditool/effects/sieve/) (`kind="sieve" sieve="..."`). The `anchor` places your key at the bottom, center, or top of the fist, and the cluster fades away from it: the played key keeps your velocity, and the other members fade by rank in order of distance from it, the nearest scaled by `rolloff`, the next by `rolloff` squared, and so on outward (ties break toward the lower key). The played key is always included even when the set does not contain it (a black-key cluster anchored on a white key keeps its white anchor), and a side that runs off the keyboard, or past a sparse sieve's last member, truncates, shrinking the cluster.

The mapping is deterministic, no seed. The member keys are remembered per held note, so the note-off releases exactly the keys that sounded; nothing hangs.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `width` | integer | `4` | `2..=12` keys |
| `kind` | string | `"chromatic"` | `"chromatic"`, `"white"`, `"black"`, `"sieve"` |
| `anchor` | string | `"center"` | `"bottom"`, `"center"`, `"top"` |
| `rolloff` | number | `0.8` | `0..=1` |
| `sieve` | string | required with `kind="sieve"` | non-empty sieve expression; only allowed with `kind="sieve"` |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

cluster-fist width=6 kind="white" anchor="bottom" rolloff=0.7
```

Every note becomes six white keys rising from it: your melody rides the bottom edge of a diatonic fist. This is how the `cowell` scene of `examples/clouds.kdl` starts.

## Try this

Clusters that are chords. Drawn from a triad sieve and anchored at the top, the fist arrives as a spread voicing under each melody note:

```kdl
cluster-fist width=5 kind="sieve" sieve="12@0|12@4|12@7" anchor="top" rolloff=0.6
```

Then go the other way entirely: `kind="black" width=12 anchor="center" rolloff=0.9` is a forearm's worth of black keys, pentatonic slabs from single touches.
