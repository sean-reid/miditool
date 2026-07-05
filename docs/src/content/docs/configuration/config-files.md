---
title: Config files
description: The shape of a miditool config, input and output selection, tempo, the remote, scenes, and a two-paragraph KDL primer.
---

A miditool config is one [KDL](https://kdl.dev) file. The smallest useful one is a single line:

```kdl title="miditool.kdl"
shuffle-lock seed=42
```

Everything else, input and output selection, tempo, the remote, scenes, is optional and has a sensible default.

## Where miditool looks

`miditool run` resolves its config in order, first hit wins:

1. A path on the command line: `miditool run examples/scrambled.kdl`
2. The `MIDITOOL_CONFIG` environment variable
3. `./miditool.kdl` in the working directory
4. `~/.miditool/config.kdl`, the miditool home (set `MIDITOOL_HOME` to move it)

When nothing is found, the first run creates `~/.miditool/config.kdl` for you: a commented starter that passes MIDI through untouched until you enable something in it. The startup line names the config that won, and `miditool doctor` reports the same resolution.

A working-directory `miditool.kdl` beats the home config on purpose: keep a folder per piece or per experiment, and the home config remains your everyday setup. Script paths always resolve relative to whichever config file is running, so scripts next to the home config travel with it.

## A KDL primer

KDL is a document language built from *nodes*. A node is a name followed by values on the same line: bare *arguments* (`transpose 12`, `only-channels 1 2`) and named *properties* (`shuffle-lock seed=42 mode="free"`). Strings are quoted, numbers are bare, and `true`/`false` are the booleans. A node can carry a block of child nodes in braces, which is how `chain`, `fork`, and `scene` hold their contents.

Comments run from `//` to the end of the line, and `/-` in front of a node comments out the whole node, children included, which is handy for muting one effect while experimenting. That is all the KDL you need for miditool.

## The file shape

```kdl title="miditool.kdl"
input "Roland" hide=true         // optional: which keyboard, and whether to hide it
output virtual="miditool Out"    // optional: where the DAW listens
tempo 96                         // optional: bpm for beats= times, default 120
remote port=8320                 // optional: the web remote, off by default

shuffle-lock seed=42             // the effects: an implicit top-level chain
velocity-curve gamma=0.8
```

### `input`

A substring of the input port's name; `miditool ports` shows the candidates. Omit it and miditool picks a default. `hide=true` hides the raw source from every other app while miditool runs, which is the [GarageBand fix](/miditool/guides/garageband/); it is macOS only and ignored elsewhere.

### `output`

Exactly one of two properties:

- `output virtual="miditool Out"` creates a virtual port with that exact name. This is the default when the node is omitted.
- `output device="IAC"` connects to an existing port whose name contains the substring, which is also [how Windows works](/miditool/guides/windows/), via loopMIDI.

Giving both, or neither, is an error:

```kdl fail
output virtual="miditool Out" device="IAC"
```

### `tempo`

Beats per minute, `20..=400`, default 120. It only matters for effects that use `beats=` times: see [Time and tempo](/miditool/configuration/time/).

### `remote`

`remote port=8320` serves the [web remote](/miditool/guides/remote/) on that port, `1..=65535`. No `remote` node, no server. By default it binds `127.0.0.1` and answers this machine only; add `bind="0.0.0.0"` to open it to your local network for a phone or tablet:

```kdl
remote port=8320 bind="0.0.0.0"
```

`bind=` takes any IP address of the machine; anything that does not parse as one is rejected.

## Scenes

Instead of bare effects, a config can hold named `scene` blocks, each a chain of its own. The [remote](/miditool/guides/remote/) switches between them mid-performance:

```kdl title="miditool.kdl"
input "Roland"
remote port=8320 bind="0.0.0.0"

scene "scrambled" {
    shuffle-lock seed=42
}
scene "echo storm" switch="kill" {
    echo repeats=6 time="300ms" decay=0.8
}
```

`switch=` says what happens to sounding notes when you leave the scene: `"let-ring"` (the default) lets them ring out; `"kill"` cuts them. Scene names must be unique and non-empty, compared exactly as written, and every scene needs at least one effect.

Anywhere an effect can go, `script "wedge.lua"` runs your own Luau code on every event, with the path resolved against the config file's directory; see [Scripting](/miditool/configuration/scripting/).

Bare effects are shorthand for a single scene named `main`. The two styles do not mix; this is an error:

```kdl fail
scene "scrambled" {
    shuffle-lock seed=42
}
velocity-curve gamma=0.8     // loose effect outside any scene
```

## Errors

Bad values are rejected with the node name and the constraint, like `tempo: beats per minute must be within 20..=400, got 500`. Every range is listed on the page for its node or [effect](/miditool/effects/shuffle-lock/). A running miditool [keeps its current graph](/miditool/guides/live-editing/) when a config edit fails, so errors cost nothing but the time to read them.
