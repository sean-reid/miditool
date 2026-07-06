//! The live control layer: performance gestures played on reserved keys,
//! and timed "moments" that wander between scenes on their own clock.
//!
//! Gestures: a [`ControlDef`] reserves note keys as controls. On the
//! graph thread a [`GestureFilter`] consumes those note-ons (plus their
//! matching note-offs and any poly-pressure in between) before the
//! effect graph ever sees them, and pushes the decoded gesture over an
//! mpsc channel to a dedicated control thread. That thread owns an
//! internal [`EngineHandle`] clone and does all the cold work, building
//! the target scene's graph through `set_scene` or forwarding a panic,
//! so the graph thread never builds anything. The graph thread's share
//! of a gesture is one array lookup plus one mpsc send, the same class
//! of cost as the existing swap-channel sends.
//!
//! Moments: with a [`MomentsDef`] the control thread also keeps a dwell
//! deadline, waiting on the gesture channel with `recv_timeout`. When
//! the deadline expires it switches to a seeded-random scene different
//! from the active one and draws the next dwell from the configured
//! window; the sequence is deterministic per seed. Any manual scene
//! change, a gesture or a `set_scene` on any handle clone, restarts the
//! countdown without consuming a draw, so the random sequence stays a
//! pure function of the number of automatic switches. Scene switches
//! ride the ordinary `set_scene` path, so each scene's kill or let-ring
//! exit semantics apply unchanged.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use miditool_core::rng::{Prng, seeded};
use miditool_core::{EventKind, Timestamp};
use rand::Rng;

use crate::handle::EngineHandle;

/// Live control configuration: which note keys act as scene gestures,
/// and the optional moments clock. Keys apply on every channel; a key
/// assigned twice keeps its last assignment, in the order next_scene,
/// prev_scene, gotos, panic_key.
#[derive(Debug, Clone)]
pub struct ControlDef {
    /// Key that advances to `(active + 1) % scene_count`.
    pub next_scene: Option<u8>,
    /// Key that steps back to `(active + count - 1) % count`.
    pub prev_scene: Option<u8>,
    /// Keys that jump straight to a scene index. Indices are
    /// caller-validated; an out-of-range jump is rejected by `set_scene`
    /// at gesture time and reported to stderr.
    pub gotos: Vec<(u8, usize)>,
    /// Key that triggers the full panic sweep.
    pub panic_key: Option<u8>,
    /// Automatic scene wandering; `None` leaves switching manual.
    pub moments: Option<MomentsDef>,
}

/// The moments clock: dwell in a scene for a seeded-random time drawn
/// uniformly from `[dwell_lo_ns, dwell_hi_ns]`, then move to a random
/// different scene.
#[derive(Debug, Clone)]
pub struct MomentsDef {
    pub dwell_lo_ns: u64,
    pub dwell_hi_ns: u64,
    pub seed: u64,
}

/// One message to the control thread.
pub(crate) enum ControlMsg {
    /// A control key decoded from the input stream by the graph thread.
    Gesture(Gesture),
    /// Some handle clone switched scenes manually; restart the dwell.
    Sync,
    /// Engine teardown.
    Shutdown,
}

/// A decoded control-key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gesture {
    Next,
    Prev,
    Goto(usize),
    Panic,
}

/// The graph thread's gesture pre-filter: consumes control-key events
/// before the graph. Per decoded event this costs one array lookup, plus
/// one mpsc send when a gesture actually fires (a player pressing a
/// control key is cold by definition; the send is the same class as the
/// swap-channel sends).
pub(crate) struct GestureFilter {
    /// Gesture per key, matched on any channel. 128 slots, no heap.
    actions: [Option<Gesture>; 128],
    /// One bit per (channel, key) note-on we consumed: its note-off and
    /// poly-pressure are consumed too, and nothing else is.
    consumed: [u128; 16],
    tx: mpsc::Sender<ControlMsg>,
}

impl GestureFilter {
    pub(crate) fn new(def: &ControlDef, tx: mpsc::Sender<ControlMsg>) -> Self {
        let mut actions = [None; 128];
        if let Some(key) = def.next_scene {
            actions[key as usize & 127] = Some(Gesture::Next);
        }
        if let Some(key) = def.prev_scene {
            actions[key as usize & 127] = Some(Gesture::Prev);
        }
        for &(key, idx) in &def.gotos {
            actions[key as usize & 127] = Some(Gesture::Goto(idx));
        }
        if let Some(key) = def.panic_key {
            actions[key as usize & 127] = Some(Gesture::Panic);
        }
        Self {
            actions,
            consumed: [0; 16],
            tx,
        }
    }

    /// Offer one decoded event; `true` means it was a control gesture
    /// (or the tail of one) and must not reach the graph.
    ///
    /// A note-on on a reserved key fires its gesture and marks (ch, key)
    /// consumed; the matching note-off clears the mark and is eaten too,
    /// while poly-pressure on a marked key is eaten without clearing it.
    /// Offs of notes we never consumed pass through untouched, so a note
    /// held from before the key was reserved still closes properly. The
    /// wire decoder normalizes velocity-0 note-ons to note-offs, so the
    /// note-on arm never sees a release.
    pub(crate) fn consume(&mut self, kind: &EventKind) -> bool {
        match *kind {
            EventKind::NoteOn { ch, key, .. } => {
                let Some(gesture) = self.actions[key as usize & 127] else {
                    return false;
                };
                self.consumed[ch as usize & 15] |= 1 << (key & 127);
                // A closed channel just means the engine is stopping.
                let _ = self.tx.send(ControlMsg::Gesture(gesture));
                true
            }
            EventKind::NoteOff { ch, key, .. } => {
                let slot = &mut self.consumed[ch as usize & 15];
                let bit = 1u128 << (key & 127);
                let eaten = *slot & bit != 0;
                *slot &= !bit;
                eaten
            }
            EventKind::PolyPressure { ch, key, .. } => {
                self.consumed[ch as usize & 15] & (1 << (key & 127)) != 0
            }
            _ => false,
        }
    }
}

/// The control thread body: apply gestures, run the moments clock, and
/// return on shutdown (explicit message or every sender gone).
pub(crate) fn control_loop(
    handle: EngineHandle,
    gestures: mpsc::Receiver<ControlMsg>,
    moments: Option<MomentsDef>,
) {
    let mut moments = moments.map(Moments::new);
    loop {
        let msg = match &moments {
            Some(m) => match gestures.recv_timeout(m.remaining()) {
                Ok(msg) => Some(msg),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match gestures.recv() {
                Ok(msg) => Some(msg),
                Err(_) => return,
            },
        };
        match msg {
            // The dwell expired: an automatic switch.
            None => {
                if let Some(m) = &mut moments {
                    m.fire(&handle);
                }
            }
            Some(ControlMsg::Shutdown) => return,
            Some(ControlMsg::Sync) => {
                if let Some(m) = &mut moments {
                    m.rewind();
                }
            }
            Some(ControlMsg::Gesture(gesture)) => {
                if apply_gesture(&handle, gesture)
                    && let Some(m) = &mut moments
                {
                    m.rewind();
                }
            }
        }
    }
}

/// Apply one gesture through the internal handle, returning whether the
/// active scene changed. A gesture targeting the scene already active is
/// a no-op: nothing is built and nothing is swapped, so no drain or
/// flush artifacts appear. The active/count read and the `set_scene`
/// call are two lock takes; a reload slipping between them can at worst
/// redirect one gesture, which `set_scene` still validates.
fn apply_gesture(handle: &EngineHandle, gesture: Gesture) -> bool {
    let target = match gesture {
        Gesture::Panic => {
            handle.panic();
            return false;
        }
        _ => {
            let (active, count) = handle.scene_cursor();
            if count == 0 {
                return false;
            }
            let target = match gesture {
                Gesture::Next => (active + 1) % count,
                Gesture::Prev => (active + count - 1) % count,
                Gesture::Goto(idx) => idx,
                Gesture::Panic => unreachable!("handled above"),
            };
            if target == active {
                return false;
            }
            target
        }
    };
    match handle.set_scene(target) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("miditool: control gesture ignored: {e}");
            false
        }
    }
}

/// The moments clock's bookkeeping: the seeded RNG, the dwell window,
/// and the current countdown.
struct Moments {
    rng: Prng,
    lo: Timestamp,
    hi: Timestamp,
    /// The dwell currently counting down, kept so a manual change can
    /// restart the countdown without consuming a draw.
    dwell: Duration,
    deadline: Instant,
}

impl Moments {
    fn new(def: MomentsDef) -> Self {
        let mut rng = seeded(def.seed, 0);
        let lo = def.dwell_lo_ns.min(def.dwell_hi_ns);
        let hi = def.dwell_lo_ns.max(def.dwell_hi_ns);
        let dwell = draw(&mut rng, lo, hi);
        Self {
            rng,
            lo,
            hi,
            dwell,
            deadline: Instant::now() + dwell,
        }
    }

    /// Time left in the current dwell; zero once it has expired.
    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// A manual scene change happened: restart the countdown with the
    /// dwell already drawn. No draw is consumed, so the automatic
    /// sequence stays deterministic across manual interference.
    fn rewind(&mut self) {
        self.deadline = Instant::now() + self.dwell;
    }

    /// Pick the next scene: uniform over every index except `active`.
    /// `None` when there is nowhere else to go.
    fn pick(&mut self, active: usize, count: usize) -> Option<usize> {
        if count < 2 {
            return None;
        }
        let r = self.rng.random_range(0..count - 1);
        Some(if r >= active { r + 1 } else { r })
    }

    /// The dwell expired: switch to a random different scene and start
    /// the next countdown. A failed switch (a scene whose build broke
    /// under reload) is reported and skipped; the clock keeps running.
    fn fire(&mut self, handle: &EngineHandle) {
        let (active, count) = handle.scene_cursor();
        if let Some(target) = self.pick(active, count)
            && let Err(e) = handle.set_scene(target)
        {
            eprintln!("miditool: moment switch failed: {e}");
        }
        self.dwell = draw(&mut self.rng, self.lo, self.hi);
        self.deadline = Instant::now() + self.dwell;
    }
}

/// One dwell, uniform in `[lo, hi]` nanoseconds.
fn draw(rng: &mut Prng, lo: Timestamp, hi: Timestamp) -> Duration {
    Duration::from_nanos(rng.random_range(lo..=hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use miditool_core::graph::{Effect, Pass};
    use miditool_core::{Event, EventBuf, Node, ProcCx};

    use crate::EngineHandle;
    use crate::graph::{Feeder, InMsg, graph_loop};
    use crate::handle::{BuildScene, SceneDef, SceneState};
    use crate::pipeline::Pipeline;
    use crate::scheduler::{Control, Tap, now_ns, scheduler_loop};

    const MS: u64 = 1_000_000;

    fn scene(name: &str) -> SceneDef {
        SceneDef {
            name: name.into(),
            kill_on_exit: false,
        }
    }

    fn def_with(f: impl FnOnce(&mut ControlDef)) -> ControlDef {
        let mut def = ControlDef {
            next_scene: None,
            prev_scene: None,
            gotos: Vec::new(),
            panic_key: None,
            moments: None,
        };
        f(&mut def);
        def
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

    /// Everything the engine wires up when a [`ControlDef`] is present,
    /// except midir: a real graph thread with the gesture filter, a real
    /// scheduler thread, a real control thread, and an [`EngineHandle`]
    /// whose manual switches notify the control thread.
    struct Rig {
        epoch: Instant,
        input: rtrb::Producer<InMsg>,
        stop: Arc<AtomicBool>,
        graph: thread::JoinHandle<()>,
        ctl: mpsc::Sender<Control>,
        out: mpsc::Receiver<(Timestamp, Vec<u8>)>,
        sched: thread::JoinHandle<()>,
        handle: EngineHandle,
        gestures: mpsc::Sender<ControlMsg>,
        control: thread::JoinHandle<()>,
    }

    fn rig(defs: Vec<SceneDef>, build: BuildScene, def: ControlDef) -> Rig {
        let epoch = Instant::now();
        let (input_tx, input_rx) = rtrb::RingBuffer::new(64);
        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(256);
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
        let (gesture_tx, gesture_rx) = mpsc::channel();
        let filter = GestureFilter::new(&def, gesture_tx.clone());
        let moments = def.moments.clone();
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
            graph_loop(
                epoch,
                Pipeline::new(root),
                input_rx,
                graph_rx,
                feeder,
                flag,
                Some(filter),
            );
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
            control: Some(gesture_tx.clone()),
        };
        let mut internal = handle.clone();
        internal.control = None;
        let control = thread::spawn(move || control_loop(internal, gesture_rx, moments));
        Rig {
            epoch,
            input: input_tx,
            stop,
            graph,
            ctl: ctl_tx,
            out: out_rx,
            sched,
            handle,
            gestures: gesture_tx,
            control,
        }
    }

    impl Rig {
        /// Mirror of the engine's midir callback: stamp the packet, push
        /// it into the input ring, unpark the graph thread.
        fn pump(&mut self, bytes: &[u8]) {
            let now = now_ns(self.epoch);
            self.input.push(InMsg::new(now, bytes)).unwrap();
            self.graph.thread().unpark();
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

        /// Poll until the active scene reaches `want`: gestures land
        /// asynchronously (graph thread, control thread, build), so the
        /// switch needs a bounded wait, never a fixed sleep.
        fn wait_active(&self, want: usize) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.handle.active() != want && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(self.handle.active(), want, "scene switch never landed");
        }

        /// Mirror of the engine's wind-down: stop flag, input closed,
        /// control thread down, graph joined, then scheduler shutdown.
        /// Returns whatever else was sent.
        fn finish(self) -> Vec<(Timestamp, Vec<u8>)> {
            self.stop.store(true, Ordering::Release);
            drop(self.input);
            self.gestures.send(ControlMsg::Shutdown).unwrap();
            self.control.join().unwrap();
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

    /// A build closure that records the scene indices it was asked for,
    /// doubling as the rebuild counter the no-op assertions need.
    fn recording_build(
        graphs: impl Fn(usize) -> Node + Send + Sync + 'static,
    ) -> (Arc<Mutex<Vec<usize>>>, BuildScene) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let build: BuildScene = Box::new(move |i| {
            seen.lock().unwrap().push(i);
            Ok(graphs(i))
        });
        (calls, build)
    }

    #[test]
    fn filter_eats_gesture_notes_and_only_their_tails() {
        let (tx, rx) = mpsc::channel();
        let def = def_with(|d| {
            d.next_scene = Some(10);
            d.prev_scene = Some(11);
            d.gotos = vec![(12, 3)];
            d.panic_key = Some(13);
        });
        let mut f = GestureFilter::new(&def, tx);
        // The gesture note-on is eaten, and so are the poly-pressure and
        // the note-off that belong to it, but only on its own channel.
        assert!(f.consume(&EventKind::NoteOn {
            ch: 2,
            key: 10,
            vel: 9
        }));
        assert!(f.consume(&EventKind::PolyPressure {
            ch: 2,
            key: 10,
            value: 5
        }));
        assert!(!f.consume(&EventKind::NoteOff {
            ch: 3,
            key: 10,
            vel: 0
        }));
        assert!(f.consume(&EventKind::NoteOff {
            ch: 2,
            key: 10,
            vel: 0
        }));
        // The claim is spent: a later off or pressure on the reserved
        // key (a note held from before the key was reserved) passes.
        assert!(!f.consume(&EventKind::NoteOff {
            ch: 2,
            key: 10,
            vel: 0
        }));
        assert!(!f.consume(&EventKind::PolyPressure {
            ch: 2,
            key: 10,
            value: 5
        }));
        // Ordinary traffic passes untouched.
        assert!(!f.consume(&EventKind::NoteOn {
            ch: 2,
            key: 60,
            vel: 9
        }));
        assert!(!f.consume(&EventKind::ControlChange {
            ch: 2,
            cc: 64,
            value: 127
        }));
        // Every reserved key decodes to its own gesture.
        for key in [11, 12, 13] {
            assert!(f.consume(&EventKind::NoteOn { ch: 0, key, vel: 1 }));
        }
        let got: Vec<Gesture> = rx
            .try_iter()
            .map(|m| match m {
                ControlMsg::Gesture(g) => g,
                _ => panic!("unexpected control message"),
            })
            .collect();
        assert_eq!(
            got,
            vec![
                Gesture::Next,
                Gesture::Prev,
                Gesture::Goto(3),
                Gesture::Panic
            ]
        );
    }

    #[test]
    fn running_status_survives_a_consumed_gesture() {
        let (tx, _rx) = mpsc::channel();
        let def = def_with(|d| d.next_scene = Some(100));
        let mut f = GestureFilter::new(&def, tx);
        let mut p = Pipeline::new(Node::Leaf(Box::new(Pass)));
        let mut out = Vec::new();
        // One packet, running status: the gesture note then a normal
        // note. Filtering decoded events (not bytes) keeps the second
        // note intact.
        p.handle_filtered(
            0,
            &[0x90, 100, 127, 60, 100],
            Some(&mut f),
            &mut |ev| out.push(ev.kind),
            &mut |_| panic!("unexpected raw bytes"),
        );
        assert_eq!(
            out,
            vec![EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel: 100
            }]
        );
    }

    #[test]
    fn next_gesture_switches_scenes_and_never_sounds() {
        let build: BuildScene = Box::new(|i| Ok(Node::Leaf(Box::new(Shift([2, 5][i])))));
        let def = def_with(|d| d.next_scene = Some(100));
        let mut rig = rig(vec![scene("a"), scene("b")], build, def);
        rig.pump(&[0x90, 60, 100]);
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x90, 62, 100]]);
        // The gesture, on a different channel than the playing: consumed
        // on any channel, and the switch lands within a bounded wait.
        rig.pump(&[0x93, 100, 127]);
        rig.wait_active(1);
        // A fresh note maps through scene 1's transpose; the held note's
        // off still drains through scene 0. The gesture itself never
        // sounded: the output is exactly these two events, no key
        // 102 or 105 anywhere.
        rig.pump(&[0x90, 61, 100]);
        rig.pump(&[0x80, 60, 0]);
        let sent = rig.wait_sends(2);
        assert_eq!(
            bytes_of(&sent),
            vec![vec![0x90, 66, 100], vec![0x80, 62, 0]]
        );
        // Shutdown silences the one note scene 1 left sounding.
        assert_eq!(bytes_of(&rig.finish()), vec![vec![0x80, 66, 0]]);
    }

    #[test]
    fn gesture_tail_is_consumed_and_neighbors_pass() {
        let build: BuildScene = Box::new(|_| Ok(Node::Leaf(Box::new(Pass))));
        let def = def_with(|d| d.next_scene = Some(100));
        let mut rig = rig(vec![scene("a"), scene("b")], build, def);
        // The gesture's whole life is eaten: on, pressure, off. A
        // non-gesture key on the same channel passes on both edges.
        rig.pump(&[0x90, 100, 127]);
        rig.pump(&[0xA0, 100, 33]);
        rig.pump(&[0x80, 100, 0]);
        rig.pump(&[0x90, 99, 80]);
        rig.pump(&[0x80, 99, 0]);
        // Ring order is send order: had any gesture byte leaked, it
        // would precede the normal note in the output.
        let sent = rig.wait_sends(2);
        assert_eq!(bytes_of(&sent), vec![vec![0x90, 99, 80], vec![0x80, 99, 0]]);
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    #[test]
    fn goto_jumps_and_an_active_goto_never_rebuilds() {
        let (calls, build) = recording_build(|_| Node::Leaf(Box::new(Pass)));
        let def = def_with(|d| d.gotos = vec![(101, 2), (102, 0)]);
        let mut rig = rig(vec![scene("a"), scene("b"), scene("c")], build, def);
        rig.pump(&[0x90, 101, 127]);
        rig.wait_active(2);
        // A goto for the scene already active: consumed, but a no-op.
        // The later switch back to 0 proves it was processed (the
        // control channel is FIFO) without ever calling build.
        rig.pump(&[0x90, 101, 127]);
        rig.pump(&[0x90, 102, 127]);
        rig.wait_active(0);
        assert_eq!(*calls.lock().unwrap(), vec![0, 2, 0]);
        rig.finish();
    }

    #[test]
    fn panic_key_triggers_the_sweep() {
        let build: BuildScene = Box::new(|_| Ok(Node::Leaf(Box::new(Pass))));
        let def = def_with(|d| d.panic_key = Some(103));
        let mut rig = rig(vec![scene("only")], build, def);
        rig.pump(&[0x90, 60, 100]);
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x90, 60, 100]]);
        rig.pump(&[0x90, 103, 127]);
        // The panic silences the sounding note and sweeps all three
        // channel-mode messages across all 16 channels: 49 sends.
        let sent = bytes_of(&rig.wait_sends(49));
        assert!(sent.contains(&vec![0x80, 60, 0]), "no silence: {sent:?}");
        for ch in 0..16u8 {
            for cc in [120, 121, 123] {
                assert!(sent.contains(&vec![0xB0 | ch, cc, 0]), "missing sweep");
            }
        }
        // The gesture key itself never sounded.
        assert!(sent.iter().all(|b| b.len() < 2 || b[1] != 103));
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    fn moments_rig(seed: u64) -> (Arc<Mutex<Vec<usize>>>, Rig) {
        let (calls, build) = recording_build(|_| Node::Leaf(Box::new(Pass)));
        let def = def_with(|d| {
            d.moments = Some(MomentsDef {
                dwell_lo_ns: 60 * MS,
                dwell_hi_ns: 80 * MS,
                seed,
            })
        });
        let r = rig(vec![scene("a"), scene("b"), scene("c")], build, def);
        (calls, r)
    }

    /// Poll the recording build until `n` calls happened (the initial
    /// pipeline build plus the automatic switches) or a generous
    /// deadline passes.
    fn wait_switches(calls: &Mutex<Vec<usize>>, n: usize) -> Vec<usize> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let got = calls.lock().unwrap().clone();
            if got.len() >= n || Instant::now() >= deadline {
                return got;
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn moments_wander_deterministically_and_never_pick_the_active_scene() {
        // With a 60-80ms dwell, three automatic switches fit comfortably
        // inside the bounded window. The build-call record is the switch
        // order, observed through the scene-distinguishing test graphs.
        let (calls_a, rig_a) = moments_rig(7);
        let seq_a = wait_switches(&calls_a, 4);
        rig_a.finish();
        let (calls_b, rig_b) = moments_rig(7);
        let seq_b = wait_switches(&calls_b, 4);
        rig_b.finish();
        assert!(
            seq_a.len() >= 4 && seq_b.len() >= 4,
            "fewer than three automatic switches: {seq_a:?} / {seq_b:?}"
        );
        // Never the active scene: every switch differs from the scene it
        // leaves, including the initial scene 0.
        for run in [&seq_a, &seq_b] {
            for w in run.windows(2) {
                assert_ne!(w[0], w[1], "a moment re-picked the active scene: {run:?}");
            }
        }
        // Same seed, same wander.
        assert_eq!(seq_a[..4], seq_b[..4], "switch order not deterministic");
    }

    #[test]
    fn moment_picks_avoid_the_active_scene_and_repeat_per_seed() {
        let mk = || {
            Moments::new(MomentsDef {
                dwell_lo_ns: 60 * MS,
                dwell_hi_ns: 80 * MS,
                seed: 9,
            })
        };
        let (mut a, mut b) = (mk(), mk());
        let walk = |m: &mut Moments| {
            let mut active = 0;
            let mut seq = Vec::new();
            for _ in 0..50 {
                let target = m.pick(active, 4).expect("four scenes to pick from");
                assert_ne!(target, active);
                assert!(target < 4);
                seq.push(target);
                active = target;
            }
            seq
        };
        assert_eq!(walk(&mut a), walk(&mut b));
        // A single scene leaves nowhere to go.
        assert_eq!(mk().pick(0, 1), None);
    }

    /// The dwell-reset bookkeeping, unit tested: asserting the reset
    /// through the full thread stack would need tight timing that flakes
    /// on loaded CI runners, so the coarse behavior lives here.
    #[test]
    fn a_manual_change_restarts_the_dwell_without_a_draw() {
        let mut m = Moments::new(MomentsDef {
            dwell_lo_ns: 60 * MS,
            dwell_hi_ns: 80 * MS,
            seed: 3,
        });
        let dwell = m.dwell;
        assert!(dwell >= Duration::from_millis(60) && dwell <= Duration::from_millis(80));
        let before = m.deadline;
        thread::sleep(Duration::from_millis(10));
        m.rewind();
        // The countdown restarted from now (sleep never undershoots), and
        // the drawn dwell is untouched: no RNG draw was consumed, so the
        // automatic sequence stays deterministic across manual changes.
        assert!(m.deadline >= before + Duration::from_millis(10));
        assert_eq!(m.dwell, dwell);
    }
}
