---
title: sieve
description: Quantize the keyboard onto a Xenakis sieve, a scale written as arithmetic on the key numbers. Off-sieve notes snap or drop.
---

`sieve` forces every note onto a sieve: a set of keys defined by arithmetic.

Sieves are the residue-class lattices Xenakis built pitch scales from for *Jonchaies* and *Akrata*: instead of listing notes, you write conditions on the key numbers, and the keys that satisfy them are the scale. Members pass untouched; everything else snaps to the nearest member, up, down, or drops entirely. Because a sieve's period need not be an octave, the "scale" can shift shape as it climbs, something no key signature can say.

The mapping is deterministic, no seed. Note-offs map the same way as their note-ons, so held notes always release correctly; with `snap="up"`, `"down"`, or `"drop"`, a dropped note-on takes its note-off with it.

## Parameters

`sieve` takes the expression as one bare string argument.

| Parameter | Type | Default | Range |
| --- | --- | --- | --- |
| expression | string argument | required | non-empty, must match at least one key |
| `snap` | string | `"nearest"` | `"nearest"`, `"up"`, `"down"`, `"drop"` |

`"nearest"` breaks ties downward and always finds a member. `"up"` and `"down"` drop the note past the last member in their direction. `"drop"` passes members and silences the rest, a [blocked-keys](/miditool/effects/blocked-keys/) written as arithmetic.

## The expression grammar

An atom `M@R` names every key where `key % M == R`: `12@0` is every C, `12@7` every G, `2@0` every even key. The modulus `M` is `1..=127` and the residue `R` must be less than `M`.

Atoms combine with three operators, plus parentheses; whitespace is free:

- `!` complement (within keys 0..=127), binds tightest
- `&` intersection, binds next
- `|` union, binds loosest

So `!3@0 & 4@1 | 12@7` reads as `((!3@0) & 4@1) | 12@7`. An expression that matches no key at all is rejected.

Some sieves:

- `12@0 | 12@4 | 12@7`: the C major triad in every octave.
- `2@0`: every even key, the whole-tone scale on C.
- `!12@1 & !12@6`: everything except C sharps and F sharps.
- `8@0 | 8@3 | 11@5`: two interlocking periods, 8 and 11, that only realign every 88 keys, so the scale never quite repeats as it climbs. This is the Jonchaies-flavored sieve from `examples/serial.kdl`.

## Example

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

sieve "8@0|8@3|11@5" snap="up"
```

Run the same line up and down the keyboard and listen to the scale change shape under your fingers.

## Try this

Turn the keyboard into a triad, then thin it to a filter:

```kdl
sieve "12@0 | 12@4 | 12@7"
```

Everything you play snaps onto C major triad tones; smashed clusters come out as voicings. Then add `snap="drop"` and only the notes already on the sieve survive, holes and all.
