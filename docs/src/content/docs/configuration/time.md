---
title: Time and tempo
description: Writing times as durations or beats, and how the tempo node converts beats to real time.
---

Any effect parameter that measures time (`time=`, `interval=`, `first=`, `duration=`, `mean=`, `window=`, `decay=`, and whatever future effects add) accepts the same two spellings, and exactly one of them per node: a duration string, or `beats=` against the tempo. Each effect's page names its time properties.

## Durations

A number with an `ms` or `s` suffix, as a string:

```kdl
delay time="250ms"
echo repeats=3 time="1.5s" decay=0.6
```

Decimals are fine (`"0.5s"`, `"62.5ms"`); signs and exponents are not. The value must be positive.

## Beats

A bare number of beats, resolved against the config's tempo:

```kdl
tempo 96

echo repeats=4 beats=0.5 decay=0.7      // eighth notes at 96 bpm
```

One beat lasts `60 / bpm` seconds, so at `tempo 96` a beat is 625ms and `beats=0.5` is 312.5ms. Any positive value works: `beats=0.25` for sixteenths, `beats=1.5` for a dotted quarter, `beats=3` for a slow pulse.

## The `tempo` node

```kdl
tempo 120
```

Beats per minute, `20..=400`, default 120. It exists only to resolve `beats=` values; if every time in the config is a duration, the tempo changes nothing.

Writing times in beats keeps a whole config in one musical grid: change the `tempo` line and every `beats=` effect shifts together, which is especially satisfying [while editing live](/miditool/guides/live-editing/).

Mixing the two spellings in one node is an error:

```kdl fail
delay time="250ms" beats=0.5
```

Times are resolved when the graph is built, not per event, so a tempo edit applies the same way any other [live edit](/miditool/guides/live-editing/) does.
