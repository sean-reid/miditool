---
title: Live editing
description: Edit the config while you play. Saves apply on the next note, held notes finish cleanly, and a broken edit never interrupts sound.
---

`miditool run` watches the config file it loaded. The edit loop is: play with one hand, tweak the file in your editor, save, hear the change. No restart, no silence.

## What happens on save

- The file is re-parsed and a new effect graph is built.
- The new graph takes over on the next event. Notes already sounding finish under the mapping that opened them, so nothing sticks and nothing cuts off mid-ring.
- Effects and their parameters, scenes, and `tempo` all reload this way.

Two settings need a restart because they change the ports themselves: `input` and `output`. Edit those, then stop and rerun.

## Broken edits are safe

If a save does not parse or fails validation, miditool prints the error, with the offending line, and keeps the previous graph running. Sound never stops because of a typo. Fix the file and save again.

```text
config error in miditool.kdl:
echo: decay must be greater than 0 and at most 1, got 1.4
(previous config still running)
```

## A practice loop

Seeded effects make live editing a compositional tool. Try this: run a scrambled keyboard,

```kdl title="miditool.kdl"
input "Roland"

shuffle-lock seed=42
```

and while playing, change `seed=42` to `seed=43`, save, and the instrument becomes a different (equally stable) scramble on the next note. Roll through seeds until one sings, then keep it; [the same seed is the same instrument forever](/miditool/configuration/seeds/).
