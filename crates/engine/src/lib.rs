//! The realtime run loop: decode incoming packets, run the effect graph,
//! and hand the results to a dedicated scheduler thread that sends each
//! event at its intended time, tracking what is sounding so shutdown
//! leaves no hanging notes.
//!
//! All timestamps live on one monotonic clock, captured as an [`Instant`]
//! when the engine starts: the callback stamps incoming events with the
//! elapsed nanoseconds, time-based effects add deltas, and an event's
//! `time` is the moment the scheduler sends it. The pieces:
//!
//! - [`Pipeline`] (pure, hot path): decode and route through the graph,
//!   emitting possibly future-timed events. Owns the hot-reload graph
//!   generations so a swapped-out graph keeps draining its held notes.
//! - The scheduler thread (owns the output and the note tracker): fed by
//!   a lock-free SPSC ring, woken by unpark, promoted to realtime
//!   priority when the OS allows it.
//! - [`EngineHandle`]: the cold-path control surface a UI or web remote
//!   drives, switching between named [`SceneDef`] graphs live, with a
//!   panic button and a best-effort tap of every sent event.
//! - [`Engine`]: the wiring, plus optional config-file watching that
//!   re-parses the scenes and swaps the active scene's graph without
//!   interrupting playing.

mod handle;
mod pipeline;
mod reload;
mod scheduler;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle, Thread};
use std::time::Instant;

use audio_thread_priority::promote_current_thread_to_real_time;
use miditool_core::{Event, Node};
use miditool_io::{Input, IoError, OutputTarget};
use thiserror::Error;

pub use handle::{BuildScene, EngineHandle, ReloadScenes, SceneDef};
pub use pipeline::{MAX_DRAINING, Pipeline};

use handle::SceneState;
use scheduler::{Control, Msg, RING_CAPACITY, TAP_CAPACITY, Tap, now_ns, scheduler_loop};

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
    #[error("scene setup: {0}")]
    Scene(String),
}

/// The callback's single-producer handle to the scheduler: the ring, the
/// running sequence counter, and the thread to unpark.
struct Feeder {
    ring: rtrb::Producer<Msg>,
    seq: u64,
    dropped: Arc<AtomicU64>,
    scheduler: Thread,
}

impl Feeder {
    fn event(&mut self, ev: Event) {
        let seq = self.seq;
        self.seq += 1;
        if self.ring.push(Msg::Event { seq, ev }).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn raw(&mut self, bytes: &[u8]) {
        let msg = if bytes.len() <= 3 {
            let mut buf = [0u8; 3];
            buf[..bytes.len()].copy_from_slice(bytes);
            Msg::Raw {
                len: bytes.len() as u8,
                bytes: buf,
            }
        } else {
            // The documented hot-path allocation exception: SysEx only.
            Msg::Sysex(bytes.into())
        };
        if self.ring.push(msg).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn wake(&self) {
        self.scheduler.unpark();
    }
}

/// State owned by the MIDI input callback thread. The feeder sits in a
/// `RefCell` because `Pipeline::handle` takes two sink closures that both
/// need it; they never overlap, so the borrow always succeeds.
struct CallbackState {
    pipeline: Pipeline,
    feeder: RefCell<Feeder>,
    graphs: mpsc::Receiver<Node>,
}

/// A running engine: one input port, one pipeline, one scheduler thread
/// owning the output.
///
/// Construct with [`Engine::run`]; stop cleanly with [`Engine::stop`].
/// Dropping a running engine performs the same flush-and-silence sequence,
/// ignoring errors.
pub struct Engine {
    input: Option<Input<CallbackState>>,
    scheduler: Option<JoinHandle<Option<IoError>>>,
    controls: mpsc::Sender<Control>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
    _watcher: Option<reload::Watcher>,
}

impl Engine {
    /// Open the output, start the scheduler thread, build a pipeline
    /// around scene 0's graph, and connect it to the chosen input port.
    /// Processing starts immediately on the backend's MIDI thread.
    ///
    /// `input` selects the source port as in [`miditool_io::open_input`]:
    /// a case-insensitive substring, or `None` to auto-pick. `scenes`
    /// must be non-empty and names the graphs `build` can produce; the
    /// returned [`EngineHandle`] switches between them live.
    ///
    /// With `reload` set to `Some((config_path, reload_scenes))`, the
    /// config file is watched and each change re-parses it and rebuilds
    /// the active scene's graph off the hot path (carried across the edit
    /// by name, falling back to scene 0), swapping it in on the next
    /// incoming MIDI event (an idle rig applies the swap when the next
    /// event arrives, which is exactly when it can first matter). Reload
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
        let state = CallbackState {
            pipeline: Pipeline::new(root),
            feeder: RefCell::new(Feeder {
                ring: ring_tx,
                seq: 0,
                dropped: Arc::clone(&dropped),
                scheduler: scheduler.thread().clone(),
            }),
            graphs: graph_rx,
        };
        let flag = Arc::clone(&stop);
        let input = miditool_io::open_input_with(
            input,
            move |_stamp, bytes, state: &mut CallbackState| {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                let CallbackState {
                    pipeline,
                    feeder,
                    graphs,
                } = state;
                let now = now_ns(epoch);
                // Install any pending reload before processing, so this
                // event is the first the new graph sees. try_recv on an
                // empty channel is one atomic load: hot-path cheap.
                while let Ok(root) = graphs.try_recv() {
                    pipeline.swap_graph(now, root, &mut |ev| feeder.borrow_mut().event(ev));
                }
                pipeline.handle(
                    now,
                    bytes,
                    &mut |ev| feeder.borrow_mut().event(ev),
                    &mut |b| feeder.borrow_mut().raw(b),
                );
                feeder.borrow().wake();
            },
            state,
        );
        let input = match input {
            Ok(input) => input,
            Err(e) => {
                abort_scheduler(&control_tx, scheduler);
                return Err(e.into());
            }
        };
        let handle = EngineHandle {
            scenes: shared,
            build,
            controls: control_tx.clone(),
            graphs: graph_tx,
            scheduler: scheduler.thread().clone(),
            dropped,
            tap: Arc::new(Mutex::new(Some(tap_rx))),
            tap_enabled,
        };
        let engine = Engine {
            input: Some(input),
            scheduler: Some(scheduler),
            controls: control_tx,
            stop,
            epoch,
            _watcher: watcher,
        };
        Ok((engine, handle))
    }

    /// Stop processing, flush all effects, and silence hanging notes.
    pub fn stop(mut self) -> Result<(), EngineError> {
        self.wind_down()
    }

    /// Shared teardown for [`Engine::stop`] and `Drop`. Idempotent.
    fn wind_down(&mut self) -> Result<(), EngineError> {
        let Some(input) = self.input.take() else {
            return Ok(());
        };
        // Stop the watcher first so no swap arrives mid-teardown, stop
        // feeding the pipeline, then disconnect. `close` blocks until the
        // callback cannot run again, making the ring's producer side
        // exclusively ours.
        self._watcher = None;
        self.stop.store(true, Ordering::Relaxed);
        let CallbackState {
            mut pipeline,
            feeder,
            ..
        } = input.close();
        let mut feeder = feeder.into_inner();
        pipeline.shutdown(now_ns(self.epoch), &mut |ev| feeder.event(ev));
        let _ = self.controls.send(Control::Shutdown);
        feeder.wake();
        let Some(handle) = self.scheduler.take() else {
            return Ok(());
        };
        match handle.join() {
            Ok(Some(e)) => Err(e.into()),
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

/// Tear down a scheduler thread that never got an engine around it.
fn abort_scheduler(controls: &mpsc::Sender<Control>, handle: JoinHandle<Option<IoError>>) {
    let _ = controls.send(Control::Shutdown);
    handle.thread().unpark();
    let _ = handle.join();
}
