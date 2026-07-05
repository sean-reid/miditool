//! The send side: a dedicated thread that owns the MIDI output and the
//! boundary note tracker, and sends every event at its intended time.
//!
//! The MIDI callback pushes [`Msg`]s into a lock-free SPSC ring and
//! unparks this thread; nothing is sent from the callback itself. Control
//! messages travel on a separate mpsc channel rather than the ring: they
//! come from cold-path threads (the engine handle), which would break the
//! ring's single-producer contract, and mpsc's costs do not matter off
//! the hot path.
//!
//! Timing: the thread parks until roughly half a millisecond before the
//! next deadline (interruptible by unpark when new work arrives), then
//! spins the final stretch for sub-millisecond accuracy.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use miditool_core::event::{CC_ALL_NOTES_OFF, CC_ALL_SOUND_OFF, CC_RESET_CONTROLLERS};
use miditool_core::{Event, EventBuf, EventKind, NoteTracker, Timestamp, wire};

/// Ring capacity in messages. Generous: at the MIDI wire's own pace this
/// is several seconds of dense traffic.
pub(crate) const RING_CAPACITY: usize = 4096;

/// How close to a deadline parking hands over to spinning.
const SPIN_NS: u64 = 500_000;

/// One item on the callback-to-scheduler ring.
pub(crate) enum Msg {
    /// A graph output event, to be sent at `ev.time`. `seq` is assigned
    /// at push time so equal-time events keep their order.
    Event { seq: u64, ev: Event },
    /// Short passthrough bytes (realtime, system common): sent as soon as
    /// the scheduler sees them, ahead of anything waiting in the heap.
    Raw { len: u8, bytes: [u8; 3] },
    /// SysEx passthrough, sent like `Raw`. The one place the hot path
    /// allocates: SysEx is rare, arbitrarily long, and never
    /// latency-critical.
    Sysex(Box<[u8]>),
}

/// Cold-path commands for the scheduler thread.
pub(crate) enum Control {
    /// Send pending note-offs immediately, drop the rest, silence the
    /// tracker, and exit the thread.
    Shutdown,
    /// Drop everything pending and silence hard; keep running.
    Panic,
}

/// Nanoseconds since the engine's shared epoch.
pub(crate) fn now_ns(epoch: Instant) -> Timestamp {
    epoch.elapsed().as_nanos() as Timestamp
}

/// A heap entry, ordered by (time, seq) through [`Reverse`] so the
/// max-heap pops the earliest deadline first.
struct Scheduled {
    time: Timestamp,
    seq: u64,
    kind: EventKind,
}

impl PartialEq for Scheduled {
    fn eq(&self, other: &Self) -> bool {
        (self.time, self.seq) == (other.time, other.seq)
    }
}

impl Eq for Scheduled {}

impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.time, self.seq).cmp(&(other.time, other.seq))
    }
}

/// The scheduler's decision core: the deadline heap plus the note tracker
/// observing everything actually sent. Pure logic behind a byte-sink
/// boundary, so it is testable without a real output port; only the
/// thread shell in [`crate::Engine`] touches hardware.
pub(crate) struct SchedulerCore {
    heap: BinaryHeap<Reverse<Scheduled>>,
    tracker: NoteTracker,
}

impl SchedulerCore {
    pub(crate) fn new() -> Self {
        Self {
            // Preallocated past the ring size; the send loop only
            // allocates if more than a whole ring's worth is pending.
            heap: BinaryHeap::with_capacity(2 * RING_CAPACITY),
            tracker: NoteTracker::new(),
        }
    }

    pub(crate) fn schedule(&mut self, time: Timestamp, seq: u64, kind: EventKind) {
        self.heap.push(Reverse(Scheduled { time, seq, kind }));
    }

    /// The next send moment, if anything is pending.
    pub(crate) fn next_deadline(&self) -> Option<Timestamp> {
        self.heap.peek().map(|Reverse(s)| s.time)
    }

    /// Send every event whose time has come.
    pub(crate) fn send_due(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        let mut buf = [0u8; 3];
        while let Some(Reverse(next)) = self.heap.peek() {
            if next.time > now {
                break;
            }
            let Reverse(s) = self.heap.pop().expect("peeked entry");
            send(wire::encode(&s.kind, &mut buf));
            self.tracker.observe(&s.kind);
        }
    }

    /// Wind down: pending note-offs are sent immediately in heap order so
    /// nothing the tracker has seen keeps sounding, other pending events
    /// are dropped, and the tracker silences whatever remains.
    pub(crate) fn shutdown(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        let mut buf = [0u8; 3];
        while let Some(Reverse(s)) = self.heap.pop() {
            if matches!(s.kind, EventKind::NoteOff { .. }) {
                send(wire::encode(&s.kind, &mut buf));
                self.tracker.observe(&s.kind);
            }
        }
        self.silence(now, send);
    }

    /// Emergency stop: drop everything pending, silence the tracker, then
    /// send All Notes Off, All Sound Off, and Reset All Controllers on all
    /// 16 channels for anything the tracker could not know about.
    pub(crate) fn panic(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        let mut buf = [0u8; 3];
        self.heap.clear();
        self.silence(now, send);
        for ch in 0..16 {
            for cc in [CC_ALL_NOTES_OFF, CC_ALL_SOUND_OFF, CC_RESET_CONTROLLERS] {
                let kind = EventKind::ControlChange { ch, cc, value: 0 };
                send(wire::encode(&kind, &mut buf));
            }
        }
    }

    fn silence(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        let mut buf = [0u8; 3];
        let mut out = EventBuf::new();
        self.tracker.silence(now, &mut out);
        for e in &out {
            send(wire::encode(&e.kind, &mut buf));
        }
    }
}

/// The scheduler thread body. Returns when a [`Control::Shutdown`]
/// arrives; the shell around it owns the real output.
pub(crate) fn scheduler_loop(
    epoch: Instant,
    mut ring: rtrb::Consumer<Msg>,
    controls: mpsc::Receiver<Control>,
    send: &mut impl FnMut(&[u8]),
) {
    let mut core = SchedulerCore::new();
    loop {
        match controls.try_recv() {
            Ok(Control::Shutdown) => {
                // The engine's teardown flush is already in the ring; take
                // it so its note-offs are not lost.
                drain(&mut ring, &mut core, send);
                core.shutdown(now_ns(epoch), send);
                return;
            }
            Ok(Control::Panic) => {
                // Whatever was queued before the panic dies with it.
                while ring.pop().is_ok() {}
                core.panic(now_ns(epoch), send);
            }
            Err(_) => {}
        }
        drain(&mut ring, &mut core, send);
        core.send_due(now_ns(epoch), send);
        match core.next_deadline() {
            Some(deadline) => wait_until(epoch, deadline),
            None => thread::park(),
        }
    }
}

/// Move everything out of the ring: events into the heap, raw bytes
/// straight out the port.
fn drain(ring: &mut rtrb::Consumer<Msg>, core: &mut SchedulerCore, send: &mut impl FnMut(&[u8])) {
    while let Ok(msg) = ring.pop() {
        match msg {
            Msg::Event { seq, ev } => core.schedule(ev.time, seq, ev.kind),
            Msg::Raw { len, bytes } => send(&bytes[..len as usize]),
            Msg::Sysex(bytes) => send(&bytes),
        }
    }
}

/// Sleep toward `deadline`: park until the spin margin (returning early
/// if unparked, so new work re-enters the loop), then spin the tail.
fn wait_until(epoch: Instant, deadline: Timestamp) {
    let now = now_ns(epoch);
    if now >= deadline {
        return;
    }
    let remaining = deadline - now;
    if remaining > SPIN_NS {
        thread::park_timeout(Duration::from_nanos(remaining - SPIN_NS));
        return;
    }
    let mut spins = 0u32;
    while now_ns(epoch) < deadline {
        std::hint::spin_loop();
        spins += 1;
        if spins.is_multiple_of(64) {
            thread::yield_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Timestamp = 1_000_000;

    fn on(key: u8) -> EventKind {
        EventKind::NoteOn {
            ch: 0,
            key,
            vel: 100,
        }
    }

    fn off(key: u8) -> EventKind {
        EventKind::NoteOff { ch: 0, key, vel: 0 }
    }

    fn on_bytes(key: u8) -> Vec<u8> {
        vec![0x90, key, 100]
    }

    fn off_bytes(key: u8) -> Vec<u8> {
        vec![0x80, key, 0]
    }

    #[test]
    fn sends_in_time_then_seq_order() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        core.schedule(30 * MS, 1, on(61));
        core.schedule(10 * MS, 2, on(62));
        let mut sent = Vec::new();
        core.send_due(0, &mut |b| sent.push(b.to_vec()));
        assert_eq!(sent, vec![on_bytes(60)]);
        assert_eq!(core.next_deadline(), Some(10 * MS));
        core.send_due(30 * MS, &mut |b| sent.push(b.to_vec()));
        assert_eq!(sent, vec![on_bytes(60), on_bytes(62), on_bytes(61)]);
        assert_eq!(core.next_deadline(), None);
    }

    #[test]
    fn equal_time_events_keep_push_order() {
        let mut core = SchedulerCore::new();
        core.schedule(5, 7, on(60));
        core.schedule(5, 8, off(60));
        let mut sent = Vec::new();
        core.send_due(5, &mut |b| sent.push(b.to_vec()));
        assert_eq!(sent, vec![on_bytes(60), off_bytes(60)]);
    }

    #[test]
    fn shutdown_sends_pending_note_offs_and_drops_the_rest() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        let mut sent = Vec::new();
        core.send_due(0, &mut |b| sent.push(b.to_vec()));
        core.schedule(20 * MS, 1, off(60));
        core.schedule(10 * MS, 2, on(61));
        core.shutdown(MS, &mut |b| sent.push(b.to_vec()));
        // The future note-on is dropped, its bytes never sent; the future
        // note-off still goes out so nothing keeps sounding.
        assert_eq!(sent, vec![on_bytes(60), off_bytes(60)]);
        assert_eq!(core.tracker.active(), 0);
    }

    #[test]
    fn shutdown_silences_hanging_notes() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        let mut sent = Vec::new();
        core.send_due(0, &mut |b| sent.push(b.to_vec()));
        core.shutdown(MS, &mut |b| sent.push(b.to_vec()));
        assert_eq!(sent, vec![on_bytes(60), off_bytes(60)]);
    }

    #[test]
    fn shutdown_after_balanced_notes_sends_nothing() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        core.schedule(1, 1, off(60));
        let mut sent = Vec::new();
        core.send_due(1, &mut |b| sent.push(b.to_vec()));
        sent.clear();
        core.shutdown(MS, &mut |b| sent.push(b.to_vec()));
        assert!(sent.is_empty());
    }

    #[test]
    fn no_stuck_notes_on_early_shutdown() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        core.schedule(5 * MS, 1, off(60));
        let mut sent = Vec::new();
        core.send_due(0, &mut |b| sent.push(b.to_vec()));
        // Shutdown lands before the note-off's deadline; it must still be
        // sent, immediately, and the tracker must end silent.
        core.shutdown(MS, &mut |b| sent.push(b.to_vec()));
        assert_eq!(sent, vec![on_bytes(60), off_bytes(60)]);
        assert_eq!(core.tracker.active(), 0);
    }

    #[test]
    fn panic_drops_pending_and_silences_everything() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        let mut sent = Vec::new();
        core.send_due(0, &mut |b| sent.push(b.to_vec()));
        core.schedule(10 * MS, 1, on(61));
        sent.clear();
        core.panic(MS, &mut |b| sent.push(b.to_vec()));
        assert!(sent.contains(&off_bytes(60)));
        for ch in 0..16u8 {
            for cc in [120, 121, 123] {
                assert!(sent.contains(&vec![0xB0 | ch, cc, 0]));
            }
        }
        // The pending note-on died with the panic; the loop keeps running.
        assert_eq!(core.next_deadline(), None);
        assert_eq!(core.tracker.active(), 0);
    }

    /// Spawn `scheduler_loop` against channels, returning handles plus a
    /// receiver of (send time, bytes) pairs.
    #[allow(clippy::type_complexity)]
    fn spawn_loop(
        epoch: Instant,
    ) -> (
        rtrb::Producer<Msg>,
        mpsc::Sender<Control>,
        mpsc::Receiver<(Timestamp, Vec<u8>)>,
        thread::JoinHandle<()>,
    ) {
        let (prod, cons) = rtrb::RingBuffer::new(64);
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            scheduler_loop(epoch, cons, ctl_rx, &mut |b| {
                out_tx.send((now_ns(epoch), b.to_vec())).unwrap();
            });
        });
        (prod, ctl_tx, out_rx, handle)
    }

    #[test]
    fn loop_sends_at_deadlines_in_order() {
        let epoch = Instant::now();
        let (mut prod, ctl, out, handle) = spawn_loop(epoch);
        let t0 = now_ns(epoch);
        let plan = [(0, on(60)), (30 * MS, off(60)), (10 * MS, on(62))];
        for (i, (dt, kind)) in plan.into_iter().enumerate() {
            let ev = Event::new(t0 + dt, kind);
            prod.push(Msg::Event { seq: i as u64, ev }).unwrap();
        }
        handle.thread().unpark();
        thread::sleep(Duration::from_millis(60));
        ctl.send(Control::Shutdown).unwrap();
        handle.thread().unpark();
        handle.join().unwrap();

        let sent: Vec<_> = out.try_iter().collect();
        let bytes: Vec<_> = sent.iter().map(|(_, b)| b.clone()).collect();
        // The note-on at +10ms overtakes the note-off pushed before it;
        // shutdown then silences the hanging on(62).
        assert_eq!(
            bytes,
            vec![on_bytes(60), on_bytes(62), off_bytes(60), off_bytes(62)]
        );
        let targets = [t0, t0 + 10 * MS, t0 + 30 * MS];
        for ((at, _), want) in sent.iter().zip(targets) {
            let error = at.abs_diff(want);
            assert!(error < 15 * MS, "sent {}ms off its deadline", error / MS);
        }
    }

    #[test]
    fn raw_and_sysex_bypass_the_heap() {
        let epoch = Instant::now();
        let (mut prod, ctl, out, handle) = spawn_loop(epoch);
        let future = Event::new(now_ns(epoch) + 50 * MS, on(60));
        prod.push(Msg::Event { seq: 0, ev: future }).unwrap();
        prod.push(Msg::Raw {
            len: 1,
            bytes: [0xF8, 0, 0],
        })
        .unwrap();
        prod.push(Msg::Sysex(Box::new([0xF0, 1, 2, 0xF7]))).unwrap();
        handle.thread().unpark();
        thread::sleep(Duration::from_millis(10));
        ctl.send(Control::Shutdown).unwrap();
        handle.thread().unpark();
        handle.join().unwrap();
        // Raw bytes go straight out; the future note-on never fires (it is
        // dropped by shutdown, and never reached the tracker).
        let bytes: Vec<_> = out.try_iter().map(|(_, b)| b).collect();
        assert_eq!(bytes, vec![vec![0xF8], vec![0xF0, 1, 2, 0xF7]]);
    }
}
