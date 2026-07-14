---
title: talea
description: Lock notes to a repeating cycle of durations, the medieval talea. You choose the onsets; the cycle decides every length, and your release is ignored.
---

`talea` locks your notes to a repeating cycle of durations.

The talea is the rhythmic engine of the isorhythmic motet: Vitry and Machaut cycled a fixed row of durations beneath a repeating row of pitches, the two drifting out of phase. Messiaen revived the device in the *Liturgie de cristal* that opens the Quatuor pour la fin du temps, where duration cycles run under the music as if they had been running forever. Here the cycle is yours to write: each note-on takes the next duration from the list, wrapping around, and its note-off is scheduled then and there.

Your release is ignored: the cycle has already fixed when the note ends. Every note-on leaves with its note-off already scheduled, so note-ons and note-offs always balance and nothing can hang; retriggering a held key stacks another note with its own scheduled end.

## Parameters

The durations are bare arguments, one to 32 of them. Plain numbers are milliseconds; with `beats=true` they are beat counts against the tempo (each still resolving to within 1ms and 60s); see [Time and tempo](/miditool/configuration/time/).

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| durations | number arguments | required | 1 to 32 entries, each `1ms..=60s` |
| `beats` | boolean | `false` | `true` reads the entries as beats |

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"
tempo 90

talea 1 0.75 0.75 1 beats=true      // a 3.5-beat cycle
```

Three and a half beats per cycle: against a 4/4 count the row lands half a beat earlier on every pass, crossing the barline on its own schedule, the isorhythmic drift built in. Without `beats=true` the entries are milliseconds: `talea 250 500 250 1000`.

## Try this

The *Liturgie de cristal* move: give each hand its own talea and let them drift out of phase.

```kdl
tempo 90

fork {
    chain {
        key-range lo=0 hi=59
        talea 2 1 1 3 beats=true
    }
    chain {
        key-range lo=60 hi=127
        talea 1 1.5 0.5 beats=true
    }
}
```

A four-duration cycle below, a three-duration cycle above: even playing plain repeated chords, the two layers phrase themselves differently on every pass.
