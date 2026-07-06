//! The composable effect graph: chains, forks, filters, and leaf effects.
//!
//! The model follows mididings: `Chain` runs children in series (an effect
//! that emits nothing swallows the event), `Fork` runs children in parallel
//! on a copy of the event and merges their outputs in order, dropping exact
//! duplicates so `fork { pass; transpose 12 }` doubles notes without
//! doubling everything else.

use std::sync::atomic::{AtomicU64, Ordering};

use arrayvec::ArrayVec;

use crate::event::{Event, EventKind, Timestamp};

/// Upper bound on how many events one input event may fan out into at any
/// point in the graph. Overflow drops events and increments a counter
/// rather than blocking or allocating.
pub const MAX_FANOUT: usize = 128;

/// Fixed-capacity output buffer handed to effects.
pub type EventBuf = ArrayVec<Event, MAX_FANOUT>;

/// Per-process context passed down the graph.
#[derive(Debug, Default)]
pub struct ProcCx {
    pub now: Timestamp,
    /// Events dropped because a buffer was full. Diagnostic only.
    pub dropped: AtomicU64,
}

impl ProcCx {
    pub fn at(now: Timestamp) -> Self {
        Self {
            now,
            dropped: AtomicU64::new(0),
        }
    }

    fn push(&self, buf: &mut EventBuf, ev: Event) {
        if buf.try_push(ev).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Append two events atomically: both or neither. With fewer than two
    /// slots left, both count as dropped. Effects that emit self-contained
    /// note-on/note-off pairs (restrike, stutter) push them through here so
    /// buffer truncation can never keep the on and drop the off, which
    /// would leave the note stuck.
    pub fn push_pair(&self, buf: &mut EventBuf, a: Event, b: Event) {
        if buf.remaining_capacity() >= 2 {
            buf.push(a);
            buf.push(b);
        } else {
            self.dropped.fetch_add(2, Ordering::Relaxed);
        }
    }
}

/// A stateful event transformer. Implementations must be realtime-safe in
/// `process` and `flush`: no allocation, locking, or blocking.
pub trait Effect: Send {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx);

    /// Called on the engine's steady cadence (every few milliseconds)
    /// whether or not input arrives. Free-running effects (loops,
    /// walkers, swarms) emit from here; the default does nothing.
    /// Implementations quantize their own musical time against `now`
    /// rather than assuming a fixed cadence.
    fn tick(&mut self, _now: Timestamp, _out: &mut EventBuf, _cx: &ProcCx) {}

    /// Emit whatever is needed to wind down cleanly (typically note-offs
    /// for anything the effect still has sounding). Called on shutdown and,
    /// later, on scene switches and hot reloads.
    fn flush(&mut self, _out: &mut EventBuf, _cx: &ProcCx) {}
}

/// Event predicates. Filters gate only the event classes they understand:
/// a key-range filter passes controllers untouched, a velocity filter
/// gates only note-ons. This keeps pedal and bend data flowing when a
/// chain is narrowed to a slice of the keyboard.
#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    /// Bitmask over channels 0..=15.
    Channels(u16),
    /// Inclusive key range; gates note and poly-pressure events only.
    KeyRange {
        lo: u8,
        hi: u8,
    },
    /// Inclusive velocity range; gates note-ons only.
    VelocityRange {
        lo: u8,
        hi: u8,
    },
    /// Pass only note events (and poly pressure).
    NotesOnly,
    /// Pass only controller events.
    ControllersOnly,
    Not(Box<Filter>),
}

impl Filter {
    pub fn passes(&self, kind: &EventKind) -> bool {
        match self {
            Filter::Channels(mask) => mask & (1 << kind.channel()) != 0,
            Filter::KeyRange { lo, hi } => kind.key().is_none_or(|k| (*lo..=*hi).contains(&k)),
            Filter::VelocityRange { lo, hi } => match kind {
                EventKind::NoteOn { vel, .. } => (*lo..=*hi).contains(vel),
                _ => true,
            },
            Filter::NotesOnly => {
                matches!(
                    kind,
                    EventKind::NoteOn { .. }
                        | EventKind::NoteOff { .. }
                        | EventKind::PolyPressure { .. }
                )
            }
            Filter::ControllersOnly => matches!(kind, EventKind::ControlChange { .. }),
            Filter::Not(inner) => !inner.passes(kind),
        }
    }
}

/// A node in the compiled effect graph.
pub enum Node {
    Chain(Vec<Node>),
    Fork(Vec<Node>),
    Filter(Filter),
    Leaf(Box<dyn Effect>),
}

impl Node {
    /// Run one event through this node, appending outputs to `out`.
    pub fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        match self {
            Node::Leaf(effect) => effect.process(ev, out, cx),
            Node::Filter(filter) => {
                if filter.passes(&ev.kind) {
                    cx.push(out, *ev);
                }
            }
            Node::Chain(children) => {
                let mut cur = EventBuf::new();
                cx.push(&mut cur, *ev);
                let mut next = EventBuf::new();
                for child in children {
                    next.clear();
                    for e in &cur {
                        child.process(e, &mut next, cx);
                    }
                    std::mem::swap(&mut cur, &mut next);
                }
                for e in &cur {
                    cx.push(out, *e);
                }
            }
            Node::Fork(children) => {
                let start = out.len();
                for child in children {
                    let mark = out.len();
                    child.process(ev, out, cx);
                    dedup_new(out, start, mark);
                }
            }
        }
    }

    /// Advance free-running effects. A child's tick output flows through
    /// the REST of its chain (a generator's notes still pass downstream
    /// transforms), and fork branches merge with the same ordered-union
    /// dedup as `process`.
    pub fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
        match self {
            Node::Leaf(effect) => effect.tick(now, out, cx),
            Node::Filter(_) => {}
            Node::Chain(children) => {
                // acc holds events still owed to the remaining children:
                // each child first processes what upstream produced, then
                // appends its own tick output for the children after it.
                let mut acc = EventBuf::new();
                let mut next = EventBuf::new();
                for child in children {
                    next.clear();
                    for e in &acc {
                        child.process(e, &mut next, cx);
                    }
                    child.tick(now, &mut next, cx);
                    std::mem::swap(&mut acc, &mut next);
                }
                for e in &acc {
                    cx.push(out, *e);
                }
            }
            Node::Fork(children) => {
                let start = out.len();
                for child in children {
                    let mark = out.len();
                    child.tick(now, out, cx);
                    dedup_new(out, start, mark);
                }
            }
        }
    }

    /// Flush every effect in the subtree.
    pub fn flush(&mut self, out: &mut EventBuf, cx: &ProcCx) {
        match self {
            Node::Leaf(effect) => effect.flush(out, cx),
            Node::Filter(_) => {}
            // Chains flush back to front so a downstream effect sees no
            // more input after it has flushed.
            Node::Chain(children) | Node::Fork(children) => {
                for child in children.iter_mut().rev() {
                    child.flush(out, cx);
                }
            }
        }
    }
}

/// Remove events in `out[mark..]` that already appear in
/// `out[start..mark]`: parallel branches emitting the identical event
/// produce it once (the mididings merge rule). Whole events are compared,
/// time included: a branch that re-emits an earlier branch's event at a
/// later time (a delay, an echo tail) is producing a distinct event, not
/// a duplicate, or the copy's note-off would vanish and the note stick.
fn dedup_new(out: &mut EventBuf, start: usize, mark: usize) {
    let mut i = mark;
    while i < out.len() {
        let duplicate = out[start..mark].iter().any(|e| *e == out[i]);
        if duplicate {
            out.remove(i);
        } else {
            i += 1;
        }
    }
}

/// The identity effect, useful as a fork branch.
pub struct Pass;

impl Effect for Pass {
    fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
        cx.push(out, *ev);
    }
}

/// Discard everything, useful for muting a branch.
pub struct Discard;

impl Effect for Discard {
    fn process(&mut self, _ev: &Event, _out: &mut EventBuf, _cx: &ProcCx) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(key: u8) -> Event {
        Event::new(
            0,
            EventKind::NoteOn {
                ch: 0,
                key,
                vel: 100,
            },
        )
    }

    struct AddSemitones(i16);

    impl Effect for AddSemitones {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
            let kind = match ev.kind {
                EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                    ch,
                    key: (key as i16 + self.0).clamp(0, 127) as u8,
                    vel,
                },
                other => other,
            };
            cx.push(out, Event::new(ev.time, kind));
        }
    }

    fn run(node: &mut Node, ev: Event) -> Vec<EventKind> {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        node.process(&ev, &mut out, &cx);
        out.iter().map(|e| e.kind).collect()
    }

    #[test]
    fn chain_composes_in_series() {
        let mut node = Node::Chain(vec![
            Node::Leaf(Box::new(AddSemitones(2))),
            Node::Leaf(Box::new(AddSemitones(3))),
        ]);
        assert_eq!(
            run(&mut node, note_on(60)),
            vec![EventKind::NoteOn {
                ch: 0,
                key: 65,
                vel: 100
            }]
        );
    }

    #[test]
    fn fork_merges_in_branch_order() {
        let mut node = Node::Fork(vec![
            Node::Leaf(Box::new(Pass)),
            Node::Leaf(Box::new(AddSemitones(12))),
        ]);
        assert_eq!(
            run(&mut node, note_on(60)),
            vec![
                EventKind::NoteOn {
                    ch: 0,
                    key: 60,
                    vel: 100
                },
                EventKind::NoteOn {
                    ch: 0,
                    key: 72,
                    vel: 100
                },
            ]
        );
    }

    #[test]
    fn fork_dedups_identical_outputs() {
        // Both branches pass controllers through untouched; the pedal event
        // must come out once, not twice.
        let mut node = Node::Fork(vec![
            Node::Leaf(Box::new(Pass)),
            Node::Leaf(Box::new(AddSemitones(12))),
        ]);
        let pedal = Event::new(
            0,
            EventKind::ControlChange {
                ch: 0,
                cc: 64,
                value: 127,
            },
        );
        assert_eq!(
            run(&mut node, pedal),
            vec![EventKind::ControlChange {
                ch: 0,
                cc: 64,
                value: 127
            }]
        );
    }

    /// Re-emits every event shifted `.0` nanoseconds later.
    struct DelayBy(u64);

    impl Effect for DelayBy {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
            cx.push(out, Event::new(ev.time + self.0, ev.kind));
        }
    }

    /// A minimal echo: passes the event, then repeats each note twice at
    /// 1000ns spacing with halved note-on velocity, like `echo decay 0.5`.
    struct EchoTwice;

    impl Effect for EchoTwice {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
            cx.push(out, *ev);
            for k in 1..=2u64 {
                let kind = match ev.kind {
                    EventKind::NoteOn { ch, key, vel } => EventKind::NoteOn {
                        ch,
                        key,
                        vel: (vel >> k).max(1),
                    },
                    EventKind::NoteOff { .. } => ev.kind,
                    _ => return,
                };
                cx.push(out, Event::new(ev.time + k * 1_000, kind));
            }
        }
    }

    fn run_timed(node: &mut Node, ev: Event) -> Vec<Event> {
        let cx = ProcCx::at(ev.time);
        let mut out = EventBuf::new();
        node.process(&ev, &mut out, &cx);
        out.iter().copied().collect()
    }

    #[test]
    fn fork_keeps_a_delayed_copy_of_the_same_kind() {
        // The delayed branch re-emits the pass branch's note-off with a
        // later time: same kind, distinct event. Both must survive, or the
        // delayed note never ends.
        let mut node = Node::Fork(vec![
            Node::Leaf(Box::new(Pass)),
            Node::Leaf(Box::new(DelayBy(500))),
        ]);
        let off = EventKind::NoteOff {
            ch: 0,
            key: 60,
            vel: 0,
        };
        assert_eq!(
            run_timed(&mut node, Event::new(100, off)),
            vec![Event::new(100, off), Event::new(600, off)]
        );
    }

    #[test]
    fn fork_pass_echo_stays_balanced_per_note() {
        // fork { pass; echo }: the echo's note-off copies share the pass
        // branch's kind but land later; deduping them would leave the
        // decayed copies sounding forever.
        let mut node = Node::Fork(vec![
            Node::Leaf(Box::new(Pass)),
            Node::Leaf(Box::new(EchoTwice)),
        ]);
        let mut all = run_timed(&mut node, note_on(60));
        all.extend(run_timed(
            &mut node,
            Event::new(
                500,
                EventKind::NoteOff {
                    ch: 0,
                    key: 60,
                    vel: 0,
                },
            ),
        ));
        let mut net = 0i32;
        for e in &all {
            match e.kind {
                EventKind::NoteOn { ch: 0, key: 60, .. } => net += 1,
                EventKind::NoteOff { ch: 0, key: 60, .. } => net -= 1,
                other => panic!("unexpected event {other:?}"),
            }
        }
        assert_eq!(net, 0, "unbalanced note-ons/offs: {all:?}");
        // The original on and off each appear once (deduped across the
        // branches), the two decayed copies bring their own offs.
        assert_eq!(all.len(), 6, "events: {all:?}");
    }

    #[test]
    fn filter_swallows_in_chain() {
        let mut node = Node::Chain(vec![
            Node::Filter(Filter::KeyRange { lo: 0, hi: 59 }),
            Node::Leaf(Box::new(AddSemitones(12))),
        ]);
        assert_eq!(run(&mut node, note_on(60)), vec![]);
        assert_eq!(
            run(&mut node, note_on(59)),
            vec![EventKind::NoteOn {
                ch: 0,
                key: 71,
                vel: 100
            }]
        );
    }

    #[test]
    fn key_filter_passes_non_note_events() {
        let mut node = Node::Filter(Filter::KeyRange { lo: 0, hi: 59 });
        let pedal = Event::new(
            0,
            EventKind::ControlChange {
                ch: 0,
                cc: 64,
                value: 127,
            },
        );
        assert_eq!(run(&mut node, pedal), vec![pedal.kind]);
    }

    #[test]
    fn not_filter_inverts() {
        let f = Filter::Not(Box::new(Filter::Channels(1 << 0)));
        assert!(!f.passes(&EventKind::NoteOn {
            ch: 0,
            key: 60,
            vel: 1
        }));
        assert!(f.passes(&EventKind::NoteOn {
            ch: 1,
            key: 60,
            vel: 1
        }));
    }

    /// A leaf that emits one fixed note-on per tick.
    struct Ticker(u8);

    impl Effect for Ticker {
        fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
            cx.push(out, *ev);
        }

        fn tick(&mut self, now: Timestamp, out: &mut EventBuf, cx: &ProcCx) {
            cx.push(
                out,
                Event::new(
                    now,
                    EventKind::NoteOn {
                        ch: 0,
                        key: self.0,
                        vel: 100,
                    },
                ),
            );
        }
    }

    #[test]
    fn chain_tick_flows_through_downstream_children() {
        let mut node = Node::Chain(vec![
            Node::Leaf(Box::new(Ticker(60))),
            Node::Leaf(Box::new(AddSemitones(12))),
        ]);
        let cx = ProcCx::at(7);
        let mut out = EventBuf::new();
        node.tick(7, &mut out, &cx);
        assert_eq!(
            out.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![EventKind::NoteOn {
                ch: 0,
                key: 72,
                vel: 100
            }],
            "the ticker's note passes through the downstream transpose"
        );
    }

    #[test]
    fn chain_tick_does_not_feed_upstream_children() {
        let mut node = Node::Chain(vec![
            Node::Leaf(Box::new(AddSemitones(12))),
            Node::Leaf(Box::new(Ticker(60))),
        ]);
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        node.tick(0, &mut out, &cx);
        assert_eq!(
            out.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![EventKind::NoteOn {
                ch: 0,
                key: 60,
                vel: 100
            }],
            "a later child's tick output is not run through earlier children"
        );
    }

    #[test]
    fn fork_tick_merges_with_dedup() {
        let mut node = Node::Fork(vec![
            Node::Leaf(Box::new(Ticker(60))),
            Node::Leaf(Box::new(Ticker(60))),
            Node::Leaf(Box::new(Ticker(64))),
        ]);
        let cx = ProcCx::at(3);
        let mut out = EventBuf::new();
        node.tick(3, &mut out, &cx);
        assert_eq!(out.len(), 2, "identical tick emissions merge");
    }

    #[test]
    fn overflow_drops_and_counts() {
        struct Exploder;
        impl Effect for Exploder {
            fn process(&mut self, ev: &Event, out: &mut EventBuf, cx: &ProcCx) {
                for _ in 0..(MAX_FANOUT + 10) {
                    cx.push(out, *ev);
                }
            }
        }
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        let mut node = Node::Leaf(Box::new(Exploder));
        node.process(&note_on(60), &mut out, &cx);
        assert_eq!(out.len(), MAX_FANOUT);
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn push_pair_is_all_or_nothing() {
        let cx = ProcCx::at(0);
        let mut out = EventBuf::new();
        for _ in 0..MAX_FANOUT - 3 {
            out.push(note_on(1));
        }
        // Three slots left: the first pair fits, the second must not be
        // split across the last slot.
        cx.push_pair(&mut out, note_on(60), note_on(61));
        assert_eq!(out.len(), MAX_FANOUT - 1);
        cx.push_pair(&mut out, note_on(62), note_on(63));
        assert_eq!(out.len(), MAX_FANOUT - 1);
        assert_eq!(out.last(), Some(&note_on(61)));
        assert_eq!(cx.dropped.load(Ordering::Relaxed), 2);
    }
}
