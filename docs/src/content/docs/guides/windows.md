---
title: Windows
description: miditool on Windows uses WinMM. Windows has no native virtual MIDI ports, so route the output through loopMIDI.
---

miditool runs on Windows over WinMM. The one platform gap is virtual ports: Windows has no native way for an application to create a MIDI port that other applications can open, so the default `output virtual="miditool Out"` cannot work there. The standard fix is [loopMIDI](https://www.tobias-erichsen.de/software/loopmidi.html), a small free utility that creates loopback MIDI ports.

## Setup

1. Install loopMIDI and create a port. The default name works; this page assumes a port named `loopMIDI Port`.
2. Point miditool's output at it with `device=` instead of `virtual=`:

```kdl title="miditool.kdl"
input "MPK"                     // substring of your keyboard's port name
output device="loopMIDI"        // substring of the loopMIDI port's name

shuffle-lock seed=42
```

3. In your DAW, select the loopMIDI port as the MIDI input.

`miditool doctor` checks for this setup and warns when no loopMIDI port exists:

```text
warn  no loopMIDI port; install loopMIDI and create one, then use `output device="loopMIDI"`
```

## Notes

- `hide=true` on the `input` node is a CoreMIDI feature; on Windows it is ignored with a note. Use a DAW with per-track MIDI input selection instead: see [Other DAWs](/miditool/guides/daws/).
- `miditool bench` needs virtual ports on both ends and cannot run on Windows.
- Everything else, including scenes, hot reload, and the [web remote](/miditool/guides/remote/), works the same as on other platforms.
