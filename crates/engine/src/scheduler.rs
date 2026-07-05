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
//!
//! The sent-event tap: once a monitor takes the consumer end, every
//! channel event dispatched here is mirrored, stamped with its send
//! moment, into a second fixed-size ring. The push is wait-free and
//! best-effort: a full ring drops the copy rather than delay the send,
//! and raw or SysEx passthrough bytes never appear.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use miditool_core::event::{CC_ALL_NOTES_OFF, CC_ALL_SOUND_OFF, CC_RESET_CONTROLLERS};
use miditool_core::{Event, EventBuf, EventKind, NoteTracker, Timestamp, wire};

/// Ring capacity in messages. Generous: at the MIDI wire's own pace this
/// is several seconds of dense traffic.
pub(crate) const RING_CAPACITY: usize = 4096;

/// Tap ring capacity in events. Plenty for a monitor polling at frame
/// rate; when nobody listens the ring is never touched.
pub(crate) const TAP_CAPACITY: usize = 1024;

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
    /// Scene kill: drop everything pending and send the note-offs (plus
    /// pedal-ups) that stop what is sounding, without the channel-mode
    /// sweep of [`Control::Panic`]; keep running.
    Silence,
}

/// The producer half of the sent-event tap, created alongside the engine
/// and enabled when [`crate::EngineHandle::take_tap`] hands out the
/// consumer end.
pub(crate) struct Tap {
    pub(crate) ring: rtrb::Producer<Event>,
    pub(crate) enabled: Arc<AtomicBool>,
}

impl Tap {
    /// Mirror one sent channel event to the monitor, if one is listening.
    /// Wait-free and allocation-free either way: disabled costs one
    /// relaxed load, and a full ring drops the copy rather than block.
    fn push(&mut self, time: Timestamp, kind: EventKind) {
        if self.enabled.load(Ordering::Relaxed) {
            let _ = self.ring.push(Event::new(time, kind));
        }
    }
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
    /// `None` only in tests; the engine always wires a tap, listened to
    /// or not, so the disabled hot path costs one predictable branch.
    tap: Option<Tap>,
}

impl SchedulerCore {
    pub(crate) fn new() -> Self {
        Self {
            // Preallocated past the ring size; the send loop only
            // allocates if more than a whole ring's worth is pending.
            heap: BinaryHeap::with_capacity(2 * RING_CAPACITY),
            tracker: NoteTracker::new(),
            tap: None,
        }
    }

    pub(crate) fn schedule(&mut self, time: Timestamp, seq: u64, kind: EventKind) {
        self.heap.push(Reverse(Scheduled { time, seq, kind }));
    }

    /// The next send moment, if anything is pending.
    pub(crate) fn next_deadline(&self) -> Option<Timestamp> {
        self.heap.peek().map(|Reverse(s)| s.time)
    }

    /// Encode and send one channel event, keeping the tracker current and
    /// mirroring the event to the tap when a monitor is listening.
    fn dispatch(&mut self, time: Timestamp, kind: EventKind, send: &mut impl FnMut(&[u8])) {
        let mut buf = [0u8; 3];
        send(wire::encode(&kind, &mut buf));
        self.tracker.observe(&kind);
        if let Some(tap) = &mut self.tap {
            tap.push(time, kind);
        }
    }

    /// Send every event whose time has come.
    pub(crate) fn send_due(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        while let Some(Reverse(next)) = self.heap.peek() {
            if next.time > now {
                break;
            }
            let Reverse(s) = self.heap.pop().expect("peeked entry");
            self.dispatch(now, s.kind, send);
        }
    }

    /// Wind down: pending note-offs are sent immediately in heap order so
    /// nothing the tracker has seen keeps sounding, other pending events
    /// are dropped, and the tracker silences whatever remains.
    pub(crate) fn shutdown(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        while let Some(Reverse(s)) = self.heap.pop() {
            if matches!(s.kind, EventKind::NoteOff { .. }) {
                self.dispatch(now, s.kind, send);
            }
        }
        self.silence(now, send);
    }

    /// Scene kill: drop everything pending and send the note-offs (plus
    /// pedal-ups) that stop what is sounding. [`SchedulerCore::panic`]
    /// minus the channel-mode sweep, leaving the DAW's controller state
    /// alone; the loop keeps running.
    pub(crate) fn kill(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        self.heap.clear();
        self.silence(now, send);
    }

    /// Emergency stop: a kill plus All Notes Off, All Sound Off, and
    /// Reset All Controllers on all 16 channels for anything the tracker
    /// could not know about.
    pub(crate) fn panic(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        self.kill(now, send);
        for ch in 0..16 {
            for cc in [CC_ALL_NOTES_OFF, CC_ALL_SOUND_OFF, CC_RESET_CONTROLLERS] {
                self.dispatch(now, EventKind::ControlChange { ch, cc, value: 0 }, send);
            }
        }
    }

    /// Silence everything the tracker has seen sounding. The note-offs go
    /// through [`SchedulerCore::dispatch`] like any other send, so the tap
    /// sees them too; re-observing them is a no-op on the reset tracker.
    fn silence(&mut self, now: Timestamp, send: &mut impl FnMut(&[u8])) {
        let mut out = EventBuf::new();
        self.tracker.silence(now, &mut out);
        for e in &out {
            self.dispatch(e.time, e.kind, send);
        }
    }
}

/// The scheduler thread body. Returns when a [`Control::Shutdown`]
/// arrives; the shell around it owns the real output.
pub(crate) fn scheduler_loop(
    epoch: Instant,
    mut ring: rtrb::Consumer<Msg>,
    controls: mpsc::Receiver<Control>,
    tap: Tap,
    send: &mut impl FnMut(&[u8]),
) {
    let mut core = SchedulerCore::new();
    core.tap = Some(tap);
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
            Ok(Control::Silence) => {
                // A scene kill: output queued before the switch dies too.
                while ring.pop().is_ok() {}
                core.kill(now_ns(epoch), send);
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

    #[test]
    fn kill_silences_and_drops_pending_without_the_sweep() {
        let mut core = SchedulerCore::new();
        core.schedule(0, 0, on(60));
        let mut sent = Vec::new();
        core.send_due(0, &mut |b| sent.push(b.to_vec()));
        core.schedule(10 * MS, 1, off(60));
        core.schedule(20 * MS, 2, on(61));
        sent.clear();
        core.kill(MS, &mut |b| sent.push(b.to_vec()));
        // The sounding note is silenced by the tracker, both pending
        // events are dropped, and no channel-mode sweep goes out.
        assert_eq!(sent, vec![off_bytes(60)]);
        assert_eq!(core.next_deadline(), None);
        assert_eq!(core.tracker.active(), 0);
    }

    /// A running `scheduler_loop` and everything a test needs to poke it:
    /// the ring producer, the control channel, a receiver of (send time,
    /// bytes) pairs, and the tap's consumer end plus its enable flag.
    struct LoopRig {
        prod: rtrb::Producer<Msg>,
        ctl: mpsc::Sender<Control>,
        out: mpsc::Receiver<(Timestamp, Vec<u8>)>,
        handle: thread::JoinHandle<()>,
        tap: rtrb::Consumer<Event>,
        tap_on: Arc<AtomicBool>,
    }

    fn spawn_loop(epoch: Instant, tap_capacity: usize) -> LoopRig {
        let (prod, cons) = rtrb::RingBuffer::new(64);
        let (ctl_tx, ctl_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let (tap_tx, tap_rx) = rtrb::RingBuffer::new(tap_capacity);
        let tap_on = Arc::new(AtomicBool::new(false));
        let tap = Tap {
            ring: tap_tx,
            enabled: Arc::clone(&tap_on),
        };
        let handle = thread::spawn(move || {
            scheduler_loop(epoch, cons, ctl_rx, tap, &mut |b| {
                out_tx.send((now_ns(epoch), b.to_vec())).unwrap();
            });
        });
        LoopRig {
            prod,
            ctl: ctl_tx,
            out: out_rx,
            handle,
            tap: tap_rx,
            tap_on,
        }
    }

    /// Poll `out` until `n` sends arrive or a generous deadline passes;
    /// fixed sleeps flake on loaded CI runners.
    fn wait_sends(
        out: &mpsc::Receiver<(Timestamp, Vec<u8>)>,
        n: usize,
    ) -> Vec<(Timestamp, Vec<u8>)> {
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.len() < n && Instant::now() < deadline {
            got.extend(out.try_iter());
            thread::sleep(Duration::from_millis(1));
        }
        got
    }

    #[test]
    fn loop_sends_at_deadlines_in_order() {
        let epoch = Instant::now();
        let LoopRig {
            mut prod,
            ctl,
            out,
            handle,
            ..
        } = spawn_loop(epoch, 16);
        let t0 = now_ns(epoch);
        let plan = [(0, on(60)), (30 * MS, off(60)), (10 * MS, on(62))];
        for (i, (dt, kind)) in plan.into_iter().enumerate() {
            let ev = Event::new(t0 + dt, kind);
            prod.push(Msg::Event { seq: i as u64, ev }).unwrap();
        }
        handle.thread().unpark();
        // Wait for all three scheduled sends before shutting down, lest a
        // stalled thread hit the shutdown-drops-pending-note-ons path.
        let mut sent = wait_sends(&out, 3);
        ctl.send(Control::Shutdown).unwrap();
        handle.thread().unpark();
        handle.join().unwrap();
        sent.extend(out.try_iter());

        let bytes: Vec<_> = sent.iter().map(|(_, b)| b.clone()).collect();
        // The note-on at +10ms overtakes the note-off pushed before it;
        // shutdown then silences the hanging on(62).
        assert_eq!(
            bytes,
            vec![on_bytes(60), on_bytes(62), off_bytes(60), off_bytes(62)]
        );
        // Order and never-early are guarantees; lateness is host scheduling
        // noise, so it stays unasserted (the bench command measures it).
        let targets = [t0, t0 + 10 * MS, t0 + 30 * MS];
        for ((at, _), want) in sent.iter().zip(targets) {
            assert!(
                *at + MS >= want,
                "sent {}ms before its deadline",
                (want - at) / MS
            );
        }
    }

    #[test]
    fn raw_and_sysex_bypass_the_heap() {
        let epoch = Instant::now();
        let LoopRig {
            mut prod,
            ctl,
            out,
            handle,
            ..
        } = spawn_loop(epoch, 16);
        // Far enough out that no CI stall can make it due before shutdown.
        let future = Event::new(now_ns(epoch) + 60_000 * MS, on(60));
        prod.push(Msg::Event { seq: 0, ev: future }).unwrap();
        prod.push(Msg::Raw {
            len: 1,
            bytes: [0xF8, 0, 0],
        })
        .unwrap();
        prod.push(Msg::Sysex(Box::new([0xF0, 1, 2, 0xF7]))).unwrap();
        handle.thread().unpark();
        let early = wait_sends(&out, 2);
        ctl.send(Control::Shutdown).unwrap();
        handle.thread().unpark();
        handle.join().unwrap();
        // Raw bytes go straight out; the future note-on never fires (it is
        // dropped by shutdown, and never reached the tracker).
        let bytes: Vec<_> = early
            .into_iter()
            .map(|(_, b)| b)
            .chain(out.try_iter().map(|(_, b)| b))
            .collect();
        assert_eq!(bytes, vec![vec![0xF8], vec![0xF0, 1, 2, 0xF7]]);
    }

    #[test]
    fn tap_mirrors_sent_channel_events() {
        let epoch = Instant::now();
        let mut rig = spawn_loop(epoch, 16);
        rig.tap_on.store(true, Ordering::Relaxed);
        let t0 = now_ns(epoch);
        rig.prod
            .push(Msg::Event {
                seq: 0,
                ev: Event::new(t0, on(60)),
            })
            .unwrap();
        rig.handle.thread().unpark();
        assert_eq!(wait_sends(&rig.out, 1).len(), 1);
        rig.ctl.send(Control::Shutdown).unwrap();
        rig.handle.thread().unpark();
        rig.handle.join().unwrap();
        // The shutdown silence for the hanging note is a sent channel
        // event too, so it follows the note-on onto the tap.
        let tapped: Vec<Event> = std::iter::from_fn(|| rig.tap.pop().ok()).collect();
        let kinds: Vec<_> = tapped.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![on(60), off(60)]);
        // Send-time stamps: never before the engine saw the event.
        for e in &tapped {
            assert!(e.time >= t0, "tapped event stamped before its push");
        }
    }

    #[test]
    fn tap_overflow_drops_extra_events_without_blocking() {
        let epoch = Instant::now();
        let mut rig = spawn_loop(epoch, 4);
        rig.tap_on.store(true, Ordering::Relaxed);
        let t0 = now_ns(epoch);
        // Balanced pairs so shutdown has nothing to add.
        let plan = [
            on(60),
            off(60),
            on(61),
            off(61),
            on(62),
            off(62),
            on(63),
            off(63),
        ];
        for (i, kind) in plan.into_iter().enumerate() {
            rig.prod
                .push(Msg::Event {
                    seq: i as u64,
                    ev: Event::new(t0, kind),
                })
                .unwrap();
        }
        rig.handle.thread().unpark();
        // Every event still goes out the port even though the tap ring
        // only holds four of them.
        let sent = wait_sends(&rig.out, 8);
        rig.ctl.send(Control::Shutdown).unwrap();
        rig.handle.thread().unpark();
        rig.handle.join().unwrap();
        assert_eq!(sent.len(), 8);
        let tapped: Vec<_> = std::iter::from_fn(|| rig.tap.pop().ok())
            .map(|e| e.kind)
            .collect();
        assert_eq!(tapped, vec![on(60), off(60), on(61), off(61)]);
    }

    #[test]
    fn raw_bytes_and_a_disabled_tap_stay_off_the_tap() {
        let epoch = Instant::now();
        let mut rig = spawn_loop(epoch, 16);
        let t0 = now_ns(epoch);
        // Sent while nobody listens: never tapped.
        rig.prod
            .push(Msg::Event {
                seq: 0,
                ev: Event::new(t0, on(60)),
            })
            .unwrap();
        rig.handle.thread().unpark();
        assert_eq!(wait_sends(&rig.out, 1).len(), 1);
        // With the tap enabled, raw and SysEx passthrough still bypass it.
        rig.tap_on.store(true, Ordering::Relaxed);
        rig.prod
            .push(Msg::Raw {
                len: 1,
                bytes: [0xF8, 0, 0],
            })
            .unwrap();
        rig.prod
            .push(Msg::Sysex(Box::new([0xF0, 1, 0xF7])))
            .unwrap();
        rig.prod
            .push(Msg::Event {
                seq: 1,
                ev: Event::new(now_ns(epoch), on(61)),
            })
            .unwrap();
        rig.handle.thread().unpark();
        assert_eq!(wait_sends(&rig.out, 3).len(), 3);
        rig.ctl.send(Control::Shutdown).unwrap();
        rig.handle.thread().unpark();
        rig.handle.join().unwrap();
        // Only channel events sent after the take appear: on(61), then the
        // shutdown silence for both hanging notes, in tracker order.
        let tapped: Vec<_> = std::iter::from_fn(|| rig.tap.pop().ok())
            .map(|e| e.kind)
            .collect();
        assert_eq!(tapped, vec![on(61), off(60), off(61)]);
    }
}
