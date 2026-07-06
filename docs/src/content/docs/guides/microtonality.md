---
title: Microtonality
description: How the microtonal effects speak MPE-style MIDI, what the receiving instrument must do, and how to arrange chains, channels, and DAWs around them.
---

Four effects retune notes between the keys: [spectral-halo](/miditool/effects/spectral-halo/), [just](/miditool/effects/just/), [scordatura](/miditool/effects/scordatura/), and [overtone-pedal](/miditool/effects/overtone-pedal/). They all share one output mechanism and one set of requirements, described here. None of them is seeded; the same input always produces the same tuning.

## How the output works

MIDI has no per-note pitch. A pitch bend message bends a whole channel, every note on it at once, so a single channel cannot hold a C tuned 14 cents flat next to a G tuned 2 cents sharp. MPE (MIDI Polyphonic Expression) works around this by spending channels on notes: each sounding note gets a member channel to itself, and the channel-wide bend becomes that one note's microtonal offset.

That is exactly what these effects do. Each microtonal note is sent as its own member channel, a pitch bend on that channel, then the note-on; the note-off later releases the same channel, and a scene switch or shutdown releases every voice and resets every bend it touched back to zero. The bend value on the wire is `round(cents / 100 / bend-range * 8192)`, so at the default `bend-range=48` one bend step is about 0.6 cents, far finer than anything in these tunings.

## What you need

A checklist. All three must hold or the output will not sound as intended:

- **An instrument that receives per-channel pitch bend.** The receiver must apply a bend on channel 5 to the notes on channel 5 only. MPE-capable synths do this natively; a multitimbral setup with one instance per channel does too.
- **Its pitch bend range set to match `bend-range`** (48 semitones by default, the MPE convention for member channels). If the instrument thinks a full bend is 2 semitones while miditool assumes 48, every detune comes out 24 times too small. Change either side, but make them agree.
- **The member channels (`channels="2-16"` by default) free of other material.** Nothing else should send notes or bends on those channels. A dry note sharing a member channel can be detuned by a bend meant for a pool voice, or cut by a stolen voice's note-off. With the default span, keep your own traffic on channel 1.

## Composing with them

Two rules of thumb:

- **Microtonal effects go last in a chain.** They emit notes across the member channels with bends attached; an effect placed after them would see that fanned-out, per-channel traffic instead of your playing, and would likely scramble the pairing of bend and note. Shape the notes first, retune last:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

tintinnabuli root="d" minor=true
just root="d" channels="2-16" bend-range=48
```

- **Give parallel microtonal effects disjoint channel ranges.** Two effects sharing members will fight over them, each bending and cutting the other's voices. Split the span instead:

```kdl title="miditool.kdl"
input "Roland"
output virtual="miditool Out"

fork {
    chain {
        key-range lo=0 hi=59
        overtone-pedal fundamental=24 partials=16 channels="2-8"
    }
    chain {
        key-range lo=60 hi=127
        just root="c" channels="9-16"
    }
}
```

Left hand on the overtone series of low C, right hand in just intonation, and neither touches the other's channels.

## Voice stealing

The default span `channels="2-16"` is a pool of 15 voices, one per member channel. The 16th simultaneously sounding tuned note has nowhere to go, so the pool releases the oldest voice and gives its channel to the newcomer.

Musically: fifteen tuned notes at once is a lot, and ordinary playing, even fat chords under [spectral-halo](/miditool/effects/spectral-halo/), rarely gets there. Where you will hear it is long pedal washes and deliberately dense textures: the oldest sounding note ends early, quietly replaced by the newest. If that matters for a piece, thin the texture, lower `partials`, or accept it as the same compromise every 15-voice hardware synth makes. Narrowing `channels` shrinks the pool further, so split spans trade polyphony for independence.

## DAW notes

Honest expectations, instrument by instrument:

- **MPE-capable instruments** (many modern soft synths) are the easy path: switch the instrument to MPE mode, set its bend range to 48, done.
- **Multitimbral setups** work just as well and predate MPE: one instrument per MIDI channel, the same patch on every member channel, each instance's bend range set to 48. More clicking, same result.
- **Plain single-channel instruments do not work.** A track that merges all incoming channels into one instrument will ignore the per-channel bends or apply the last one to everything; either way you get equal temperament, all the notes at their nominal keys with none of the detune. The notes still play, so it fails quietly. If everything sounds suspiciously in tune, this is why.

Channel-handling details for specific hosts are in the [GarageBand](/miditool/guides/garageband/) and [other DAWs](/miditool/guides/daws/) guides.
