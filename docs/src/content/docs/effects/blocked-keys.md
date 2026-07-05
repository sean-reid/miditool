---
title: blocked-keys
description: Silence a fixed set of keys or pitch classes. The notes you cannot play shape the ones you can.
---

`blocked-keys` silences a fixed set of keys; everything else passes untouched.

This is the silent hand of Ligeti's *Touches bloquees*: keys held down so the playing hand's strikes on them produce nothing but the gap. Run a fast even line across a blocked patch of keyboard and the holes turn it into jagged, syncopated rhythm you never played. The blocked notes are not remapped, they are simply absent.

A blocked note-on is dropped and its matching note-off drops with it, so nothing is ever left hanging.

## Parameters

`blocked-keys` takes the keys as bare arguments, at least one.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| keys | integer arguments | required | `0..=127`; pitch classes `0..=11` with `by-class=true` |
| `by-class` | boolean | `false` | `true`, `false` |

With `by-class=true` the arguments are pitch classes, blocked in every octave: `blocked-keys 0 by-class=true` silences every C on the keyboard.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

blocked-keys 60 62 64 65 67      // a hole in the middle of the keyboard
```

## Try this

Block all five black-key pitch classes, in every octave:

```kdl
blocked-keys 1 3 6 8 10 by-class=true
```

Now hammer chromatic runs and glissandi; only the white keys survive, and the missing notes are the groove. Then flip it, `blocked-keys 0 2 4 5 7 9 11 by-class=true`, and you are playing the pentatonic ghost of the same gesture.
