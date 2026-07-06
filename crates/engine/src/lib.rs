//! The realtime run loop: a minimal MIDI input callback hands raw packets
//! to a dedicated graph thread that decodes them, runs the effect graph on
//! a steady tick, and feeds a scheduler thread that sends each event at
//! its intended time, tracking what is sounding so shutdown leaves no
//! hanging notes.
//!
//! All timestamps live on one monotonic clock, captured as an [`Instant`]
//! when the engine starts: the callback stamps incoming packets with the
//! elapsed nanoseconds, time-based effects add deltas, and an event's
//! `time` is the moment the scheduler sends it. The pieces:
//!
//! - The MIDI callback (backend thread): timestamp the packet, push it
//!   into a lock-free SPSC input ring, unpark the graph thread.
//! - The graph thread (owns the [`Pipeline`]): drain the input ring,
//!   apply graph swaps (even while idle), and advance free-running
//!   effects every few milliseconds, emitting into a second SPSC ring.
//!   Owns the hot-reload graph generations so a swapped-out graph keeps
//!   draining its held notes.
//! - The scheduler thread (owns the output and the note tracker): fed by
//!   the graph thread's ring, woken by unpark.
//!
//! Both realtime threads are promoted to realtime priority when the OS
//! allows it. Around them:
//!
//! - [`EngineHandle`]: the cold-path control surface a UI or web remote
//!   drives, switching between named [`SceneDef`] graphs live, with a
//!   panic button and a best-effort tap of every sent event.
//! - [`Engine`]: the wiring, plus optional config-file watching that
//!   re-parses the scenes and swaps the active scene's graph without
//!   interrupting playing.

mod graph;
mod handle;
mod pipeline;
mod reload;
mod scheduler;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle, Thread};
use std::time::Instant;

use audio_thread_priority::promote_current_thread_to_real_time;
use miditool_io::{Input, IoError, OutputTarget};
use thiserror::Error;

pub use handle::{BuildScene, EngineHandle, ReloadScenes, SceneDef};
pub use pipeline::{MAX_DRAINING, Pipeline};

use graph::{Feeder, INPUT_RING_CAPACITY, InMsg, graph_loop};
use handle::SceneState;
use scheduler::{Control, RING_CAPACITY, TAP_CAPACITY, Tap, now_ns, scheduler_loop};

/// Errors from engine setup and teardown. The per-event path reports
/// nothing: a failed send mid-stream has no one to tell, though the first
/// such error surfaces from [`Engine::stop`].
#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Io(#[from] IoError),
    #[error("config watcher: {0}")]
    Watch(#[from] notify::Error),
    #[error("could not spawn the scheduler thread: {0}")]
    Spawn(std::io::Error),
    #[error("the scheduler thread panicked")]
    SchedulerPanicked,
    #[error("the graph thread panicked")]
    GraphPanicked,
    #[error("scene setup: {0}")]
    Scene(String),
}

/// State owned by the MIDI input callback thread: the producer side of
/// the input ring and the graph thread to unpark. The pipeline itself
/// lives on the graph thread.
struct CallbackState {
    input: rtrb::Producer<InMsg>,
    dropped: Arc<AtomicU64>,
    graph: Thread,
}

/// A running engine: one input port, one graph thread owning the
/// pipeline, one scheduler thread owning the output.
///
/// Construct with [`Engine::run`]; stop cleanly with [`Engine::stop`].
/// Dropping a running engine performs the same flush-and-silence sequence,
/// ignoring errors.
pub struct Engine {
    input: Option<Input<CallbackState>>,
    graph: Option<JoinHandle<()>>,
    scheduler: Option<JoinHandle<Option<IoError>>>,
    controls: mpsc::Sender<Control>,
    stop: Arc<AtomicBool>,
    _watcher: Option<reload::Watcher>,
}

impl Engine {
    /// Open the output, start the scheduler and graph threads, build a
    /// pipeline around scene 0's graph, and connect it to the chosen input
    /// port. Processing starts immediately.
    ///
    /// `input` selects the source port as in [`miditool_io::open_input`]:
    /// a case-insensitive substring, or `None` to auto-pick. `scenes`
    /// must be non-empty and names the graphs `build` can produce; the
    /// returned [`EngineHandle`] switches between them live.
    ///
    /// With `reload` set to `Some((config_path, reload_scenes))`, the
    /// config file is watched and each change re-parses it and rebuilds
    /// the active scene's graph off the hot path (carried across the edit
    /// by name, falling back to scene 0), swapping it in on the graph
    /// thread within one tick, whether or not any MIDI is arriving. Reload
    /// errors go to stderr and leave the running scenes in place: a
    /// broken edit must never kill a performance.
    pub fn run(
        input: Option<&str>,
        output: &OutputTarget,
        scenes: Vec<SceneDef>,
        build: BuildScene,
        reload: Option<(PathBuf, ReloadScenes)>,
    ) -> Result<(Engine, EngineHandle), EngineError> {
        if scenes.is_empty() {
            return Err(EngineError::Scene("no scenes defined".into()));
        }
        let root = build(0).map_err(EngineError::Scene)?;
        let build: Arc<BuildScene> = Arc::new(build);

        let mut out = miditool_io::open_output(output)?;
        let epoch = Instant::now();

        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(RING_CAPACITY);
        let (control_tx, control_rx) = mpsc::channel();
        // The tap pair exists whether or not anyone ever listens; the
        // producer side stays a single predictable branch until
        // [`EngineHandle::take_tap`] flips it live.
        let (tap_tx, tap_rx) = rtrb::RingBuffer::new(TAP_CAPACITY);
        let tap_enabled = Arc::new(AtomicBool::new(false));
        let tap = Tap {
            ring: tap_tx,
            enabled: Arc::clone(&tap_enabled),
        };
        let scheduler = thread::Builder::new()
            .name("miditool scheduler".into())
            .spawn(move || {
                // Best effort: an unprivileged scheduler still works, just
                // with coarser wakeups under load. 512 frames at 48 kHz is
                // a plausible period for an event thread.
                if let Err(e) = promote_current_thread_to_real_time(512, 48_000) {
                    eprintln!("miditool: scheduler thread runs without realtime priority: {e:?}");
                }
                let mut first_err: Option<IoError> = None;
                scheduler_loop(epoch, ring_rx, control_rx, tap, &mut |bytes| {
                    if let Err(e) = out.send(bytes) {
                        first_err.get_or_insert(e);
                    }
                });
                first_err
            })
            .map_err(EngineError::Spawn)?;

        let (graph_tx, graph_rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(SceneState {
            defs: scenes,
            active: 0,
        }));
        let watcher = match reload {
            Some((path, reload_scenes)) => {
                let watch = reload::watch(
                    path,
                    reload_scenes,
                    Arc::clone(&build),
                    Arc::clone(&shared),
                    graph_tx.clone(),
                );
                match watch {
                    Ok(w) => Some(w),
                    Err(e) => {
                        abort_scheduler(&control_tx, scheduler);
                        return Err(e.into());
                    }
                }
            }
            None => None,
        };

        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let (input_tx, input_rx) = rtrb::RingBuffer::new(INPUT_RING_CAPACITY);
        let feeder = Feeder {
            ring: ring_tx,
            seq: 0,
            dropped: Arc::clone(&dropped),
            scheduler: scheduler.thread().clone(),
        };
        let pipeline = Pipeline::new(root);
        let graph_stop = Arc::clone(&stop);
        let graph = thread::Builder::new()
            .name("miditool graph".into())
            .spawn(move || {
                // Same best-effort promotion as the scheduler: the graph
                // thread sits between the callback and the send loop, so
                // it deserves the same wakeup priority.
                if let Err(e) = promote_current_thread_to_real_time(512, 48_000) {
                    eprintln!("miditool: graph thread runs without realtime priority: {e:?}");
                }
                graph_loop(epoch, pipeline, input_rx, graph_rx, feeder, graph_stop);
            });
        let graph = match graph {
            Ok(g) => g,
            Err(e) => {
                abort_scheduler(&control_tx, scheduler);
                return Err(EngineError::Spawn(e));
            }
        };

        let state = CallbackState {
            input: input_tx,
            dropped: Arc::clone(&dropped),
            graph: graph.thread().clone(),
        };
        let flag = Arc::clone(&stop);
        let input = miditool_io::open_input_with(
            input,
            move |_stamp, bytes, state: &mut CallbackState| {
                if flag.load(Ordering::Relaxed) || bytes.is_empty() {
                    return;
                }
                // Timestamp and forward; all decoding and graph work
                // happens on the graph thread. [`InMsg::new`] copies short
                // packets inline and boxes only SysEx-length ones, the
                // documented hot-path allocation exception.
                let msg = InMsg::new(now_ns(epoch), bytes);
                if state.input.push(msg).is_err() {
                    state.dropped.fetch_add(1, Ordering::Relaxed);
                }
                state.graph.unpark();
            },
            state,
        );
        let input = match input {
            Ok(input) => input,
            Err(e) => {
                abort_graph(&stop, graph);
                abort_scheduler(&control_tx, scheduler);
                return Err(e.into());
            }
        };
        let handle = EngineHandle {
            scenes: shared,
            build,
            controls: control_tx.clone(),
            graphs: graph_tx,
            graph: graph.thread().clone(),
            scheduler: scheduler.thread().clone(),
            dropped,
            tap: Arc::new(Mutex::new(Some(tap_rx))),
            tap_enabled,
        };
        let engine = Engine {
            input: Some(input),
            graph: Some(graph),
            scheduler: Some(scheduler),
            controls: control_tx,
            stop,
            _watcher: watcher,
        };
        Ok((engine, handle))
    }

    /// Stop processing, flush all effects, and silence hanging notes.
    pub fn stop(mut self) -> Result<(), EngineError> {
        self.wind_down()
    }

    /// Shared teardown for [`Engine::stop`] and `Drop`. Idempotent.
    ///
    /// Ordering: stop the watcher so no swap arrives mid-teardown, raise
    /// the stop flag, disconnect the input (`close` blocks until the
    /// callback cannot run again, making the input ring's producer side
    /// dead), then join the graph thread, which drains whatever the
    /// callback left in the ring, flushes the pipeline into the
    /// scheduler's ring, and exits. Only then is the scheduler told to
    /// shut down, so the flush's note-offs are already queued when it
    /// takes its final drain.
    fn wind_down(&mut self) -> Result<(), EngineError> {
        let Some(input) = self.input.take() else {
            return Ok(());
        };
        self._watcher = None;
        self.stop.store(true, Ordering::Release);
        drop(input.close());
        let graph_panicked = match self.graph.take() {
            Some(handle) => {
                handle.thread().unpark();
                handle.join().is_err()
            }
            None => false,
        };
        let _ = self.controls.send(Control::Shutdown);
        let Some(handle) = self.scheduler.take() else {
            return Ok(());
        };
        handle.thread().unpark();
        match handle.join() {
            Ok(Some(e)) => Err(e.into()),
            Ok(None) if graph_panicked => Err(EngineError::GraphPanicked),
            Ok(None) => Ok(()),
            Err(_) => Err(EngineError::SchedulerPanicked),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.wind_down();
    }
}

/// Tear down a graph thread that never got an engine around it.
fn abort_graph(stop: &Arc<AtomicBool>, handle: JoinHandle<()>) {
    stop.store(true, Ordering::Release);
    handle.thread().unpark();
    let _ = handle.join();
}

/// Tear down a scheduler thread that never got an engine around it.
fn abort_scheduler(controls: &mpsc::Sender<Control>, handle: JoinHandle<Option<IoError>>) {
    let _ = controls.send(Control::Shutdown);
    handle.thread().unpark();
    let _ = handle.join();
}
