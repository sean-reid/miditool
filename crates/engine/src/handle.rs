//! Scenes and the live control surface: the shared scene table and the
//! cloneable [`EngineHandle`] a remote UI drives.
//!
//! A scene is a named effect graph; the config declares several and the
//! player switches between them mid-performance. The handle owns every
//! cold-path control: building and swapping scene graphs, panic, the
//! drop counter, and the sent-event tap. Scene state itself lives in an
//! `Arc<Mutex<...>>` shared with the config watcher, which is fine
//! because nothing on the realtime threads ever touches it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread::Thread;

use miditool_core::{Event, Node};

use crate::scheduler::Control;

/// One named scene from the config: an effect graph the player switches
/// to live.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDef {
    /// The scene's name as written in the config. Hot reload matches the
    /// active scene across edits by this name.
    pub name: String,
    /// Silence sounding notes and drop pending scheduled events the
    /// moment the player switches away from this scene, instead of
    /// letting them ring and drain.
    pub kill_on_exit: bool,
}

/// Builds the graph for scene `idx` from the current config specs.
pub type BuildScene = Box<dyn Fn(usize) -> Result<Node, String> + Send + Sync>;

/// Re-parses the config file and refreshes whatever store [`BuildScene`]
/// reads; returns the new scene list.
pub type ReloadScenes = Box<dyn Fn() -> Result<Vec<SceneDef>, String> + Send>;

/// The scene table shared by the handle and the config watcher. `active`
/// always indexes into `defs`; both mutate together under the lock.
pub(crate) struct SceneState {
    pub(crate) defs: Vec<SceneDef>,
    pub(crate) active: usize,
}

/// Lock a cold-path mutex, shrugging off poison: scene state is plain
/// data and stays valid whether or not a panicking thread finished its
/// update, and a control surface must outlive a broken build closure.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A cheap-to-clone, thread-safe remote control for a running
/// [`Engine`](crate::Engine): scene switching, panic, diagnostics, and
/// the sent-event tap. Every method is cold-path; none of them block the
/// realtime threads.
#[derive(Clone)]
pub struct EngineHandle {
    pub(crate) scenes: Arc<Mutex<SceneState>>,
    pub(crate) build: Arc<BuildScene>,
    pub(crate) controls: mpsc::Sender<Control>,
    pub(crate) graphs: mpsc::Sender<Node>,
    pub(crate) graph: Thread,
    pub(crate) scheduler: Thread,
    pub(crate) dropped: Arc<AtomicU64>,
    pub(crate) tap: Arc<Mutex<Option<rtrb::Consumer<Event>>>>,
    pub(crate) tap_enabled: Arc<AtomicBool>,
}

impl EngineHandle {
    /// The scene table as of the last successful load or reload.
    pub fn scenes(&self) -> Vec<SceneDef> {
        lock(&self.scenes).defs.clone()
    }

    /// Index of the active scene.
    pub fn active(&self) -> usize {
        lock(&self.scenes).active
    }

    /// Build scene `idx` and swap it in.
    ///
    /// The graph is built here, on the caller's thread, then handed to
    /// the graph thread over the same channel hot reload uses. The swap
    /// applies promptly, even while the player is idle (the graph thread
    /// is unparked and installs it before the next packet or tick), and
    /// notes held through the outgoing graph keep draining through it. If
    /// the outgoing scene has [`SceneDef::kill_on_exit`], the scheduler
    /// additionally silences everything sounding and drops all pending
    /// scheduled events, immediately.
    ///
    /// Fails on an out-of-range index, a build error, or a stopped
    /// engine; the active scene is unchanged in every failure case.
    pub fn set_scene(&self, idx: usize) -> Result<(), String> {
        let mut state = lock(&self.scenes);
        if idx >= state.defs.len() {
            return Err(format!(
                "scene index {idx} out of range ({} scenes)",
                state.defs.len()
            ));
        }
        let root = (self.build)(idx)?;
        if state.defs.get(state.active).is_some_and(|d| d.kill_on_exit) {
            let _ = self.controls.send(Control::Silence);
            self.scheduler.unpark();
        }
        self.graphs
            .send(root)
            .map_err(|_| "the engine is not running".to_string())?;
        self.graph.unpark();
        state.active = idx;
        Ok(())
    }

    /// Emergency stop: silence everything sounding, drop everything
    /// pending, and sweep All Notes Off, All Sound Off, and Reset All
    /// Controllers across all 16 channels. The engine keeps running.
    pub fn panic(&self) {
        let _ = self.controls.send(Control::Panic);
        self.scheduler.unpark();
    }

    /// Events dropped because a realtime ring was full: input packets the
    /// graph thread had no room for, or output events the scheduler's
    /// ring rejected. Diagnostic; anything above zero means sustained
    /// overload.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The sent-event tap: every channel event the scheduler sends,
    /// stamped with its send time, mirrored into a fixed-size ring. Best
    /// effort by design: a slow consumer loses events rather than stall
    /// the sender, and raw or SysEx passthrough bytes never appear. At
    /// most one consumer exists across all clones of the handle; every
    /// call after the first returns `None`.
    pub fn take_tap(&mut self) -> Option<rtrb::Consumer<Event>> {
        let taken = lock(&self.tap).take();
        if taken.is_some() {
            self.tap_enabled.store(true, Ordering::Relaxed);
        }
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread;
    use std::time::{Duration, Instant};

    use miditool_core::graph::{Effect, Pass};
    use miditool_core::{EventBuf, EventKind, ProcCx, Timestamp};

    use crate::graph::{Feeder, InMsg, graph_loop};
    use crate::pipeline::Pipeline;
    use crate::scheduler::{Tap, now_ns, scheduler_loop};

    const MS: Timestamp = 1_000_000;

    fn def(name: &str, kill: bool) -> SceneDef {
        SceneDef {
            name: name.into(),
            kill_on_exit: kill,
        }
    }

    /// Shifts note keys by a fixed offset; everything else passes.
    struct Shift(u8);

    impl Effect for Shift {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, _cx: &ProcCx) {
            let kind = match ev.kind {
                EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                    ch,
                    key: key + self.0,
                    vel,
                },
                EventKind::NoteOff { ch, key, vel } => EventKind::NoteOff {
                    ch,
                    key: key + self.0,
                    vel,
                },
                other => other,
            };
            out.push(Event::new(ev.time, kind));
        }
    }

    /// Emits each note-on immediately plus its note-off scheduled `.0`
    /// nanoseconds later, like a one-shot delay line.
    struct AutoOff(Timestamp);

    impl Effect for AutoOff {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, _cx: &ProcCx) {
            if let EventKind::NoteOn { ch, key, .. } = ev.kind {
                out.push(*ev);
                out.push(Event::new(
                    ev.time + self.0,
                    EventKind::NoteOff { ch, key, vel: 0 },
                ));
            }
        }
    }

    /// A handle wired to a real graph thread and a real scheduler thread:
    /// everything the engine wires up except midir. `pump` stands in for
    /// the input callback.
    struct Rig {
        epoch: Instant,
        input: rtrb::Producer<InMsg>,
        stop: Arc<AtomicBool>,
        graph: thread::JoinHandle<()>,
        ctl: mpsc::Sender<Control>,
        out: mpsc::Receiver<(Timestamp, Vec<u8>)>,
        sched: thread::JoinHandle<()>,
        handle: EngineHandle,
    }

    fn rig(defs: Vec<SceneDef>, build: BuildScene) -> Rig {
        let epoch = Instant::now();
        let (input_tx, input_rx) = rtrb::RingBuffer::new(64);
        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(64);
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let (tap_tx, tap_rx) = rtrb::RingBuffer::new(16);
        let tap_enabled = Arc::new(AtomicBool::new(false));
        let tap = Tap {
            ring: tap_tx,
            enabled: Arc::clone(&tap_enabled),
        };
        let sched = thread::spawn(move || {
            scheduler_loop(epoch, ring_rx, ctl_rx, tap, &mut |b| {
                out_tx.send((now_ns(epoch), b.to_vec())).unwrap();
            });
        });
        let root = build(0).expect("scene 0 builds");
        let (graph_tx, graph_rx) = mpsc::channel();
        let dropped = Arc::new(AtomicU64::new(0));
        let feeder = Feeder {
            ring: ring_tx,
            seq: 0,
            dropped: Arc::clone(&dropped),
            scheduler: sched.thread().clone(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let graph = thread::spawn(move || {
            graph_loop(epoch, Pipeline::new(root), input_rx, graph_rx, feeder, flag);
        });
        let handle = EngineHandle {
            scenes: Arc::new(Mutex::new(SceneState { defs, active: 0 })),
            build: Arc::new(build),
            controls: ctl_tx.clone(),
            graphs: graph_tx,
            graph: graph.thread().clone(),
            scheduler: sched.thread().clone(),
            dropped,
            tap: Arc::new(Mutex::new(Some(tap_rx))),
            tap_enabled,
        };
        Rig {
            epoch,
            input: input_tx,
            stop,
            graph,
            ctl: ctl_tx,
            out: out_rx,
            sched,
            handle,
        }
    }

    impl Rig {
        /// Mirror of the engine's midir callback: stamp the packet, push
        /// it into the input ring, unpark the graph thread. Returns the
        /// timestamp used.
        fn pump(&mut self, bytes: &[u8]) -> Timestamp {
            let now = now_ns(self.epoch);
            self.input.push(InMsg::new(now, bytes)).unwrap();
            self.graph.thread().unpark();
            now
        }

        /// Poll the output until `n` sends arrive or a generous deadline
        /// passes; fixed sleeps flake on loaded CI runners.
        fn wait_sends(&self, n: usize) -> Vec<(Timestamp, Vec<u8>)> {
            let mut got = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(5);
            while got.len() < n && Instant::now() < deadline {
                got.extend(self.out.try_iter());
                thread::sleep(Duration::from_millis(1));
            }
            got
        }

        /// Mirror of the engine's wind-down: stop flag, input closed,
        /// graph thread joined, then scheduler shutdown. Returns whatever
        /// else was sent.
        fn finish(self) -> Vec<(Timestamp, Vec<u8>)> {
            self.stop.store(true, Ordering::Release);
            drop(self.input);
            self.graph.thread().unpark();
            self.graph.join().unwrap();
            self.ctl.send(Control::Shutdown).unwrap();
            self.sched.thread().unpark();
            self.sched.join().unwrap();
            self.out.try_iter().collect()
        }
    }

    fn bytes_of(sent: &[(Timestamp, Vec<u8>)]) -> Vec<Vec<u8>> {
        sent.iter().map(|(_, b)| b.clone()).collect()
    }

    #[test]
    fn handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EngineHandle>();
    }

    #[test]
    fn set_scene_swaps_the_mapping_and_drains_held_notes() {
        let build: BuildScene = Box::new(|i| Ok(Node::Leaf(Box::new(Shift([2, 5][i])))));
        let mut rig = rig(vec![def("a", false), def("b", false)], build);
        assert_eq!(rig.handle.scenes(), vec![def("a", false), def("b", false)]);

        rig.pump(&[0x90, 60, 100]);
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x90, 62, 100]]);

        rig.handle.set_scene(1).unwrap();
        assert_eq!(rig.handle.active(), 1);
        // The swap precedes any packet pushed after it: a fresh note maps
        // through scene 1 while the held note's off still routes through
        // scene 0.
        rig.pump(&[0x90, 61, 100]);
        rig.pump(&[0x80, 60, 0]);
        assert_eq!(
            bytes_of(&rig.wait_sends(2)),
            vec![vec![0x90, 66, 100], vec![0x80, 62, 0]]
        );
        // Shutdown silences the one note scene 1 left sounding.
        assert_eq!(bytes_of(&rig.finish()), vec![vec![0x80, 66, 0]]);
    }

    /// Passes everything; on flush emits a marker note-off for key `id`
    /// on channel 15, making swaps observable through the output alone.
    /// A note-off rather than a CC because the scheduler's shutdown
    /// forwards pending note-offs and drops everything else.
    struct Marker(u8);

    impl Effect for Marker {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, _cx: &ProcCx) {
            out.push(*ev);
        }

        fn flush(&mut self, out: &mut EventBuf, _cx: &ProcCx) {
            out.push(Event::new(
                0,
                EventKind::NoteOff {
                    ch: 15,
                    key: self.0,
                    vel: 0,
                },
            ));
        }
    }

    #[test]
    fn set_scene_applies_while_the_player_is_idle() {
        let build: BuildScene = Box::new(|i| Ok(Node::Leaf(Box::new(Marker(i as u8 + 1)))));
        let rig = rig(vec![def("a", false), def("b", false)], build);
        // No MIDI traffic at all. The outgoing scene 0 graph is idle, so
        // the swap flushes it on the spot; its marker reaching the output
        // proves the swap applied without waiting for an input event.
        rig.handle.set_scene(1).unwrap();
        let sent = rig.wait_sends(1);
        assert_eq!(bytes_of(&sent), vec![vec![0x8F, 1, 0]]);
        // Shutdown flushes the graph now current: scene 1's.
        assert_eq!(bytes_of(&rig.finish()), vec![vec![0x8F, 2, 0]]);
    }

    #[test]
    fn switching_away_from_a_kill_scene_silences_and_drops_pending() {
        let build: BuildScene = Box::new(|i| {
            Ok(match i {
                0 => Node::Leaf(Box::new(AutoOff(60_000 * MS))),
                _ => Node::Leaf(Box::new(Pass)),
            })
        });
        let mut rig = rig(vec![def("kill", true), def("plain", false)], build);
        rig.pump(&[0x90, 60, 100]);
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x90, 60, 100]]);

        // Switching away silences the sounding note immediately. The far
        // future note-off pending in the heap is dropped: shutdown, which
        // sends any pending note-off it finds, has nothing left to say.
        rig.handle.set_scene(1).unwrap();
        assert_eq!(rig.handle.active(), 1);
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x80, 60, 0]]);
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    #[test]
    fn switching_away_from_a_plain_scene_lets_pending_events_ring() {
        let build: BuildScene = Box::new(|i| {
            Ok(match i {
                0 => Node::Leaf(Box::new(AutoOff(100 * MS))),
                _ => Node::Leaf(Box::new(Pass)),
            })
        });
        let mut rig = rig(vec![def("ring", false), def("plain", false)], build);
        let t = rig.pump(&[0x90, 60, 100]);
        // Wait for the note-on so the packet is known to have mapped
        // through scene 0 before the switch; a pending swap installs
        // ahead of any packet still in the input ring.
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x90, 60, 100]]);
        rig.handle.set_scene(1).unwrap();
        // No kill: the scheduled note-off fires at its own deadline.
        let sent = rig.wait_sends(1);
        assert_eq!(bytes_of(&sent), vec![vec![0x80, 60, 0]]);
        // Never early; lateness is host scheduling noise and stays
        // unasserted.
        let at = sent[0].0;
        assert!(at + MS >= t + 100 * MS, "pending note-off sent early");
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    #[test]
    fn set_scene_rejects_an_out_of_range_index() {
        let build: BuildScene = Box::new(|_| Ok(Node::Leaf(Box::new(Pass))));
        let rig = rig(vec![def("only", false)], build);
        assert!(rig.handle.set_scene(1).is_err());
        assert_eq!(rig.handle.active(), 0);
        rig.finish();
    }

    #[test]
    fn a_failed_scene_build_leaves_the_active_scene_alone() {
        let build: BuildScene = Box::new(|i| match i {
            0 => Ok(Node::Leaf(Box::new(Pass))),
            _ => Err("bad scene".into()),
        });
        let rig = rig(vec![def("a", false), def("b", false)], build);
        assert_eq!(rig.handle.set_scene(1), Err("bad scene".to_string()));
        assert_eq!(rig.handle.active(), 0);
        rig.finish();
    }

    #[test]
    fn take_tap_hands_out_the_consumer_exactly_once() {
        let build: BuildScene = Box::new(|_| Ok(Node::Leaf(Box::new(Pass))));
        let rig = rig(vec![def("only", false)], build);
        let mut first = rig.handle.clone();
        let mut second = rig.handle.clone();
        assert!(first.take_tap().is_some());
        assert!(second.take_tap().is_none());
        rig.finish();
    }
}
