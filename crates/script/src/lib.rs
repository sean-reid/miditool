//! User-defined MIDI effects written in Luau, running inside the effect
//! graph.
//!
//! A [`ScriptEffect`] wraps a Luau script as a [`miditool_core::Effect`]
//! leaf. Scripts run synchronously on the realtime path; sandboxing makes
//! that safe:
//!
//! - Luau only: no `io`, no `os.execute`, no `require`, no FFI.
//! - A 16 MB memory limit on the Lua heap.
//! - A per-call time budget of 5 ms, enforced through the Luau interrupt
//!   callback, so an infinite loop cannot stall the engine.
//!
//! # Fail-open
//!
//! A broken script must never silence the keyboard mid-performance. The
//! first time a handler fails for any reason (runtime error, exceeded time
//! budget, out of memory), the effect logs one line to stderr and switches
//! itself into passthrough mode: from then on every event flows through
//! unchanged, as if the node were `Pass`. [`ScriptEffect::error`] reports
//! why. Flush still releases any notes the script left sounding.
//!
//! # The Lua API
//!
//! A script defines a global function `on_event(ev)` that is called for
//! every event reaching the node. `ev` is a table:
//!
//! | field     | present for                         | range              |
//! |-----------|-------------------------------------|--------------------|
//! | `kind`    | always                              | see below          |
//! | `ch`      | always                              | 1..16              |
//! | `time_ms` | always                              | event time, number |
//! | `key`     | note-on, note-off, poly-pressure    | 0..127             |
//! | `vel`     | note-on, note-off                   | 0..127             |
//! | `value`   | poly-pressure, cc, channel-pressure | 0..127             |
//! | `cc`      | cc                                  | 0..127             |
//! | `program` | program                             | 0..127             |
//! | `bend`    | bend                                | -8192..8191        |
//!
//! `kind` is one of `"note-on"`, `"note-off"`, `"poly-pressure"`, `"cc"`,
//! `"program"`, `"channel-pressure"`, `"bend"`.
//!
//! The return value of `on_event` decides what the node emits:
//!
//! - `nil` passes the input event through unchanged.
//! - `false` drops it.
//! - A table with a `kind` field emits that single event.
//! - An array of such tables emits several events (an empty array drops).
//!
//! Emitted tables use the same fields. `kind` is required; `ch` defaults to
//! the input's channel; other fields default from the input where the input
//! carries them (`key` from any keyed event, `vel` from a note, `value`
//! from a pressure or cc event, and so on). A field that has no default and
//! is not supplied is a handler error. Numeric fields are rounded and
//! clamped to their ranges; a note-on velocity clamps to 1..127 because
//! velocity 0 means note-off on the wire. An optional `delay_ms >= 0`
//! schedules the emitted event that far after the input event's time.
//!
//! A script may also define a global `on_flush()` (no arguments), called at
//! scene switch and shutdown for the script's own cleanup. It follows the
//! same return convention except that `nil` emits nothing (there is no
//! input event to pass through), and emitted tables have no input to
//! default from, so every field their kind needs is required.
//!
//! ## Injected globals
//!
//! - `seed`: the config seed, as a number.
//! - `rng()`: a float in `[0, 1)`.
//! - `rng_range(lo, hi)`: an integer in `lo..hi` inclusive.
//! - `note_name(key)`: `"C4"` for key 60.
//! - `log(msg)`: writes to stderr, rate-limited to one line per second.
//!
//! `rng` and `rng_range` are deterministic streams derived from the seed:
//! the same seed and the same input events always produce the same
//! performance. Luau's own `math.random` is also seeded from the script
//! seed, so nothing in the sandbox is unseeded; the injected helpers are
//! still preferred because their streams are independent of library
//! internals.
//!
//! Script globals persist between events, so stateful effects (counters,
//! note maps as tables) are natural:
//!
//! ```lua
//! held = 0
//! function on_event(ev)
//!     if ev.kind == "note-on" then held = held + 1 end
//!     if ev.kind == "note-off" then held = held - 1 end
//!     return nil
//! end
//! ```
//!
//! ## Caveats
//!
//! - The `ev` table is reused between calls for speed. Copy any fields you
//!   want to keep; do not store the table itself.
//! - Note-off safety net: the node counts every note-on and note-off it
//!   emits, and on flush releases whatever is still sounding, after
//!   `on_flush` has run. This upholds the graph-level invariant (no
//!   orphaned script notes at scene switch) even for buggy scripts. Live
//!   note-off matching correctness during a performance remains the script
//!   author's job: if you rewrite the key of a note-on, rewrite its
//!   note-off the same way, or the note sticks until the next flush.
//!
//! # Realtime cost
//!
//! Building the `ev` table and reading results allocates in Lua's own heap,
//! which is bounded by the memory limit. A script node typically costs
//! single-digit microseconds per event. The Rust side avoids per-event
//! allocation on the happy path (the `ev` table is created once and its
//! fields mutated per call); error paths may allocate for messages, and the
//! injected globals take an uncontended mutex for their state.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use miditool_core::rng::seeded;
use miditool_core::{Effect, Event, EventBuf, EventKind, PerNote, ProcCx, Timestamp};
use mlua::{Function, Lua, Table, Value, VmState};
use rand::Rng;

/// Upper bound on the Lua heap, per script.
const MEMORY_LIMIT: usize = 16 * 1024 * 1024;

/// Time budget for one `on_event` or `on_flush` call. Generous next to the
/// realtime budget; its job is catching runaway loops, not shaving
/// microseconds.
const CALL_BUDGET: Duration = Duration::from_millis(5);

/// Time budget for running the script's top-level code at load time.
const LOAD_BUDGET: Duration = Duration::from_millis(50);

/// The interrupt callback reads the clock only every this many
/// invocations, keeping the common case to one relaxed atomic increment.
const INTERRUPT_STRIDE: u64 = 64;

/// Errors from loading a script. Once loaded, a script never returns an
/// error: failures at run time switch the effect into passthrough (see the
/// crate docs on fail-open).
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The script file could not be read.
    #[error("cannot read script {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// The script failed to compile or its top-level code failed. The
    /// message includes the chunk name (the file path) and line.
    #[error("{0}")]
    Compile(String),
}

/// Nanoseconds since an arbitrary process-wide anchor, monotonic.
fn monotonic_ns() -> u64 {
    static ANCHOR: OnceLock<Instant> = OnceLock::new();
    ANCHOR.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Deadline shared between the effect and the Luau interrupt callback.
struct Budget {
    /// `u64::MAX` while no handler is running.
    deadline_ns: AtomicU64,
    /// Interrupt invocations, for the clock-read stride.
    ticks: AtomicU64,
}

impl Budget {
    fn new() -> Self {
        Self {
            deadline_ns: AtomicU64::new(u64::MAX),
            ticks: AtomicU64::new(0),
        }
    }

    fn arm(&self, budget: Duration) {
        let deadline = monotonic_ns().saturating_add(budget.as_nanos() as u64);
        self.deadline_ns.store(deadline, Ordering::Relaxed);
    }

    fn disarm(&self) {
        self.deadline_ns.store(u64::MAX, Ordering::Relaxed);
    }

    fn exceeded(&self) -> bool {
        self.ticks
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(INTERRUPT_STRIDE)
            && monotonic_ns() > self.deadline_ns.load(Ordering::Relaxed)
    }
}

/// A Luau script as an effect-graph leaf. See the crate docs for the Lua
/// API and the fail-open contract.
pub struct ScriptEffect {
    name: String,
    on_event: Function,
    on_flush: Option<Function>,
    /// The `ev` table, created once and mutated per call.
    ev_table: Table,
    budget: Arc<Budget>,
    /// Note-ons minus note-offs emitted by this node, per (channel, key).
    /// Flush releases every slot still positive.
    sounding: PerNote<u8>,
    /// `Some` once a handler has failed; the effect is in passthrough mode
    /// and this holds the reason. Doubles as the warned-once flag.
    error: Option<String>,
    /// Owns the interpreter; handles above keep it alive too, this field
    /// makes the ownership explicit.
    _lua: Lua,
}

impl std::fmt::Debug for ScriptEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptEffect")
            .field("name", &self.name)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl ScriptEffect {
    /// Load and compile a script from a file. Errors are human-readable and
    /// include the file and line.
    pub fn from_file(path: &Path, seed: u64) -> Result<ScriptEffect, ScriptError> {
        let source = std::fs::read_to_string(path).map_err(|source| ScriptError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_source(&path.display().to_string(), &source, seed)
    }

    /// Load and compile a script from a source string. `name` labels error
    /// messages and log lines.
    pub fn from_source(name: &str, source: &str, seed: u64) -> Result<ScriptEffect, ScriptError> {
        let lua = Lua::new();
        let compile = |e: mlua::Error| ScriptError::Compile(e.to_string());
        lua.set_memory_limit(MEMORY_LIMIT).map_err(compile)?;

        let budget = Arc::new(Budget::new());
        let interrupt_budget = budget.clone();
        lua.set_interrupt(move |_| {
            if interrupt_budget.exceeded() {
                return Err(mlua::Error::runtime(
                    "script exceeded the 5 ms per-call time budget",
                ));
            }
            Ok(VmState::Continue)
        });

        install_globals(&lua, name, seed).map_err(compile)?;

        budget.arm(LOAD_BUDGET);
        let loaded = lua.load(source).set_name(name).exec();
        budget.disarm();
        loaded.map_err(compile)?;

        let globals = lua.globals();
        let Ok(on_event) = globals.get::<Function>("on_event") else {
            return Err(ScriptError::Compile(format!(
                "{name}: script defines no global `on_event` function"
            )));
        };
        let on_flush = match globals.get::<Value>("on_flush").map_err(compile)? {
            Value::Function(f) => Some(f),
            Value::Nil => None,
            other => {
                return Err(ScriptError::Compile(format!(
                    "{name}: `on_flush` must be a function, got {}",
                    other.type_name()
                )));
            }
        };
        let ev_table = lua.create_table().map_err(compile)?;

        Ok(ScriptEffect {
            name: name.to_string(),
            on_event,
            on_flush,
            ev_table,
            budget,
            sounding: PerNote::new(),
            error: None,
            _lua: lua,
        })
    }

    /// The error that switched this script into passthrough mode, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Enter passthrough mode, warning on stderr once per script.
    fn fail(&mut self, err: &mlua::Error) {
        if self.error.is_some() {
            return;
        }
        let msg = err.to_string();
        eprintln!(
            "miditool-script: script '{}' failed: {msg}; passing events through from now on",
            self.name
        );
        self.error = Some(msg);
    }

    /// Push an event, counting emitted note-ons and note-offs so flush can
    /// release whatever is left sounding. Tracks only what actually fit in
    /// the buffer.
    fn emit(&mut self, ev: Event, out: &mut EventBuf, cx: &ProcCx) {
        if out.try_push(ev).is_err() {
            cx.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match ev.kind {
            EventKind::NoteOn { ch, key, .. } => {
                let n = self.sounding.get(ch, key);
                self.sounding.set(ch, key, n.saturating_add(1));
            }
            EventKind::NoteOff { ch, key, .. } => {
                let n = self.sounding.get(ch, key);
                self.sounding.set(ch, key, n.saturating_sub(1));
            }
            _ => {}
        }
    }

    /// Call `on_event`. `Ok(None)` means pass the input through; `Ok(buf)`
    /// means emit exactly these events (possibly none).
    fn run_on_event(&self, ev: &Event) -> mlua::Result<Option<EventBuf>> {
        fill_ev_table(&self.ev_table, ev)?;
        self.budget.arm(CALL_BUDGET);
        let result = self.on_event.call::<Value>(&self.ev_table);
        self.budget.disarm();
        match result? {
            Value::Nil => Ok(None),
            Value::Boolean(false) => Ok(Some(EventBuf::new())),
            Value::Table(t) => {
                let mut buf = EventBuf::new();
                collect_events(&t, Some(ev), ev.time, &mut buf)?;
                Ok(Some(buf))
            }
            other => Err(mlua::Error::runtime(format!(
                "on_event returned {}, expected nil, false, or a table",
                other.type_name()
            ))),
        }
    }

    /// Call `on_flush` and collect its events. `nil` and `false` both mean
    /// nothing to emit.
    fn run_on_flush(&self, f: &Function, now: Timestamp) -> mlua::Result<EventBuf> {
        self.budget.arm(CALL_BUDGET);
        let result = f.call::<Value>(());
        self.budget.disarm();
        match result? {
            Value::Nil | Value::Boolean(false) => Ok(EventBuf::new()),
            Value::Table(t) => {
                let mut buf = EventBuf::new();
                collect_events(&t, None, now, &mut buf)?;
                Ok(buf)
            }
            other => Err(mlua::Error::runtime(format!(
                "on_flush returned {}, expected nil, false, or a table",
                other.type_name()
            ))),
        }
    }
}

impl Effect for ScriptEffect {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        if self.error.is_some() {
            self.emit(*ev, out, cx);
            return;
        }
        match self.run_on_event(ev) {
            Ok(None) => self.emit(*ev, out, cx),
            Ok(Some(emitted)) => {
                for e in emitted {
                    self.emit(e, out, cx);
                }
            }
            Err(e) => {
                self.fail(&e);
                self.emit(*ev, out, cx);
            }
        }
    }

    fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        if self.error.is_none()
            && let Some(f) = self.on_flush.clone()
        {
            match self.run_on_flush(&f, cx.now) {
                Ok(emitted) => {
                    for e in emitted {
                        self.emit(e, out, cx);
                    }
                }
                Err(e) => self.fail(&e),
            }
        }
        // Safety net: release everything this node still has sounding,
        // whatever state the script is in.
        let sounding = std::mem::take(&mut self.sounding);
        sounding.for_each(|ch, key, n| {
            if n == 0 {
                return;
            }
            let off = EventKind::NoteOff { ch, key, vel: 0 };
            if out.try_push(Event::new(cx.now, off)).is_err() {
                cx.dropped.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
}

/// Install `seed`, `rng`, `rng_range`, `note_name`, and `log`.
fn install_globals(lua: &Lua, name: &str, seed: u64) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.set("seed", seed as f64)?;

    // Luau's own math.random is seeded from the script seed, so even a
    // script that reaches for it stays deterministic per seed.
    lua.load(format!("math.randomseed({seed})"))
        .set_name("=seed math.random")
        .exec()?;

    let rng = Mutex::new(seeded(seed, 0));
    globals.set(
        "rng",
        lua.create_function(move |_, ()| Ok(rng.lock().unwrap().random::<f64>()))?,
    )?;

    let range_rng = Mutex::new(seeded(seed, 1));
    globals.set(
        "rng_range",
        lua.create_function(move |_, (lo, hi): (i64, i64)| {
            if lo > hi {
                return Err(mlua::Error::runtime(format!(
                    "rng_range: lo ({lo}) is greater than hi ({hi})"
                )));
            }
            Ok(range_rng.lock().unwrap().random_range(lo..=hi))
        })?,
    )?;

    globals.set(
        "note_name",
        lua.create_function(|_, key: i64| {
            if !(0..=127).contains(&key) {
                return Err(mlua::Error::runtime(format!(
                    "note_name: key {key} is outside 0..127"
                )));
            }
            const NAMES: [&str; 12] = [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ];
            Ok(format!("{}{}", NAMES[(key % 12) as usize], key / 12 - 1))
        })?,
    )?;

    let script = name.to_string();
    let last_log = Mutex::new(None::<Instant>);
    globals.set(
        "log",
        lua.create_function(move |_, msg: String| {
            let mut last = last_log.lock().unwrap();
            if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                eprintln!("[script {script}] {msg}");
                *last = Some(Instant::now());
            }
            Ok(())
        })?,
    )?;
    Ok(())
}

/// Write `ev` into the reused table, clearing fields the kind lacks.
fn fill_ev_table(t: &Table, ev: &Event) -> mlua::Result<()> {
    for field in ["key", "vel", "value", "cc", "program", "bend", "delay_ms"] {
        t.raw_set(field, Value::Nil)?;
    }
    t.raw_set("time_ms", ev.time as f64 / 1_000_000.0)?;
    t.raw_set("ch", ev.kind.channel() as i64 + 1)?;
    match ev.kind {
        EventKind::NoteOn { key, vel, .. } => {
            t.raw_set("kind", "note-on")?;
            t.raw_set("key", key)?;
            t.raw_set("vel", vel)?;
        }
        EventKind::NoteOff { key, vel, .. } => {
            t.raw_set("kind", "note-off")?;
            t.raw_set("key", key)?;
            t.raw_set("vel", vel)?;
        }
        EventKind::PolyPressure { key, value, .. } => {
            t.raw_set("kind", "poly-pressure")?;
            t.raw_set("key", key)?;
            t.raw_set("value", value)?;
        }
        EventKind::ControlChange { cc, value, .. } => {
            t.raw_set("kind", "cc")?;
            t.raw_set("cc", cc)?;
            t.raw_set("value", value)?;
        }
        EventKind::ProgramChange { program, .. } => {
            t.raw_set("kind", "program")?;
            t.raw_set("program", program)?;
        }
        EventKind::ChannelPressure { value, .. } => {
            t.raw_set("kind", "channel-pressure")?;
            t.raw_set("value", value)?;
        }
        EventKind::PitchBend { value, .. } => {
            t.raw_set("kind", "bend")?;
            t.raw_set("bend", value)?;
        }
    }
    Ok(())
}

/// Read events out of a handler's return table: a single event table (one
/// with a `kind` field) or an array of them. Overflowing the buffer is a
/// handler error; scripts should stay far below `MAX_FANOUT` per input.
fn collect_events(
    t: &Table,
    input: Option<&Event>,
    base_time: Timestamp,
    buf: &mut EventBuf,
) -> mlua::Result<()> {
    fn push(buf: &mut EventBuf, ev: Event) -> mlua::Result<()> {
        buf.try_push(ev)
            .map_err(|_| mlua::Error::runtime("script emitted too many events for one input"))
    }
    if !matches!(t.raw_get::<Value>("kind")?, Value::Nil) {
        return push(buf, event_from_table(t, input, base_time)?);
    }
    for item in t.sequence_values::<Value>() {
        let Value::Table(item) = item? else {
            return Err(mlua::Error::runtime(
                "array entries returned from a handler must be event tables",
            ));
        };
        push(buf, event_from_table(&item, input, base_time)?)?;
    }
    Ok(())
}

/// Build one event from a returned table. Missing fields default from the
/// input event where it carries them; numbers are rounded and clamped.
fn event_from_table(t: &Table, input: Option<&Event>, base_time: Timestamp) -> mlua::Result<Event> {
    let Value::String(kind) = t.raw_get::<Value>("kind")? else {
        return Err(mlua::Error::runtime(
            "emitted event needs a `kind` string field",
        ));
    };
    let kind = kind.to_str()?;
    let kind = kind.as_ref();

    let in_kind = input.map(|e| e.kind);
    let ch = match number_field(t, "ch")? {
        Some(n) => (n.round() as i64).clamp(1, 16) as u8 - 1,
        None => in_kind
            .map(|k| k.channel())
            .ok_or_else(|| missing("ch", kind))?,
    };
    let key = |t: &Table| clamped_field(t, "key", 0, 127, in_kind.and_then(|k| k.key()), kind);
    let vel_default = match in_kind {
        Some(EventKind::NoteOn { vel, .. }) | Some(EventKind::NoteOff { vel, .. }) => Some(vel),
        _ => None,
    };
    let value_default = match in_kind {
        Some(EventKind::PolyPressure { value, .. })
        | Some(EventKind::ControlChange { value, .. })
        | Some(EventKind::ChannelPressure { value, .. }) => Some(value),
        _ => None,
    };

    let event_kind = match kind {
        "note-on" => EventKind::NoteOn {
            ch,
            key: key(t)?,
            // Velocity 0 would mean note-off on the wire; clamp it away.
            vel: clamped_field(t, "vel", 1, 127, vel_default, kind)?,
        },
        "note-off" => EventKind::NoteOff {
            ch,
            key: key(t)?,
            vel: clamped_field(t, "vel", 0, 127, vel_default, kind)?,
        },
        "poly-pressure" => EventKind::PolyPressure {
            ch,
            key: key(t)?,
            value: clamped_field(t, "value", 0, 127, value_default, kind)?,
        },
        "cc" => {
            let cc_default = match in_kind {
                Some(EventKind::ControlChange { cc, .. }) => Some(cc),
                _ => None,
            };
            EventKind::ControlChange {
                ch,
                cc: clamped_field(t, "cc", 0, 127, cc_default, kind)?,
                value: clamped_field(t, "value", 0, 127, value_default, kind)?,
            }
        }
        "program" => {
            let program_default = match in_kind {
                Some(EventKind::ProgramChange { program, .. }) => Some(program),
                _ => None,
            };
            EventKind::ProgramChange {
                ch,
                program: clamped_field(t, "program", 0, 127, program_default, kind)?,
            }
        }
        "channel-pressure" => EventKind::ChannelPressure {
            ch,
            value: clamped_field(t, "value", 0, 127, value_default, kind)?,
        },
        "bend" => EventKind::PitchBend {
            ch,
            value: match number_field(t, "bend")? {
                Some(n) => (n.round() as i64).clamp(-8192, 8191) as i16,
                None => match in_kind {
                    Some(EventKind::PitchBend { value, .. }) => value,
                    _ => return Err(missing("bend", kind)),
                },
            },
        },
        other => {
            return Err(mlua::Error::runtime(format!(
                "unknown event kind `{other}`"
            )));
        }
    };

    let time = match number_field(t, "delay_ms")? {
        None => base_time,
        Some(ms) if ms >= 0.0 => base_time + (ms * 1_000_000.0).round() as u64,
        Some(ms) => {
            return Err(mlua::Error::runtime(format!(
                "delay_ms must be >= 0, got {ms}"
            )));
        }
    };
    Ok(Event::new(time, event_kind))
}

fn missing(field: &str, kind: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "emitted `{kind}` event is missing `{field}` and the input has no value to inherit"
    ))
}

/// Read an optional numeric field; a present non-number is an error.
fn number_field(t: &Table, field: &str) -> mlua::Result<Option<f64>> {
    match t.raw_get::<Value>(field)? {
        Value::Nil => Ok(None),
        Value::Integer(n) => Ok(Some(n as f64)),
        Value::Number(n) => Ok(Some(n)),
        other => Err(mlua::Error::runtime(format!(
            "field `{field}` must be a number, got {}",
            other.type_name()
        ))),
    }
}

/// Read a 7-bit field, defaulting from the input and clamping into
/// `lo..=hi`.
fn clamped_field(
    t: &Table,
    field: &str,
    lo: u8,
    hi: u8,
    default: Option<u8>,
    kind: &str,
) -> mlua::Result<u8> {
    match number_field(t, field)? {
        Some(n) => Ok((n.round() as i64).clamp(lo as i64, hi as i64) as u8),
        None => default
            .map(|d| d.clamp(lo, hi))
            .ok_or_else(|| missing(field, kind)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(source: &str) -> ScriptEffect {
        ScriptEffect::from_source("test", source, 42).expect("script loads")
    }

    fn run(fx: &mut ScriptEffect, ev: Event) -> Vec<Event> {
        let cx = ProcCx::at(ev.time);
        let mut out = EventBuf::new();
        fx.process(&ev, &mut out, &cx);
        out.iter().copied().collect()
    }

    fn flush_at(fx: &mut ScriptEffect, now: Timestamp) -> Vec<Event> {
        let cx = ProcCx::at(now);
        let mut out = EventBuf::new();
        fx.flush(&mut out, &cx);
        out.iter().copied().collect()
    }

    fn on(key: u8) -> Event {
        Event::new(
            0,
            EventKind::NoteOn {
                ch: 0,
                key,
                vel: 100,
            },
        )
    }

    fn off(key: u8) -> Event {
        Event::new(0, EventKind::NoteOff { ch: 0, key, vel: 0 })
    }

    fn cc(cc: u8, value: u8) -> Event {
        Event::new(0, EventKind::ControlChange { ch: 0, cc, value })
    }

    #[test]
    fn transpose_round_trip() {
        let mut fx = load(
            r#"
            function on_event(ev)
                if ev.key then ev.key = ev.key + 12 end
                return ev
            end
            "#,
        );
        assert_eq!(
            run(&mut fx, on(60)),
            vec![Event::new(
                0,
                EventKind::NoteOn {
                    ch: 0,
                    key: 72,
                    vel: 100
                }
            )]
        );
        assert_eq!(
            run(&mut fx, off(60)),
            vec![Event::new(
                0,
                EventKind::NoteOff {
                    ch: 0,
                    key: 72,
                    vel: 0
                }
            )]
        );
        // Non-keyed events fall through the guard untouched.
        assert_eq!(run(&mut fx, cc(64, 127)), vec![cc(64, 127)]);
    }

    #[test]
    fn return_false_drops() {
        let mut fx = load("function on_event(ev) return false end");
        assert_eq!(run(&mut fx, on(60)), vec![]);
    }

    #[test]
    fn return_nil_passes_through() {
        let mut fx = load("function on_event(ev) return nil end");
        let ev = Event::new(12_345, EventKind::PitchBend { ch: 4, value: -100 });
        assert_eq!(run(&mut fx, ev), vec![ev]);
    }

    #[test]
    fn empty_array_drops() {
        let mut fx = load("function on_event(ev) return {} end");
        assert_eq!(run(&mut fx, on(60)), vec![]);
        assert_eq!(fx.error(), None);
    }

    #[test]
    fn array_emits_several_events() {
        let mut fx = load(
            r#"
            function on_event(ev)
                return {
                    { kind = "note-on", key = 60 },
                    { kind = "note-on", key = 64 },
                    { kind = "note-on", key = 67 },
                }
            end
            "#,
        );
        let out = run(
            &mut fx,
            Event::new(
                7,
                EventKind::NoteOn {
                    ch: 2,
                    key: 60,
                    vel: 90,
                },
            ),
        );
        let keys: Vec<u8> = out.iter().filter_map(|e| e.kind.key()).collect();
        assert_eq!(keys, vec![60, 64, 67]);
        for e in &out {
            // ch, vel, and time all inherited from the input.
            assert_eq!(e.time, 7);
            assert!(matches!(e.kind, EventKind::NoteOn { ch: 2, vel: 90, .. }));
        }
    }

    #[test]
    fn delay_ms_lands_in_event_time() {
        let mut fx =
            load(r#"function on_event(ev) return { kind = "note-on", delay_ms = 250 } end"#);
        let ev = Event::new(
            1_000_000,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel: 100,
            },
        );
        let out = run(&mut fx, ev);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].time, 1_000_000 + 250_000_000);
    }

    #[test]
    fn negative_delay_fails_open() {
        let mut fx =
            load(r#"function on_event(ev) return { kind = "note-on", delay_ms = -1 } end"#);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert!(fx.error().unwrap().contains("delay_ms"));
    }

    #[test]
    fn defaults_inherit_from_input() {
        let mut fx = load(r#"function on_event(ev) return { kind = "note-off" } end"#);
        let ev = Event::new(
            3,
            EventKind::NoteOn {
                ch: 5,
                key: 61,
                vel: 99,
            },
        );
        assert_eq!(
            run(&mut fx, ev),
            vec![Event::new(
                3,
                EventKind::NoteOff {
                    ch: 5,
                    key: 61,
                    vel: 99
                }
            )]
        );

        let mut fx = load(r#"function on_event(ev) return { kind = "cc", value = 10 } end"#);
        assert_eq!(
            run(&mut fx, cc(64, 127)),
            vec![Event::new(
                0,
                EventKind::ControlChange {
                    ch: 0,
                    cc: 64,
                    value: 10
                }
            )]
        );
    }

    #[test]
    fn fields_are_clamped() {
        let mut fx = load(
            r#"
            function on_event(ev)
                return { kind = "note-on", ch = 99, key = 300, vel = -5 }
            end
            "#,
        );
        assert_eq!(
            run(&mut fx, on(60)),
            vec![Event::new(
                0,
                EventKind::NoteOn {
                    ch: 15,
                    key: 127,
                    vel: 1
                }
            )]
        );

        let mut fx = load(r#"function on_event(ev) return { kind = "bend", bend = 20000 } end"#);
        assert_eq!(
            run(&mut fx, on(60)),
            vec![Event::new(0, EventKind::PitchBend { ch: 0, value: 8191 })]
        );
    }

    #[test]
    fn bad_kind_fails_open() {
        let mut fx = load(r#"function on_event(ev) return { kind = "warble" } end"#);
        // The failing event passes through, and so does everything after.
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert!(fx.error().unwrap().contains("warble"));
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn bad_field_type_fails_open() {
        let mut fx = load(r#"function on_event(ev) return { kind = "note-on", key = "high" } end"#);
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert!(fx.error().unwrap().contains("key"));
    }

    #[test]
    fn bad_return_type_fails_open() {
        let mut fx = load("function on_event(ev) return 7 end");
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert!(
            fx.error()
                .unwrap()
                .contains("expected nil, false, or a table")
        );
    }

    #[test]
    fn missing_field_with_no_default_fails_open() {
        // A note-on emitted from a cc input has no key to inherit.
        let mut fx = load(r#"function on_event(ev) return { kind = "note-on", vel = 100 } end"#);
        assert_eq!(run(&mut fx, cc(1, 2)), vec![cc(1, 2)]);
        assert!(fx.error().unwrap().contains("key"));
    }

    const RANDOM_NOTES: &str = r#"
        function on_event(ev)
            if ev.kind ~= "note-on" then return nil end
            return { kind = "note-on", key = rng_range(0, 127), vel = 1 + math.floor(rng() * 127) }
        end
    "#;

    #[test]
    fn math_random_is_seeded_too() {
        let src = r#"
            function on_event(ev)
                if ev.kind == "note-on" then
                    ev.vel = 1 + math.floor(math.random() * 126)
                    return ev
                end
            end
        "#;
        let mut a = ScriptEffect::from_source("a", src, 5).unwrap();
        let mut b = ScriptEffect::from_source("b", src, 5).unwrap();
        let mut c = ScriptEffect::from_source("c", src, 6).unwrap();
        let mut same = true;
        let mut all_match_c = true;
        for key in [60u8, 62, 64, 65, 67] {
            let ra = run(&mut a, on(key));
            let rb = run(&mut b, on(key));
            let rc = run(&mut c, on(key));
            same &= ra == rb;
            all_match_c &= ra == rc;
        }
        assert!(same, "math.random must be deterministic per seed");
        assert!(!all_match_c, "a different seed must change math.random");
    }

    #[test]
    fn same_seed_same_output() {
        let mut a = ScriptEffect::from_source("a", RANDOM_NOTES, 7).unwrap();
        let mut b = ScriptEffect::from_source("b", RANDOM_NOTES, 7).unwrap();
        for key in 0..50u8 {
            assert_eq!(run(&mut a, on(key)), run(&mut b, on(key)));
        }
    }

    #[test]
    fn different_seeds_differ() {
        let mut a = ScriptEffect::from_source("a", RANDOM_NOTES, 7).unwrap();
        let mut b = ScriptEffect::from_source("b", RANDOM_NOTES, 8).unwrap();
        let diverged = (0..50u8).any(|key| run(&mut a, on(key)) != run(&mut b, on(key)));
        assert!(diverged);
    }

    #[test]
    fn rng_range_stays_in_bounds() {
        let mut fx = load(
            r#"
            function on_event(ev)
                return { kind = "note-on", key = rng_range(10, 12) }
            end
            "#,
        );
        let mut seen = [false; 3];
        for _ in 0..100 {
            let out = run(&mut fx, on(60));
            let key = out[0].kind.key().unwrap();
            assert!((10..=12).contains(&key), "key {key} out of range");
            seen[key as usize - 10] = true;
        }
        assert_eq!(seen, [true; 3], "all three values should be hit");
    }

    #[test]
    fn rng_range_reversed_bounds_fail_open() {
        let mut fx = load(
            r#"function on_event(ev) return { kind = "cc", cc = rng_range(9, 3), value = 0 } end"#,
        );
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert!(fx.error().unwrap().contains("rng_range"));
    }

    #[test]
    fn globals_persist_between_events() {
        let mut fx = load(
            r#"
            n = 0
            function on_event(ev)
                n = n + 1
                ev.vel = math.min(n, 127)
                return ev
            end
            "#,
        );
        let vels: Vec<u8> = (0..3)
            .filter_map(|_| match run(&mut fx, on(60))[0].kind {
                EventKind::NoteOn { vel, .. } => Some(vel),
                _ => None,
            })
            .collect();
        assert_eq!(vels, vec![1, 2, 3]);
    }

    #[test]
    fn injected_globals_are_available() {
        // Loading runs the top level; a wrong value raises there and the
        // script fails to load.
        load(
            r#"
            assert(seed == 42, "seed")
            assert(note_name(60) == "C4", "note_name 60")
            assert(note_name(0) == "C-1", "note_name 0")
            assert(note_name(127) == "G9", "note_name 127")
            log("script loaded")
            function on_event(ev) return nil end
            "#,
        );
    }

    #[test]
    fn time_ms_is_visible_to_the_script() {
        let mut fx = load(
            r#"function on_event(ev) return { kind = "cc", cc = 1, value = math.floor(ev.time_ms) } end"#,
        );
        let ev = Event::new(
            5_000_000,
            EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel: 100,
            },
        );
        assert_eq!(
            run(&mut fx, ev)[0].kind,
            EventKind::ControlChange {
                ch: 0,
                cc: 1,
                value: 5
            }
        );
    }

    #[test]
    fn reused_ev_table_clears_stale_fields() {
        let mut fx = load(
            r#"
            function on_event(ev)
                if ev.kind == "cc" then
                    assert(ev.key == nil, "stale key on a cc event")
                    assert(ev.vel == nil, "stale vel on a cc event")
                end
                return nil
            end
            "#,
        );
        run(&mut fx, on(60));
        run(&mut fx, cc(64, 127));
        assert_eq!(fx.error(), None);
    }

    #[test]
    fn on_flush_events_are_emitted() {
        let mut fx = load(
            r#"
            function on_event(ev) return nil end
            function on_flush()
                return { { kind = "cc", ch = 3, cc = 64, value = 0 } }
            end
            "#,
        );
        assert_eq!(
            flush_at(&mut fx, 99),
            vec![Event::new(
                99,
                EventKind::ControlChange {
                    ch: 2,
                    cc: 64,
                    value: 0
                }
            )]
        );
    }

    #[test]
    fn safety_net_releases_orphaned_notes() {
        // The script swallows note-offs, so its note-ons never resolve.
        let mut fx = load(
            r#"
            function on_event(ev)
                if ev.kind == "note-off" then return false end
                return nil
            end
            "#,
        );
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, off(60)), vec![]);
        assert_eq!(
            flush_at(&mut fx, 500),
            vec![Event::new(
                500,
                EventKind::NoteOff {
                    ch: 0,
                    key: 60,
                    vel: 0
                }
            )]
        );
        // The slate is clean afterwards.
        assert_eq!(flush_at(&mut fx, 501), vec![]);
    }

    #[test]
    fn safety_net_runs_after_on_flush() {
        // A note-on emitted by on_flush itself is still released.
        let mut fx = load(
            r#"
            function on_event(ev) return false end
            function on_flush()
                return { { kind = "note-on", ch = 1, key = 70, vel = 80 } }
            end
            "#,
        );
        assert_eq!(
            flush_at(&mut fx, 10),
            vec![
                Event::new(
                    10,
                    EventKind::NoteOn {
                        ch: 0,
                        key: 70,
                        vel: 80
                    }
                ),
                Event::new(
                    10,
                    EventKind::NoteOff {
                        ch: 0,
                        key: 70,
                        vel: 0
                    }
                ),
            ]
        );
    }

    #[test]
    fn handler_error_fails_open_and_warns_once() {
        let mut fx = load(
            r#"
            n = 0
            function on_event(ev)
                n = n + 1
                if n == 3 then error("boom") end
                return false
            end
            "#,
        );
        // The script drops the first two events.
        assert_eq!(run(&mut fx, on(60)), vec![]);
        assert_eq!(run(&mut fx, on(61)), vec![]);
        assert_eq!(fx.error(), None);
        // The third errors: it passes through and trips fail-open (the
        // single stderr warning is guarded by the same flag).
        assert_eq!(run(&mut fx, on(62)), vec![on(62)]);
        assert!(fx.error().unwrap().contains("boom"));
        // From now on everything passes through, script untouched.
        assert_eq!(run(&mut fx, on(63)), vec![on(63)]);
        assert_eq!(run(&mut fx, off(63)), vec![off(63)]);
    }

    #[test]
    fn fail_open_flush_still_releases_tracked_notes() {
        let mut fx = load(
            r#"
            n = 0
            function on_event(ev)
                n = n + 1
                if n == 2 then error("boom") end
                return nil
            end
            "#,
        );
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert_eq!(run(&mut fx, cc(1, 1)), vec![cc(1, 1)]);
        assert!(fx.error().is_some());
        // Passthrough events keep being tracked in fail-open mode.
        assert_eq!(run(&mut fx, on(61)), vec![on(61)]);
        let mut released = flush_at(&mut fx, 0);
        released.sort_by_key(|e| e.kind.key());
        assert_eq!(released, vec![off(60), off(61)]);
    }

    #[test]
    fn runaway_loop_is_interrupted() {
        let mut fx = load("function on_event(ev) while true do end end");
        let start = Instant::now();
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        assert!(fx.error().unwrap().contains("budget"));
        // Interrupted near the 5 ms budget, not seconds later.
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn memory_bomb_fails_open_without_aborting() {
        let mut fx = load(
            r#"
            function on_event(ev)
                local t = {}
                while true do t[#t + 1] = table.create(65536) end
            end
            "#,
        );
        assert_eq!(run(&mut fx, on(60)), vec![on(60)]);
        let err = fx.error().unwrap().to_lowercase();
        assert!(err.contains("memory"), "unexpected error: {err}");
        assert_eq!(run(&mut fx, off(60)), vec![off(60)]);
    }

    #[test]
    fn compile_error_surfaces_file_and_line() {
        let dir = std::env::temp_dir().join(format!("miditool-script-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.luau");
        std::fs::write(&path, "local x = 1\nthis is not lua\n").unwrap();
        let err = ScriptEffect::from_file(&path, 0).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("broken.luau"), "no file in: {msg}");
        assert!(msg.contains(":2"), "no line in: {msg}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_file_loads_and_runs() {
        let dir = std::env::temp_dir().join(format!("miditool-script-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drop_all.luau");
        std::fs::write(&path, "function on_event(ev) return false end\n").unwrap();
        let mut fx = ScriptEffect::from_file(&path, 0).unwrap();
        assert_eq!(run(&mut fx, on(60)), vec![]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let err = ScriptEffect::from_file(Path::new("/nonexistent/nope.luau"), 0).unwrap_err();
        assert!(matches!(err, ScriptError::Io { .. }));
        assert!(err.to_string().contains("nope.luau"));
    }

    #[test]
    fn missing_on_event_is_a_load_error() {
        let err = ScriptEffect::from_source("test", "x = 1", 0).unwrap_err();
        assert!(err.to_string().contains("on_event"));
    }

    #[test]
    fn non_function_on_flush_is_a_load_error() {
        let err = ScriptEffect::from_source(
            "test",
            "function on_event(ev) return nil end\non_flush = 3",
            0,
        )
        .unwrap_err();
        assert!(err.to_string().contains("on_flush"));
    }

    #[test]
    fn per_event_cost_stays_reasonable() {
        // A smoke bound, not a benchmark: 10k events through a transposing
        // script must average far below a millisecond each.
        let mut fx = load(
            r#"
            function on_event(ev)
                if ev.key then ev.key = math.min(ev.key + 12, 127) end
                return ev
            end
            "#,
        );
        let start = Instant::now();
        for i in 0..10_000u64 {
            let ev = Event::new(
                i,
                EventKind::NoteOn {
                    ch: 0,
                    key: (i % 128) as u8,
                    vel: 100,
                },
            );
            assert_eq!(run(&mut fx, ev).len(), 1);
        }
        let per_event = start.elapsed() / 10_000;
        println!("per-event cost: {per_event:?}");
        assert!(
            per_event < Duration::from_millis(1),
            "per-event cost {per_event:?}"
        );
    }
}
