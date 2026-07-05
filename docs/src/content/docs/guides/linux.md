---
title: Linux
description: miditool on Linux uses ALSA and works out of the box, including virtual output ports. Raspberry Pi is a supported target.
---

miditool speaks ALSA and works out of the box on Linux: real inputs, virtual output ports, everything the config language offers. Point your DAW (or a softsynth like FluidSynth) at the `miditool Out` port in its ALSA MIDI inputs.

## Checks

If `miditool ports` shows nothing or errors, make sure the ALSA sequencer is loaded:

```sh
miditool doctor
```

```text
ok    midi backend: 1 input (USB MIDI keyboard MIDI 1), 2 outputs (...)
ok    config miditool.kdl: parses, 2 top-level effects
ok    /dev/snd/seq exists
```

A missing `/dev/snd/seq` means the sequencer module is not loaded; `modprobe snd-seq` fixes it (most distributions load it automatically).

The one macOS-only feature is `hide=true` on the `input` node; on Linux it is ignored with a note. You rarely need it: Linux DAWs select MIDI inputs explicitly.

## Raspberry Pi

A Pi runs miditool comfortably; ALSA is the same there. A Pi *between* the keyboard and the computer is a supported pattern: the Pi does the transformation and the computer only ever sees one MIDI device, which sidesteps every DAW input question at once. A dedicated guide to that setup is coming.
