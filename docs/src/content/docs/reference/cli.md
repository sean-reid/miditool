---
title: CLI reference
description: Every miditool subcommand, its synopsis, and the shape of its output. run, ports, monitor, effects, hide, unhide, bench, doctor.
---

```text
miditool <command>

  run       Run an effect graph between an input port and an output port
  ports     List MIDI input and output ports
  monitor   Print incoming MIDI events from an input port
  effects   List the built-in effects and their parameters
  hide      Hide a MIDI source from other apps until Ctrl-C (macOS only)
  unhide    Restore sources hidden by a crashed run (macOS only)
  bench     Measure round-trip latency through a pass-through engine
  doctor    Check the environment: ports, config, hidden sources, DAW state
```

## miditool run

```sh
miditool run [CONFIG]
```

Runs the effect graph. `CONFIG` defaults to `./miditool.kdl`. The command holds the ports open, watches the config for [live edits](/miditool/guides/live-editing/), and winds down on Ctrl-C by releasing every held note.

```text
miditool: Roland -> miditool Out (virtual). Ctrl-C to stop.
```

If the config sets `hide=true`, the input is hidden after connecting and restored on exit; see [the GarageBand guide](/miditool/guides/garageband/).

## miditool ports

```sh
miditool ports
```

Lists every MIDI input and output the system offers, one per line:

```text
inputs:
  Roland FP-30 MIDI IN
outputs:
  Roland FP-30 MIDI OUT
  IAC Driver Bus 1
```

## miditool monitor

```sh
miditool monitor [--input <SUBSTRING>]
```

Prints incoming events from an input port. `--input` matches a substring of the port name; without it the first non-miditool port is used. Timestamps are seconds since the first event.

```text
monitoring. Ctrl-C to stop.
     0.000  ch1  note-on   C4   (60) vel 96
     0.412  ch1  note-off  C4   (60) vel 0
     1.003  ch1  cc64 = 127
     2.741  ch1  bend +2048
```

## miditool effects

```sh
miditool effects
```

Prints the built-in effects reference: every effect with its parameters and defaults, the routing nodes, and the config file shape. It is the offline, always-current version of the [Effects](/miditool/effects/shuffle-lock/) section.

## miditool hide

```sh
miditool hide <NAME>
```

Hides the MIDI source matching `NAME` from every other app, until Ctrl-C restores it. macOS only. Mostly useful for testing; `miditool run` hides and restores by itself when the config says `input "..." hide=true`.

```text
Roland FP-30 MIDI IN is hidden from other apps; restart any app that was already listening. Ctrl-C to restore.
```

## miditool unhide

```sh
miditool unhide [NAME]
```

Restores sources hidden by a run that was killed hard. macOS only. Without `NAME` it restores every hidden source:

```text
restored Roland FP-30 MIDI IN
```

## miditool bench

```sh
miditool bench [--rounds <N>]
```

Measures round-trip latency by sending `N` note pairs (default 500) through a live pass-through engine: a virtual source, the whole decode-process-encode path, a virtual sink, and back through the OS MIDI service to a listener. The numbers cover the entire stack, not just miditool's pipeline.

```text
bench: 500 note pairs, miditool bench in -> engine -> miditool bench out
  count         min         p50         p90         p99         max   lost
   1000     212.4us     341.7us     498.2us     772.9us    1103.5us      0
```

Read `p50` as the typical added latency and `p99` as the bad case; both are normally well under a millisecond, far below the few milliseconds a key press takes to travel over USB. A nonzero `lost` column means another app grabbed the bench ports; close it and rerun. Needs virtual ports, so it runs on macOS and Linux but [not Windows](/miditool/guides/windows/).

## miditool doctor

```sh
miditool doctor [CONFIG]
```

Runs every environment check and prints one verdict line per check: `ok`, `warn`, or `fail`. All checks run even when one fails, so a single report shows everything that needs attention. Exit status is nonzero only for hard failures (no MIDI backend, a config that does not parse).

```text
ok    midi backend: 1 input (Roland FP-30 MIDI IN), 2 outputs (Roland FP-30 MIDI OUT, IAC Driver Bus 1)
ok    config miditool.kdl: parses, 2 top-level effects
warn  possibly hidden (or offline): Arturia KeyLab; run `miditool unhide` to be sure
warn  GarageBand is running; apps started before miditool keep hearing the raw keyboard until relaunched
ok    Logic Pro is not running
```

The platform-specific checks: hidden sources and running DAWs on macOS, the ALSA sequencer on Linux, loopMIDI on Windows.
