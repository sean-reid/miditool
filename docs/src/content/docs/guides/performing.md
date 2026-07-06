---
title: Performing
description: Control miditool from the keyboard itself. Reserved keys step, jump, and panic between scenes, and a seeded moments clock can wander the scenes on its own.
---

A performance should not need a laptop within reach. The `control` block reserves keys on your keyboard as gestures: step through scenes, jump straight to one, silence everything, all without looking away from the instrument. It can also hand the switching to a clock and let the piece change rooms by itself.

## The control block

One optional `control` block sits at the top level, next to `input` and `tempo`. Here is a full rig, three scenes stepped through from the keys:

```kdl title="performance.kdl"
input "Roland"
output virtual="miditool Performance"

tempo 90

control {
    next-scene key=108               // C8 steps to the next room
    goto key=21 scene="halo"         // A0 jumps straight home
    panic key=20                     // G#0 flushes and silences everything
    // Uncomment to let the moments clock wander the rooms on its own:
    // moments dwell-lo="20s" dwell-hi="90s" seed=7
}

// Home: a quiet halo of neighbors around whatever is played.
scene "halo" {
    resonance-halo width=2 level=0.2 decay="2s"
    velocity-curve gamma=0.9 ceiling=110
}

// Hold the sustain pedal to teach the looper a phrase, release to
// set it circling; each pass warps one seeded detail.
scene "looper" {
    crippled-looper seed=17 pedal=64 max=12
    feldman-field seed=5 floor=8 ceiling=30 jitter=3
}

// The same pedal runs the palindrome machine here: hold to record,
// release to hear the phrase backwards at half the recorded pace.
scene "mirror" switch="kill" {
    retrograde pedal=64 speed=0.5
    velocity-curve gamma=1.1 ceiling=100
}
```

## How gesture keys work

A reserved key is consumed before the effect graph ever sees it, on every channel: the note-on fires the gesture, and its note-off (plus any poly-pressure in between) is eaten too. A gesture never sounds and never reaches an effect, which is why you pick keys you never play: the extremes of the keyboard. The rig above uses the top C for stepping and the bottom of the range for jumping and panic, keys the music does not need.

Keys are MIDI key numbers `0..=127`, and each key serves exactly one role; assigning a key twice is a config error that names both claimants. Everything you play on the other 125 keys passes through untouched.

## The gestures

- `next-scene key=` steps forward through the scenes in file order, wrapping from the last back to the first; `prev-scene key=` steps back the same way.
- `goto key= scene=` jumps straight to a named scene. The node is repeatable, one per destination, and the name must exist (the implicit `main` scene of a bare-effects config counts).
- `panic key=` flushes and silences everything, the same sweep as the remote's PANIC button.

A gesture that targets the scene already active is a no-op: nothing rebuilds, nothing flushes. On a real switch, what happens to sounding notes is the scene's `switch=` setting, `"kill"` or `"let-ring"`, exactly as with any other switch; see [Config files](/miditool/configuration/config-files/#scenes).

## The moments sequencer

The model is Stockhausen's moment form: a piece assembled from self-sufficient moments that cut from one to the next rather than develop. `moments` hands scene switching to a clock that does exactly that:

```kdl title="miditool.kdl"
input "Roland"

control {
    moments dwell-lo="20s" dwell-hi="90s" seed=7
}

scene "halo" {
    resonance-halo width=2 level=0.2 decay="2s"
}
scene "clouds" {
    poisson-cloud seed=3 density=6.0 duration="1.5s"
}
scene "loom" {
    mechanico pulse="150ms" repeats=16 jam=0.1 seed=7
}
```

The clock dwells in the active scene for a seeded random time drawn uniformly from `dwell-lo..=dwell-hi`, then cuts to a random scene that is never the one it is leaving, and draws the next dwell. Both dwells are plain duration strings, each at least 2 seconds, with `dwell-lo` no greater than `dwell-hi`. The whole wander is a pure function of the seed, so the same seed gives the same sequence of rooms and stays; see [Seeds](/miditool/configuration/seeds/).

You can still switch by hand while the clock runs. Any manual change, a gesture or a remote tap, rewinds the clock: the current dwell restarts from that moment without consuming a random draw, so however much you interfere, the automatic sequence picks up where it deterministically left off.

## With the web remote

The control block and [the web remote](/miditool/guides/remote/) compose; run both at once. Gestures, remote taps, and the moments clock all travel the same scene-switch path, so the remote's scene keys always show the active scene however the switch happened, every manual switch rewinds the moments clock, and the panic key and the remote's PANIC are the same sweep. The keyboard covers the moves you make mid-phrase; the phone covers the ones you make between pieces.

## A note on reloads

Gestures resolve to scene positions when the engine starts. In v1 a [live config reload](/miditool/guides/live-editing/) does not re-resolve them: a reload that reorders or renames scenes leaves `next-scene`, `prev-scene`, and `goto` pointing at the old positions until a restart. Editing parameters inside scenes reloads fine; if you must reorder scenes, restart.

## Pedal effects

Two effects are performed from the pedal rather than the keys: [`crippled-looper`](/miditool/effects/crippled-looper/) sets a pedal-captured phrase circling with one seeded warp per pass, and [`retrograde`](/miditool/effects/retrograde/) plays one back mirrored in time. Their pedal CC is consumed the way gesture keys are, so nothing sustains in a scene that contains them. Give them scenes where you do not need sustain, as the rig above does, or point `pedal=` at another CC and keep both.
