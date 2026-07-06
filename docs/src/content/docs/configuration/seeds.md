---
title: Seeds
description: Every random effect is seeded and reproducible. The same seed is the same behavior forever, which makes even a scrambled keyboard learnable.
---

Every random effect takes a `seed`, and everything it ever does follows from it. That is most of the stochastic palette, from [`shuffle-lock`](/miditool/effects/shuffle-lock/), [`loose-keys`](/miditool/effects/loose-keys/), [`restrike`](/miditool/effects/restrike/), and [`registral-scatter`](/miditool/effects/registral-scatter/) through [`poisson-cloud`](/miditool/effects/poisson-cloud/), [`note-roulette`](/miditool/effects/note-roulette/), [`velocity-dice`](/miditool/effects/velocity-dice/), [`duration-lottery`](/miditool/effects/duration-lottery/), and [`density-governor`](/miditool/effects/density-governor/) to [`added-value`](/miditool/effects/added-value/), [`feldman-field`](/miditool/effects/feldman-field/), and [`anti-accent`](/miditool/effects/anti-accent/), plus the five generators ([`continuum`](/miditool/effects/continuum/), [`metronome-swarm`](/miditool/effects/metronome-swarm/), [`brownian-walker`](/miditool/effects/brownian-walker/), [`mechanico`](/miditool/effects/mechanico/), and [`continuator`](/miditool/effects/continuator/)), the effects that are only sometimes random ([`wedge-mirror`](/miditool/effects/wedge-mirror/) below 1.0 probability, [`klangfarben`](/miditool/effects/klangfarben/) in random mode, [`aggregate-gate`](/miditool/effects/aggregate-gate/) with leak) and the `rng()` helpers in [scripts](/miditool/configuration/scripting/). There is no unseeded randomness anywhere in miditool.

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
