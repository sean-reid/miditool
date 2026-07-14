---
title: Scripting
description: Write your own effect in Luau. One function, a small event table, seeded randomness, and a fail-open safety net around every script.
---

The built-in effects cover a lot, but not everything you will want to do to a MIDI stream. The `script` node runs a [Luau](https://luau.org) file inline in the chain: your function sees every event, and whatever it returns is what flows on to the next effect. It is the escape hatch for the mappings miditool does not ship: mirrors, chord generators, conditional filters, anything you can state as "given this event, emit those".

## Quickstart

```sh
miditool new script wedge
```

writes `wedge.lua`, a runnable starter that mirrors the keyboard around middle C. Add it to a config next to the file:

```kdl title="miditool.kdl"
input "Roland"

scene "wedge" {
    velocity-curve gamma=0.8
    script "wedge.lua" seed=1
}
```

then `miditool run`. The path resolves against the config file's directory, so the pair travels together. `seed=` is optional and defaults to 0; it matters only if the script draws randomness.

## The event table

miditool calls the global `on_event(ev)` for every event that reaches the script. `ev` is a plain table:

| Field | Type | On which kinds | Meaning |
| --- | --- | --- | --- |
| `kind` | string | all | `"note-on"`, `"note-off"`, `"poly-pressure"`, `"cc"`, `"program"`, `"channel-pressure"`, `"bend"` |
| `ch` | integer | all | channel, `1..16`, as printed on the keyboard |
| `key` | integer | note-on, note-off, poly-pressure | MIDI key, `0..127`, middle C is 60 |
| `vel` | integer | note-on, note-off | velocity, `0..127` |
| `value` | integer | poly-pressure, cc, channel-pressure | the pressure or controller value, `0..127` |
| `cc` | integer | cc | controller number, `0..127` |
| `program` | integer | program | program number, `0..127` |
| `bend` | integer | bend | pitch bend, `-8192..8191`, 0 is centered |
| `time_ms` | number | all | milliseconds since the engine started |

## Return conventions

What `on_event` returns decides what flows downstream:

- `nil` (or falling off the end): the event passes through unchanged.
- `false`: the event is dropped.
- a table: one event is emitted. Mutating `ev` and returning it is the usual move.
- an array of tables: several events are emitted, in order.

An emitted table may carry `delay_ms` to schedule it late. But the reason to reach for a script is logic no built-in can express. Nothing in KDL can see the time between your notes; a script can, and can turn hesitation into emphasis:

```lua
-- The longer the silence before a note, the harder it lands.
local last_ms = 0

function on_event(ev)
    if ev.kind == "note-on" then
        local rest = math.min(ev.time_ms - last_ms, 3000)
        last_ms = ev.time_ms
        local scaled = math.floor(ev.vel * (0.6 + rest / 3000))
        ev.vel = math.max(1, math.min(127, scaled))
        return ev
    end
end
```

A script can also define a global `on_flush()`, called when the engine winds down or the scene is left, for scripts that hold notes open and want to close them on their own terms.

## Helpers

Every script gets a few globals injected:

- `seed`: the `seed=` value from the config node.
- `rng()`: a float in `[0, 1)`, deterministic per seed.
- `rng_range(lo, hi)`: an integer in `lo..=hi`, same generator.
- `note_name(key)`: `note_name(60)` is `"C4"`, handy in logs.
- `log(msg)`: print to stderr while playing. Rate-limited, so a `log` per event will not flood the terminal.

## State and determinism

Script globals persist across events, so counters, held-note maps, and toggles are ordinary Lua variables. Here is a stateful one: every third note drops an octave.

```lua
local count = 0
local dropped = {} -- keys we lowered, so the note-off follows

function on_event(ev)
    if ev.kind == "note-on" then
        count = count + 1
        if count % 3 == 0 then
            dropped[ev.key] = true
            ev.key = ev.key - 12
            return ev
        end
    elseif ev.kind == "note-off" and dropped[ev.key] then
        dropped[ev.key] = nil
        ev.key = ev.key - 12
        return ev
    end
end
```

Randomness follows [the seeds rule](/miditool/configuration/seeds/): `rng()` and `rng_range()` are seeded by the node's `seed=`, so the same seed replays the same choices forever. There is no wall-clock or unseeded randomness in the API; a scripted performance is as reproducible as a `shuffle-lock` one.

## Safety

A script cannot take the stream down with it:

- A runtime error fails open: the script becomes a passthrough, one warning goes to stderr, and sound continues.
- Notes a script started that are still sounding when you switch scenes are released automatically, the same guarantee the built-in effects have.
- Each call runs under an execution budget and a memory limit, so an accidental infinite loop or unbounded table stalls the script, not your performance.

## Reloading

Scripts reload with the config: [edit and save the config file](/miditool/guides/live-editing/) and the script is re-read along with everything else. A quick `touch`-style trick is to add or remove a trailing space in the config and save. Editing only the `.lua` file does not yet trigger a reload.
