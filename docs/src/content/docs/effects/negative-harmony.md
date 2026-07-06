---
title: negative-harmony
description: Reflect every note around the key's tonic-dominant axis. Major turns minor and melodies answer themselves in the mirror.
---

`negative-harmony` reflects every note around the tonic-dominant axis of a key.

This is the mirror from Ernst Levy's *A Theory of Harmony*, lately a jazz reharmonization staple: pitch class `pc` maps to `(7 + 2 * tonic - pc) mod 12`, the reflection whose axis lies midway between the tonic and its dominant (in C, between E flat and E). Under this convention the tonic maps to the dominant and back, and major triads come out minor. The reflected pitch class is voiced at the key nearest the one you played, so lines stay in register while their contour inverts.

The full pitch-class mapping for `tonic="c"`:

| You play | C | Db | D | Eb | E | F | F# | G | Ab | A | Bb | B |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| You hear | G | F# | F | E | Eb | D | Db | C | B | Bb | A | Ab |

By default (`mode="replace"`) the mirror replaces each note. With `mode="add"` your note sounds too, and the mirror joins it at `level` of the played velocity, floored at 1; `level` only matters in add mode.

The reflection is deterministic, no seed, and stateless, so note-offs, retriggers, and even orphan note-offs always land on exactly the keys that are sounding.

## Parameters

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| `tonic` | note name | required | `"c"`, `"f#"`, `"bb"`, or `"0"`..`"11"` |
| `mode` | string | `"replace"` | `"replace"`, `"add"` |
| `level` | number | `0.8` | `0..=1` |

`tonic` takes a note name, a letter `a`..`g` with an optional `#` or `b` (`"c"`, `"f#"`, `"bb"`, case does not matter), or a pitch class written `"0"`..`"11"`.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

negative-harmony tonic="c"
```

Play a C major triad and hear a C minor sonority: the C drops to the G below, the E slips to E flat, the G climbs to the C above. Every ascending line you play comes back descending.

## Try this

Keep your line and give it a mirror partner:

```kdl
negative-harmony tonic="c" mode="add" level=0.5
```

Every note arrives with its reflection at half the touch, strict contrary motion without a second player. Then move `tonic` to match whatever key you are actually in; the axis follows the key, and the mirror starts answering functionally, dominant for tonic, subdominant for supertonic.
