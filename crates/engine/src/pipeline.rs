//! The pure hot path: decode incoming packets, route events through the
//! effect graph, and emit output events (possibly future-timed) for the
//! scheduler to send.
//!
//! Hot reload keeps mididings semantics: a note that went on through one
//! graph must go off through that same graph, so its note-off lands where
//! the old mapping sent it. A swapped-out graph is therefore kept draining
//! until every input note (and sustain pedal) it opened has closed, at
//! which point it is flushed and dropped. Up to [`MAX_DRAINING`] old
//! graphs are retained; a further swap force-flushes the oldest.

use arrayvec::ArrayVec;
use miditool_core::event::CC_SUSTAIN;
use miditool_core::wire::{Decoded, Decoder};
use miditool_core::{Event, EventBuf, EventKind, Node, PerNote, ProcCx, Timestamp};

/// How many swapped-out graphs may drain held notes concurrently.
pub const MAX_DRAINING: usize = 3;

/// A graph generation: the compiled graph plus how much input-side state
/// still points at it. A generation is idle when no held note and no
/// pressed pedal was routed through it.
struct Generation {
    /// Nonzero id; `note_gen` and `sustain_gen` slots store it directly,
    /// with 0 meaning "no owner". Ids wrap after 65535 reloads, so a note
    /// held across that many swaps could route its note-off through the
    /// wrong graph; we accept that.
    id: u16,
    root: Node,
    /// Held input notes that were opened through this graph.
    notes: u16,
    /// Channel mask of sustain-downs this graph processed, not yet matched
    /// by an up routed through it.
    sustain: u16,
}

impl Generation {
    fn idle(&self) -> bool {
        self.notes == 0 && self.sustain == 0
    }
}

/// The pure hot path: decoder plus the current and draining effect graphs.
///
/// Callers feed it raw input packets and a monotonic timestamp. Decoded
/// channel voice messages run through the graph and come out via `emit` as
/// timestamped events whose `time` is the intended send moment. SysEx,
/// system common, and realtime bytes come out via `raw` verbatim. Nothing
/// here allocates or blocks per event.
pub struct Pipeline {
    decoder: Decoder,
    current: Generation,
    /// Swapped-out graphs still draining, oldest first.
    draining: ArrayVec<Generation, MAX_DRAINING>,
    /// Last id handed out; see [`Generation::id`].
    counter: u16,
    /// Which generation opened each held input (channel, key); 0 = none.
    note_gen: PerNote<u16>,
    /// Which generation saw each channel's pending pedal-down; 0 = none.
    sustain_gen: [u16; 16],
}

impl Pipeline {
    pub fn new(root: Node) -> Self {
        Self {
            decoder: Decoder::new(),
            current: Generation {
                id: 1,
                root,
                notes: 0,
                sustain: 0,
            },
            draining: ArrayVec::new(),
            counter: 1,
            note_gen: PerNote::new(),
            sustain_gen: [0; 16],
        }
    }

    /// Decode one incoming packet and run the graph. `now_ns` is the
    /// engine-monotonic timestamp in nanoseconds; effects may emit events
    /// stamped later than it.
    ///
    /// SysEx and system common packets (first byte 0xF0..=0xF7) are handed
    /// to `raw` verbatim without decoding, as are realtime bytes.
    pub fn handle(
        &mut self,
        now_ns: Timestamp,
        bytes: &[u8],
        emit: &mut impl FnMut(Event),
        raw: &mut impl FnMut(&[u8]),
    ) {
        match bytes.first() {
            None => return,
            // SysEx and system common: not modeled, pass the packet through.
            Some(&b) if (0xF0..=0xF7).contains(&b) => {
                raw(bytes);
                return;
            }
            _ => {}
        }
        for &b in bytes {
            match self.decoder.step(b) {
                Decoded::Event(kind) => self.route(now_ns, kind, emit),
                Decoded::Realtime(byte) => raw(&[byte]),
                Decoded::Pending => {}
            }
        }
    }

    /// Install a new graph built from a reloaded config.
    ///
    /// The old graph keeps draining the notes and pedals it opened. If it
    /// is already idle its flush is emitted immediately so nothing is
    /// lost; otherwise it joins the draining set, force-flushing the
    /// oldest draining graph when the set is full.
    pub fn swap_graph(&mut self, now_ns: Timestamp, root: Node, emit: &mut impl FnMut(Event)) {
        self.counter = self.counter.checked_add(1).unwrap_or(1);
        let next = Generation {
            id: self.counter,
            root,
            notes: 0,
            sustain: 0,
        };
        let mut old = std::mem::replace(&mut self.current, next);
        if old.idle() {
            flush_graph(&mut old.root, now_ns, emit);
            return;
        }
        if self.draining.is_full() {
            // Notes still pointing at the evicted graph go stale; their
            // note-offs will fall through to the current graph.
            let mut evicted = self.draining.remove(0);
            flush_graph(&mut evicted.root, now_ns, emit);
        }
        self.draining.push(old);
    }

    /// Flush every graph, draining generations first (oldest to newest),
    /// then the current one. Leaves the pipeline clean for reuse.
    pub fn shutdown(&mut self, now_ns: Timestamp, emit: &mut impl FnMut(Event)) {
        for mut old in std::mem::take(&mut self.draining) {
            flush_graph(&mut old.root, now_ns, emit);
        }
        flush_graph(&mut self.current.root, now_ns, emit);
        self.current.notes = 0;
        self.current.sustain = 0;
        self.note_gen = PerNote::new();
        self.sustain_gen = [0; 16];
        self.decoder = Decoder::new();
    }

    /// Route one decoded event to the generation that must process it.
    fn route(&mut self, now: Timestamp, kind: EventKind, emit: &mut impl FnMut(Event)) {
        match kind {
            EventKind::NoteOn { ch, key, .. } => {
                // A retrigger without a note-off releases the old owner's
                // claim; the retriggered note belongs to the current graph.
                let prev = self.note_gen.take(ch, key);
                if prev != 0 {
                    self.release_note(prev, now, emit);
                }
                self.note_gen.set(ch, key, self.current.id);
                self.current.notes += 1;
                run_graph(&mut self.current.root, now, kind, emit);
            }
            EventKind::NoteOff { ch, key, .. } => {
                let owner = self.note_gen.take(ch, key);
                self.route_to(owner, now, kind, emit);
                if owner != 0 {
                    self.release_note(owner, now, emit);
                }
            }
            EventKind::ControlChange {
                ch,
                cc: CC_SUSTAIN,
                value,
            } if value >= 64 => {
                // Pedal data can be continuous; a repeated down transfers
                // ownership to the current graph and releases the old
                // owner's claim, whose flush covers whatever the up it
                // will never see would have done.
                let prev = self.sustain_gen[ch as usize & 15];
                if prev != 0 && prev != self.current.id {
                    self.release_sustain(prev, ch, now, emit);
                }
                self.sustain_gen[ch as usize & 15] = self.current.id;
                self.current.sustain |= 1 << ch;
                run_graph(&mut self.current.root, now, kind, emit);
            }
            EventKind::ControlChange {
                ch, cc: CC_SUSTAIN, ..
            } => {
                let owner = std::mem::take(&mut self.sustain_gen[ch as usize & 15]);
                self.route_to(owner, now, kind, emit);
                if owner != 0 && owner != self.current.id {
                    // If the current graph also holds a pedal-down on this
                    // channel (unreachable under the ownership transfer
                    // above, but cheap to honor), let it see the release.
                    if self.current.sustain & (1 << ch) != 0 {
                        run_graph(&mut self.current.root, now, kind, emit);
                        self.current.sustain &= !(1 << ch);
                    }
                    self.release_sustain(owner, ch, now, emit);
                } else {
                    self.current.sustain &= !(1 << ch);
                }
            }
            // Everything else is stateless with respect to generations.
            _ => run_graph(&mut self.current.root, now, kind, emit),
        }
    }

    /// Process `kind` through the generation with the given id. A stale or
    /// missing id (evicted graph, note held from before the engine saw
    /// it) falls through to the current graph.
    fn route_to(&mut self, id: u16, now: Timestamp, kind: EventKind, emit: &mut impl FnMut(Event)) {
        let root = if id != 0 && id != self.current.id {
            match self.draining.iter_mut().find(|g| g.id == id) {
                Some(owner) => &mut owner.root,
                None => &mut self.current.root,
            }
        } else {
            &mut self.current.root
        };
        run_graph(root, now, kind, emit);
    }

    /// Drop one note from a generation's claim, retiring it if drained.
    fn release_note(&mut self, id: u16, now: Timestamp, emit: &mut impl FnMut(Event)) {
        if id == self.current.id {
            self.current.notes = self.current.notes.saturating_sub(1);
        } else if let Some(i) = self.draining.iter().position(|g| g.id == id) {
            self.draining[i].notes = self.draining[i].notes.saturating_sub(1);
            self.retire_if_idle(i, now, emit);
        }
    }

    /// Drop one channel's pedal from a generation's claim, retiring it if
    /// drained.
    fn release_sustain(&mut self, id: u16, ch: u8, now: Timestamp, emit: &mut impl FnMut(Event)) {
        if id == self.current.id {
            self.current.sustain &= !(1 << ch);
        } else if let Some(i) = self.draining.iter().position(|g| g.id == id) {
            self.draining[i].sustain &= !(1 << ch);
            self.retire_if_idle(i, now, emit);
        }
    }

    /// Flush and drop a draining generation once nothing points at it.
    fn retire_if_idle(&mut self, i: usize, now: Timestamp, emit: &mut impl FnMut(Event)) {
        if self.draining[i].idle() {
            let mut retired = self.draining.remove(i);
            flush_graph(&mut retired.root, now, emit);
        }
    }
}

/// Run one event through a graph, forwarding its outputs.
fn run_graph(root: &mut Node, now: Timestamp, kind: EventKind, emit: &mut impl FnMut(Event)) {
    let cx = ProcCx::at(now);
    let ev = Event::new(now, kind);
    let mut out = EventBuf::new();
    root.process(&ev, &mut out, &cx);
    for e in &out {
        emit(*e);
    }
}

/// Flush a graph, forwarding its outputs.
fn flush_graph(root: &mut Node, now: Timestamp, emit: &mut impl FnMut(Event)) {
    let cx = ProcCx::at(now);
    let mut out = EventBuf::new();
    root.flush(&mut out, &cx);
    for e in &out {
        emit(*e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miditool_core::graph::{Effect, Pass};

    fn pass() -> Pipeline {
        Pipeline::new(Node::Leaf(Box::new(Pass)))
    }

    /// Shifts note keys by a fixed offset and, so tests can see which
    /// graph handled the pedal, moves CC64 by the same offset in channel.
    /// On flush it emits a marker: CC `id` on channel 15.
    struct Shift {
        semis: u8,
        id: u8,
    }

    impl Effect for Shift {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, _cx: &ProcCx) {
            let kind = match ev.kind {
                EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                    ch,
                    key: key + self.semis,
                    vel,
                },
                EventKind::NoteOff { ch, key, vel } => EventKind::NoteOff {
                    ch,
                    key: key + self.semis,
                    vel,
                },
                EventKind::ControlChange {
                    ch,
                    cc: CC_SUSTAIN,
                    value,
                } => EventKind::ControlChange {
                    ch: ch + self.semis,
                    cc: CC_SUSTAIN,
                    value,
                },
                other => other,
            };
            out.push(Event::new(ev.time, kind));
        }

        fn flush(&mut self, out: &mut EventBuf, _cx: &ProcCx) {
            out.push(Event::new(
                0,
                EventKind::ControlChange {
                    ch: 15,
                    cc: self.id,
                    value: 1,
                },
            ));
        }
    }

    fn shift(semis: u8, id: u8) -> Node {
        Node::Leaf(Box::new(Shift { semis, id }))
    }

    fn flushed(id: u8) -> EventKind {
        EventKind::ControlChange {
            ch: 15,
            cc: id,
            value: 1,
        }
    }

    /// Feed packets through `handle`, collecting emitted event kinds.
    fn feed(p: &mut Pipeline, packets: &[&[u8]]) -> Vec<EventKind> {
        let mut out = Vec::new();
        for (i, packet) in packets.iter().enumerate() {
            p.handle(
                i as Timestamp,
                packet,
                &mut |ev| out.push(ev.kind),
                &mut |_| panic!("unexpected raw bytes"),
            );
        }
        out
    }

    fn swap(p: &mut Pipeline, root: Node) -> Vec<EventKind> {
        let mut out = Vec::new();
        p.swap_graph(0, root, &mut |ev| out.push(ev.kind));
        out
    }

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

    fn pedal(ch: u8, value: u8) -> EventKind {
        EventKind::ControlChange {
            ch,
            cc: CC_SUSTAIN,
            value,
        }
    }

    #[test]
    fn note_round_trip() {
        let mut p = pass();
        let out = feed(&mut p, &[&[0x90, 60, 100], &[0x80, 60, 0]]);
        assert_eq!(out, vec![on(60), off(60)]);
    }

    #[test]
    fn running_status_decodes_full_messages() {
        let mut p = pass();
        let out = feed(&mut p, &[&[0x90, 60, 100, 62, 100]]);
        assert_eq!(out, vec![on(60), on(62)]);
    }

    #[test]
    fn shutdown_flushes_held_effects() {
        /// Holds every note-on and releases it only on flush.
        struct Hold(Vec<Event>);
        impl Effect for Hold {
            fn process(&mut self, ev: &Event, out: &mut EventBuf, _cx: &ProcCx) {
                if matches!(ev.kind, EventKind::NoteOn { .. }) {
                    self.0.push(*ev);
                    out.push(*ev);
                }
            }
            fn flush(&mut self, out: &mut EventBuf, _cx: &ProcCx) {
                for ev in self.0.drain(..) {
                    if let EventKind::NoteOn { ch, key, .. } = ev.kind {
                        out.push(Event::new(ev.time, EventKind::NoteOff { ch, key, vel: 0 }));
                    }
                }
            }
        }
        let mut p = Pipeline::new(Node::Leaf(Box::new(Hold(Vec::new()))));
        feed(&mut p, &[&[0x91, 72, 100]]);
        let mut out = Vec::new();
        p.shutdown(1, &mut |ev| out.push(ev.kind));
        assert_eq!(
            out,
            vec![EventKind::NoteOff {
                ch: 1,
                key: 72,
                vel: 0
            }]
        );
    }

    #[test]
    fn sysex_passes_through_verbatim() {
        let mut p = pass();
        let mut raw = Vec::new();
        p.handle(0, &[0xF0, 1, 2, 3, 0xF7], &mut |_| panic!(), &mut |b| {
            raw.push(b.to_vec())
        });
        assert_eq!(raw, vec![vec![0xF0, 1, 2, 3, 0xF7]]);
    }

    #[test]
    fn realtime_passes_through_verbatim() {
        let mut p = pass();
        let mut raw = Vec::new();
        p.handle(0, &[0xF8], &mut |_| panic!(), &mut |b| raw.push(b.to_vec()));
        assert_eq!(raw, vec![vec![0xF8]]);
    }

    #[test]
    fn realtime_interleaved_in_a_note_packet() {
        let mut p = pass();
        let mut out = Vec::new();
        let mut raw = Vec::new();
        p.handle(
            0,
            &[0x90, 60, 0xF8, 100],
            &mut |ev| out.push(ev.kind),
            &mut |b| raw.push(b.to_vec()),
        );
        assert_eq!(raw, vec![vec![0xF8]]);
        assert_eq!(out, vec![on(60)]);
    }

    #[test]
    fn note_off_routes_through_the_old_graph() {
        let mut p = Pipeline::new(shift(2, 1));
        assert_eq!(feed(&mut p, &[&[0x90, 60, 100]]), vec![on(62)]);
        assert!(swap(&mut p, shift(5, 2)).is_empty());
        // The held note goes off where graph 1 put it, a fresh note maps
        // through graph 2, and graph 1 flushes once it is empty.
        let out = feed(&mut p, &[&[0x80, 60, 0], &[0x90, 61, 100]]);
        assert_eq!(out, vec![off(62), flushed(1), on(66)]);
    }

    #[test]
    fn swap_with_no_held_state_flushes_immediately() {
        let mut p = Pipeline::new(shift(2, 1));
        assert_eq!(swap(&mut p, shift(5, 2)), vec![flushed(1)]);
    }

    #[test]
    fn retrigger_releases_the_old_graphs_claim() {
        let mut p = Pipeline::new(shift(2, 1));
        feed(&mut p, &[&[0x90, 60, 100]]);
        swap(&mut p, shift(5, 2));
        // Re-striking the held key transfers it to the new graph; the old
        // graph has nothing left and flushes.
        let out = feed(&mut p, &[&[0x90, 60, 100]]);
        assert_eq!(out, vec![flushed(1), on(65)]);
        assert_eq!(feed(&mut p, &[&[0x80, 60, 0]]), vec![off(65)]);
    }

    #[test]
    fn pedal_up_routes_through_the_old_graph() {
        let mut p = Pipeline::new(shift(2, 1));
        assert_eq!(feed(&mut p, &[&[0xB0, 64, 127]]), vec![pedal(2, 127)]);
        swap(&mut p, shift(5, 2));
        // The release lands on graph 1's channel, then graph 1 retires.
        let out = feed(&mut p, &[&[0xB0, 64, 0]]);
        assert_eq!(out, vec![pedal(2, 0), flushed(1)]);
    }

    #[test]
    fn fourth_swap_evicts_the_oldest_graph() {
        let mut p = Pipeline::new(shift(1, 1));
        feed(&mut p, &[&[0x90, 60, 100]]);
        // Each of graphs 2..=4 takes a held note, sending 1..=3 draining.
        for id in [2, 3, 4] {
            assert!(swap(&mut p, shift(id, id)).is_empty());
            feed(&mut p, &[&[0x90, 60 + id, 100]]);
        }
        // Three graphs are draining; the next swap force-flushes graph 1.
        assert_eq!(swap(&mut p, shift(9, 9)), vec![flushed(1)]);
        // Graph 1's note went stale, so its off falls through to the
        // current graph; the others still drain through their own graphs.
        assert_eq!(feed(&mut p, &[&[0x80, 60, 0]]), vec![off(69)]);
        assert_eq!(feed(&mut p, &[&[0x80, 62, 0]]), vec![off(64), flushed(2)]);
    }

    #[test]
    fn shutdown_flushes_draining_and_current() {
        let mut p = Pipeline::new(shift(2, 1));
        feed(&mut p, &[&[0x90, 60, 100]]);
        swap(&mut p, shift(5, 2));
        let mut out = Vec::new();
        p.shutdown(1, &mut |ev| out.push(ev.kind));
        assert_eq!(out, vec![flushed(1), flushed(2)]);
    }
}
