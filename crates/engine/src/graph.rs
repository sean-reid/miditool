//! The graph thread: the dedicated realtime thread that owns the
//! [`Pipeline`] and runs the effect graph on a steady tick.
//!
//! The MIDI input callback does almost nothing: it timestamps the raw
//! packet, pushes it into a lock-free SPSC input ring, and unparks this
//! thread. The loop here drains that ring through the pipeline, applies
//! pending graph swaps (even while the player is idle), and every
//! [`TICK_NS`] advances free-running effects via [`Pipeline::tick`].
//! Everything the pipeline emits goes out through the [`Feeder`], the
//! single producer into the scheduler's ring, so both rings stay SPSC.
//!
//! Waking: the callback unparks the thread on every packet, so input is
//! drained immediately (the extra hop costs one ring push and an unpark,
//! well under a millisecond); with no input the thread parks until the
//! next tick deadline.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use miditool_core::{Event, Node, Timestamp};

use crate::pipeline::Pipeline;
use crate::scheduler::{Msg, now_ns};

/// Input ring capacity in packets. Generous: the wire itself cannot carry
/// this many packets in the few microseconds the graph thread needs to
/// wake and drain.
pub(crate) const INPUT_RING_CAPACITY: usize = 1024;

/// How many packet bytes an [`InMsg`] stores inline. Covers every channel
/// voice message run the backend batches short of SysEx.
const INPUT_INLINE: usize = 30;

/// The steady tick period: how often free-running effects advance.
pub(crate) const TICK_NS: Timestamp = 5_000_000;

/// One item on the callback-to-graph input ring: a raw input packet
/// stamped with its arrival time.
pub(crate) enum InMsg {
    /// A short packet, stored inline.
    Packet {
        time: Timestamp,
        len: u8,
        bytes: [u8; INPUT_INLINE],
    },
    /// A packet too long for the inline buffer: in practice SysEx, the
    /// documented hot-path allocation exception.
    Long { time: Timestamp, bytes: Box<[u8]> },
}

impl InMsg {
    /// Package one timestamped packet for the ring.
    pub(crate) fn new(time: Timestamp, bytes: &[u8]) -> Self {
        if bytes.len() <= INPUT_INLINE {
            let mut buf = [0u8; INPUT_INLINE];
            buf[..bytes.len()].copy_from_slice(bytes);
            InMsg::Packet {
                time,
                len: bytes.len() as u8,
                bytes: buf,
            }
        } else {
            InMsg::Long {
                time,
                bytes: bytes.into(),
            }
        }
    }

    fn parts(&self) -> (Timestamp, &[u8]) {
        match self {
            InMsg::Packet { time, len, bytes } => (*time, &bytes[..*len as usize]),
            InMsg::Long { time, bytes } => (*time, bytes),
        }
    }
}

/// The graph thread's single-producer handle to the scheduler: the ring,
/// the running sequence counter, and the thread to unpark.
pub(crate) struct Feeder {
    pub(crate) ring: rtrb::Producer<Msg>,
    /// Assigned to events at push time so equal-time events keep their
    /// order. Raw pushes consume a number too, making `seq` double as a
    /// push counter; the heap only needs it monotonic, so gaps are fine.
    pub(crate) seq: u64,
    pub(crate) dropped: Arc<AtomicU64>,
    pub(crate) scheduler: Thread,
}

impl Feeder {
    pub(crate) fn event(&mut self, ev: Event) {
        let seq = self.seq;
        self.seq += 1;
        if self.ring.push(Msg::Event { seq, ev }).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn raw(&mut self, bytes: &[u8]) {
        self.seq += 1;
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

    /// Pushes so far; the graph loop compares marks to decide whether the
    /// scheduler needs a wakeup.
    fn mark(&self) -> u64 {
        self.seq
    }

    pub(crate) fn wake(&self) {
        self.scheduler.unpark();
    }
}

/// The graph thread body. Returns once `stop` is set, after a final drain
/// of the input ring and a full [`Pipeline::shutdown`] through the feeder;
/// the scheduler's own shutdown control follows from the engine.
///
/// The feeder sits in a `RefCell` because `Pipeline::handle` takes two
/// sink closures that both need it; they never overlap, so the borrow
/// always succeeds.
pub(crate) fn graph_loop(
    epoch: Instant,
    mut pipeline: Pipeline,
    mut input: rtrb::Consumer<InMsg>,
    graphs: mpsc::Receiver<Node>,
    feeder: Feeder,
    stop: Arc<AtomicBool>,
) {
    let feeder = RefCell::new(feeder);
    let mut next_tick = now_ns(epoch) + TICK_NS;
    loop {
        // Load the flag before the final drain: everything the callback
        // pushed before the engine closed the input is still taken. A
        // callback racing the flag itself can at worst lose a packet whose
        // notes the scheduler's shutdown silence covers anyway.
        let stopping = stop.load(Ordering::Acquire);
        let mark = feeder.borrow().mark();
        while let Ok(msg) = input.pop() {
            // A swap queued before a packet is processed installs before
            // that packet runs, exactly as when the callback owned the
            // pipeline. try_recv on an empty channel is hot-path cheap.
            apply_swaps(&mut pipeline, &graphs, &feeder, epoch);
            let (time, bytes) = msg.parts();
            pipeline.handle(
                time,
                bytes,
                &mut |ev| feeder.borrow_mut().event(ev),
                &mut |b| feeder.borrow_mut().raw(b),
            );
        }
        // Idle swaps: with no input in flight a pending swap still lands
        // here, within one tick period of being queued.
        apply_swaps(&mut pipeline, &graphs, &feeder, epoch);
        if stopping {
            pipeline.shutdown(now_ns(epoch), &mut |ev| feeder.borrow_mut().event(ev));
            feeder.borrow().wake();
            return;
        }
        let now = now_ns(epoch);
        if now >= next_tick {
            pipeline.tick(now, &mut |ev| feeder.borrow_mut().event(ev));
            next_tick = now + TICK_NS;
        }
        if feeder.borrow().mark() != mark {
            feeder.borrow().wake();
        }
        let now = now_ns(epoch);
        if now < next_tick {
            // Wakes early when the callback unparks us with fresh input;
            // a park token left by a push we already drained just costs
            // one extra empty pass.
            thread::park_timeout(Duration::from_nanos(next_tick - now));
        }
    }
}

/// Install every pending graph swap.
fn apply_swaps(
    pipeline: &mut Pipeline,
    graphs: &mpsc::Receiver<Node>,
    feeder: &RefCell<Feeder>,
    epoch: Instant,
) {
    while let Ok(root) = graphs.try_recv() {
        pipeline.swap_graph(now_ns(epoch), root, &mut |ev| feeder.borrow_mut().event(ev));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use miditool_core::graph::{Effect, Pass};
    use miditool_core::{EventBuf, EventKind, ProcCx};

    use crate::scheduler::{Control, Tap, scheduler_loop};

    /// A full two-ring rig: input ring into a real graph thread, feeder
    /// ring into a real scheduler thread, output collected as (send time,
    /// bytes) pairs. Everything the engine wires up except midir.
    struct Rig {
        epoch: Instant,
        input: rtrb::Producer<InMsg>,
        graphs: mpsc::Sender<Node>,
        stop: Arc<AtomicBool>,
        graph: thread::JoinHandle<()>,
        ctl: mpsc::Sender<Control>,
        out: mpsc::Receiver<(Timestamp, Vec<u8>)>,
        sched: thread::JoinHandle<()>,
    }

    fn rig(root: Node) -> Rig {
        let epoch = Instant::now();
        let (in_tx, in_rx) = rtrb::RingBuffer::new(64);
        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(256);
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let (tap_tx, _tap_rx) = rtrb::RingBuffer::new(4);
        let tap = Tap {
            ring: tap_tx,
            enabled: Arc::new(AtomicBool::new(false)),
        };
        let sched = thread::spawn(move || {
            scheduler_loop(epoch, ring_rx, ctl_rx, tap, &mut |b| {
                out_tx.send((now_ns(epoch), b.to_vec())).unwrap();
            });
        });
        let feeder = Feeder {
            ring: ring_tx,
            seq: 0,
            dropped: Arc::new(AtomicU64::new(0)),
            scheduler: sched.thread().clone(),
        };
        let (graph_tx, graph_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let graph = thread::spawn(move || {
            graph_loop(epoch, Pipeline::new(root), in_rx, graph_rx, feeder, flag);
        });
        Rig {
            epoch,
            input: in_tx,
            graphs: graph_tx,
            stop,
            graph,
            ctl: ctl_tx,
            out: out_rx,
            sched,
        }
    }

    impl Rig {
        /// Mirror of the engine's midir callback: stamp, push, unpark.
        /// Returns the timestamp used.
        fn pump(&mut self, bytes: &[u8]) -> Timestamp {
            let now = now_ns(self.epoch);
            self.input.push(InMsg::new(now, bytes)).unwrap();
            self.graph.thread().unpark();
            now
        }

        /// Swap in a new graph the way `set_scene` does: queue it, then
        /// unpark the graph thread.
        fn swap(&self, root: Node) {
            self.graphs.send(root).unwrap();
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

    /// Net note-on minus note-off count for `key` across raw sends.
    fn balance(sent: &[(Timestamp, Vec<u8>)], key: u8) -> i32 {
        let mut net = 0;
        for (_, b) in sent {
            if b.len() == 3 && b[1] == key {
                match b[0] & 0xF0 {
                    0x90 if b[2] > 0 => net += 1,
                    0x80 | 0x90 => net -= 1,
                    _ => {}
                }
            }
        }
        net
    }

    /// A free-running stub: each tick releases the previous tick's note
    /// and strikes a fresh one; flush releases whatever still sounds.
    struct Ticker {
        key: u8,
        sounding: bool,
    }

    impl Ticker {
        fn new(key: u8) -> Self {
            Self {
                key,
                sounding: false,
            }
        }

        fn off(&self, time: Timestamp) -> Event {
            Event::new(
                time,
                EventKind::NoteOff {
                    ch: 0,
                    key: self.key,
                    vel: 0,
                },
            )
        }
    }

    impl Effect for Ticker {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, _cx: &ProcCx) {
            out.push(*ev);
        }

        fn tick(&mut self, now: Timestamp, out: &mut EventBuf, _cx: &ProcCx) {
            if self.sounding {
                out.push(self.off(now));
            }
            out.push(Event::new(
                now,
                EventKind::NoteOn {
                    ch: 0,
                    key: self.key,
                    vel: 100,
                },
            ));
            self.sounding = true;
        }

        fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
            if self.sounding {
                out.push(self.off(cx.now));
                self.sounding = false;
            }
        }
    }

    /// Strikes one note on its first tick and holds it; flush releases
    /// it. A drone that only the flush path can end.
    struct Drone {
        key: u8,
        struck: bool,
    }

    impl Effect for Drone {
        fn process(&mut self, _ev: &Event, _out: &mut EventBuf, _cx: &ProcCx) {}

        fn tick(&mut self, now: Timestamp, out: &mut EventBuf, _cx: &ProcCx) {
            if !self.struck {
                out.push(Event::new(
                    now,
                    EventKind::NoteOn {
                        ch: 0,
                        key: self.key,
                        vel: 100,
                    },
                ));
                self.struck = true;
            }
        }

        fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
            if self.struck {
                out.push(Event::new(
                    cx.now,
                    EventKind::NoteOff {
                        ch: 0,
                        key: self.key,
                        vel: 0,
                    },
                ));
                self.struck = false;
            }
        }
    }

    /// Passes everything; on flush emits a marker note-off for key `id`
    /// on channel 15, so a swap or shutdown is observable through the
    /// output alone. A note-off rather than a CC because the scheduler's
    /// shutdown forwards pending note-offs and drops everything else.
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

    fn marker_bytes(id: u8) -> Vec<u8> {
        vec![0x8F, id, 0]
    }

    #[test]
    fn free_running_effect_flows_without_any_input() {
        let rig = rig(Node::Leaf(Box::new(Ticker::new(60))));
        // No input at all: the tick cadence alone must drive notes all the
        // way to the output. Eventual arrival and ordering are asserted;
        // wall-clock pacing never is.
        let sent = rig.wait_sends(6);
        assert!(sent.len() >= 6, "ticks never reached the output: {sent:?}");
        for (i, (_, b)) in sent.iter().enumerate() {
            let want = if i % 2 == 0 { 0x90 } else { 0x80 };
            assert_eq!(b[0] & 0xF0, want, "send {i} out of order: {sent:?}");
            assert_eq!(b[1], 60);
        }
        // Shutdown releases the note left sounding by the last tick.
        let mut all = sent;
        all.extend(rig.finish());
        assert_eq!(balance(&all, 60), 0, "unbalanced notes: {all:?}");
    }

    #[test]
    fn idle_graph_swap_applies_within_a_bounded_wait() {
        let rig = rig(Node::Leaf(Box::new(Marker(1))));
        // No input traffic at all. The idle current graph is flushed on
        // the spot by the swap, so its marker reaching the output proves
        // the new graph is installed.
        rig.swap(Node::Leaf(Box::new(Marker(2))));
        let sent = rig.wait_sends(1);
        assert_eq!(bytes_of(&sent), vec![marker_bytes(1)]);
        // The swapped-in graph is live: the shutdown flush is its marker,
        // not the old graph's.
        assert_eq!(bytes_of(&rig.finish()), vec![marker_bytes(2)]);
    }

    #[test]
    fn input_flows_through_both_rings() {
        let mut rig = rig(Node::Leaf(Box::new(Pass)));
        let t_on = rig.pump(&[0x90, 60, 100]);
        let sent = rig.wait_sends(1);
        assert_eq!(bytes_of(&sent), vec![vec![0x90, 60, 100]]);
        // Never early; lateness is host scheduling noise and stays
        // unasserted (the bench command measures it).
        assert!(sent[0].0 >= t_on, "sent before the callback stamped it");
        rig.pump(&[0x80, 60, 0]);
        assert_eq!(bytes_of(&rig.wait_sends(1)), vec![vec![0x80, 60, 0]]);
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    #[test]
    fn a_swap_queued_before_a_packet_installs_first() {
        let mut rig = rig(Node::Leaf(Box::new(Marker(1))));
        rig.swap(Node::Leaf(Box::new(Pass)));
        // Pushed after the swap was queued: the note must map through the
        // new graph, and the old graph's flush marker precedes it.
        rig.pump(&[0x90, 60, 100]);
        rig.pump(&[0x80, 60, 0]);
        let sent = rig.wait_sends(3);
        assert_eq!(
            bytes_of(&sent),
            vec![marker_bytes(1), vec![0x90, 60, 100], vec![0x80, 60, 0]]
        );
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    #[test]
    fn raw_and_sysex_packets_pass_through_verbatim() {
        let mut rig = rig(Node::Leaf(Box::new(Pass)));
        rig.pump(&[0xF8]);
        // Longer than the inline buffer, exercising the boxed path.
        let sysex: Vec<u8> = std::iter::once(0xF0)
            .chain((0..40).map(|i| i as u8))
            .chain(std::iter::once(0xF7))
            .collect();
        rig.pump(&sysex);
        let sent = rig.wait_sends(2);
        assert_eq!(bytes_of(&sent), vec![vec![0xF8], sysex]);
        assert!(bytes_of(&rig.finish()).is_empty());
    }

    #[test]
    fn shutdown_releases_a_free_running_effects_sounding_notes() {
        let rig = rig(Node::Leaf(Box::new(Drone {
            key: 62,
            struck: false,
        })));
        // Wait until the drone is audibly mid-flight, then wind down.
        let sent = rig.wait_sends(1);
        assert_eq!(bytes_of(&sent), vec![vec![0x90, 62, 100]]);
        // The flush path releases it: the pipeline shutdown emits the
        // note-off through the feeder before the scheduler winds down.
        let rest = rig.finish();
        assert_eq!(bytes_of(&rest), vec![vec![0x80, 62, 0]]);
        let mut all = sent;
        all.extend(rest);
        assert_eq!(balance(&all, 62), 0);
    }
}
