---
title: Seeds
description: Every random effect is seeded and reproducible. The same seed is the same behavior forever, which makes even a scrambled keyboard learnable.
---

Three effects are random: [`shuffle-lock`](/miditool/effects/shuffle-lock/), [`loose-keys`](/miditool/effects/loose-keys/), and [`restrike`](/miditool/effects/restrike/) each require a `seed`, and everything they ever do follows from it. There is no unseeded randomness anywhere in miditool.

## Same seed, same behavior, forever

```kdl
shuffle-lock seed=42
```

This is the same permutation of the keys today, tomorrow, on your laptop, and on the studio machine. Restarting miditool changes nothing. Recording a take and recreating it next week changes nothing. A seed is part of the piece, the way a key signature is.

The seed is any unsigned 64-bit integer. There is nothing special about any value; `seed=1` and `seed=982347` are equally random-looking, just different.

## Rerolling

Want a different scramble? Edit the number:

```kdl
shuffle-lock seed=43
```

With [live editing](/miditool/guides/live-editing/), rerolling is an instrument you play: save, hear the new mapping on the next note, keep rolling until something sings. When you find it, the number in the file *is* the sound; commit it, write it on the score, send it to a friend.

## Why this makes scrambled keyboards learnable

An unseeded scramble would be a novelty: every session a new instrument, nothing carries over. A seeded scramble is a *stable* instrument that happens to be strange. `shuffle-lock seed=42` rewards practice exactly the way a piano does; your hands learn where the notes went, and the knowledge keeps. That is the founding idea of miditool: randomness as a compositional decision you make once, not noise that happens to you per session.

Two independent random effects in one config just take two seeds:

```kdl
fork {
    chain {
        key-range lo=21 hi=59
        loose-keys seed=7 sigma=3.5
    }
    chain {
        key-range lo=60 hi=108
        loose-keys seed=11 lo=72 hi=96
    }
}
```

Reusing a seed across two effects is fine too; they draw independently, the numbers just start from the same place.
