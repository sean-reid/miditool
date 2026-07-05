---
title: Other DAWs
description: Pointing Logic Pro, Reaper, Ableton Live, Cubase, MainStage, or any other DAW at the miditool Out port.
---

Most DAWs let you choose which MIDI input a track records. Point that choice at `miditool Out` and you are done; no hiding, no special setup. This page collects the per-DAW details.

For all of them, start `miditool run` first so the `miditool Out` port exists when the DAW scans for devices.

## Logic Pro

Logic records all inputs by default, but each track can be pinned to one port: in the **Track inspector**, set **MIDI In Port** to `miditool Out`. That track then records only miditool's output, and your raw keyboard can stay visible for other tracks.

## Reaper

Open **Preferences > MIDI Devices**. Reaper disables new MIDI inputs by default, so first enable `miditool Out` (right-click it, Enable input). Then on the track, click the record-arm input selector and choose `miditool Out` under Input: MIDI.

## Ableton Live

Two levels. In **Preferences > Link/MIDI**, make sure the **Track** toggle is on for `miditool Out` in the MIDI Ports list. Then on the track itself, set **MIDI From** to `miditool Out`. To keep the raw keyboard out entirely, turn its Track toggle off in the same preferences pane.

## Cubase

Set the track's MIDI input in the track inspector to `miditool Out` instead of All MIDI Inputs. If you use All MIDI Inputs on other tracks, exclude the raw keyboard: in **Studio Setup > MIDI Port Setup**, uncheck the keyboard's **in 'All MIDI Inputs'** column so only miditool's stream reaches those tracks.

## MainStage

Each channel strip has its own input assignment: in the channel strip inspector, set the **MIDI Input** to `miditool Out`.

## Any other DAW

The checklist:

1. Can the DAW select MIDI inputs, globally or per track? Pick `miditool Out` and, where possible, disable the raw keyboard as an input.
2. If it cannot (it listens to everything, like GarageBand), use the macOS hide feature: `input "..." hide=true` in the config. See [Using miditool with GarageBand](/miditool/guides/garageband/).

## Tip: one port, many instruments

The [`channelize`](/miditool/effects/channelize/) effect routes different scenes or keyboard splits to different MIDI channels on the same `miditool Out` port. A DAW with multitimbral routing can then put each channel on its own instrument:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

fork {
    chain {
        key-range lo=21 hi=59
        channelize 2          // left hand -> channel 2 -> bass track
    }
    chain {
        key-range lo=60 hi=108
        channelize 3          // right hand -> channel 3 -> keys track
    }
}
```
