//! The realtime run loop: decode incoming packets, run the effect graph,
//! encode and send the results, and track what is sounding so shutdown
//! leaves no hanging notes.
//!
//! The hot path is [`Pipeline`], a pure function of bytes in to bytes out
//! with no I/O dependencies, so it is fully testable without hardware.
//! [`Engine`] is the thin layer that wires a pipeline between real MIDI
//! ports: the pipeline and the output connection are moved into the input
//! callback outright, so the per-event path takes no locks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use miditool_core::event::{CC_ALL_NOTES_OFF, CC_ALL_SOUND_OFF, CC_RESET_CONTROLLERS};
use miditool_core::wire::{self, Decoded, Decoder};
use miditool_core::{Event, EventBuf, EventKind, Node, NoteTracker, ProcCx, Timestamp};
use miditool_io::{Input, IoError, Output, OutputTarget};
use thiserror::Error;

/// Errors from engine setup and teardown. The per-event path reports
/// nothing: a failed send mid-stream has no one to tell.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Io(#[from] IoError),
}

/// The pure hot path: decoder, effect graph, and boundary note tracker.
///
/// Callers feed it raw input packets plus a monotonic timestamp and give
/// it a `send` sink for outgoing wire bytes. Nothing here allocates or
/// blocks per event.
pub struct Pipeline {
    decoder: Decoder,
    root: Node,
    tracker: NoteTracker,
}

impl Pipeline {
    pub fn new(root: Node) -> Self {
        Self {
            decoder: Decoder::new(),
            root,
            tracker: NoteTracker::new(),
        }
    }

    /// Feed one incoming packet. `now_ns` is the engine-monotonic
    /// timestamp in nanoseconds.
    ///
    /// Channel voice messages are decoded, run through the graph, and the
    /// results encoded and handed to `send`. SysEx and system common
    /// packets (first byte 0xF0..=0xF7) are forwarded verbatim without
    /// decoding, as are realtime bytes. Every channel event actually sent
    /// is observed by the note tracker.
    pub fn handle(&mut self, now_ns: Timestamp, bytes: &[u8], send: &mut impl FnMut(&[u8])) {
        match bytes.first() {
            None => return,
            // SysEx and system common: not modeled, pass the packet through.
            Some(&b) if (0xF0..=0xF7).contains(&b) => {
                send(bytes);
                return;
            }
            _ => {}
        }
        // Destructure so the decoder can borrow mutably alongside the rest.
        let Self {
            decoder,
            root,
            tracker,
        } = self;
        let cx = ProcCx::at(now_ns);
        let mut buf = [0u8; 3];
        decoder.feed(bytes, |decoded| match decoded {
            Decoded::Event(kind) => {
                let ev = Event::new(now_ns, kind);
                let mut out = EventBuf::new();
                root.process(&ev, &mut out, &cx);
                for e in &out {
                    send(wire::encode(&e.kind, &mut buf));
                    tracker.observe(&e.kind);
                }
            }
            Decoded::Realtime(byte) => send(&[byte]),
            Decoded::Pending => {}
        });
    }

    /// Flush all effects, then silence the tracker, sending the resulting
    /// bytes. Leaves the pipeline clean for reuse.
    pub fn shutdown(&mut self, now_ns: Timestamp, send: &mut impl FnMut(&[u8])) {
        let cx = ProcCx::at(now_ns);
        let mut buf = [0u8; 3];
        let mut out = EventBuf::new();
        self.root.flush(&mut out, &cx);
        for e in &out {
            send(wire::encode(&e.kind, &mut buf));
            self.tracker.observe(&e.kind);
        }
        out.clear();
        self.tracker.silence(now_ns, &mut out);
        for e in &out {
            send(wire::encode(&e.kind, &mut buf));
        }
    }

    /// Emergency stop: silence everything the tracker has seen, then send
    /// All Notes Off, All Sound Off, and Reset All Controllers on all 16
    /// channels for anything the tracker could not know about.
    pub fn panic(&mut self, now_ns: Timestamp, send: &mut impl FnMut(&[u8])) {
        let mut buf = [0u8; 3];
        let mut out = EventBuf::new();
        self.tracker.silence(now_ns, &mut out);
        for e in &out {
            send(wire::encode(&e.kind, &mut buf));
        }
        for ch in 0..16 {
            for cc in [CC_ALL_NOTES_OFF, CC_ALL_SOUND_OFF, CC_RESET_CONTROLLERS] {
                let kind = EventKind::ControlChange { ch, cc, value: 0 };
                send(wire::encode(&kind, &mut buf));
            }
        }
    }
}

/// State owned by the MIDI input callback thread.
type Owned = (Pipeline, Output);

/// A running engine: one input port, one pipeline, one output.
///
/// Construct with [`Engine::run`]; stop cleanly with [`Engine::stop`].
/// Dropping a running engine performs the same flush-and-silence sequence,
/// ignoring errors.
pub struct Engine {
    input: Option<Input<Owned>>,
    stop: Arc<AtomicBool>,
    started: Instant,
}

impl Engine {
    /// Open the output, build a pipeline around `root`, and connect it to
    /// the chosen input port. Processing starts immediately on the
    /// backend's MIDI thread.
    ///
    /// `input` selects the source port as in [`miditool_io::open_input`]:
    /// a case-insensitive substring, or `None` to auto-pick.
    pub fn run(
        input: Option<&str>,
        output: &OutputTarget,
        root: Node,
    ) -> Result<Engine, EngineError> {
        let out = miditool_io::open_output(output)?;
        let pipeline = Pipeline::new(root);
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let flag = Arc::clone(&stop);
        let input = miditool_io::open_input_with(
            input,
            move |_stamp, bytes, owned: &mut Owned| {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                let (pipeline, out) = owned;
                let now_ns = started.elapsed().as_nanos() as Timestamp;
                pipeline.handle(now_ns, bytes, &mut |b| {
                    // Nowhere to report a failed send from the MIDI thread.
                    let _ = out.send(b);
                });
            },
            (pipeline, out),
        )?;
        Ok(Engine {
            input: Some(input),
            stop,
            started,
        })
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
        // Stop feeding the pipeline, then disconnect. `close` blocks until
        // the callback cannot run again, so we own the state exclusively.
        self.stop.store(true, Ordering::Relaxed);
        let (mut pipeline, mut out) = input.close();
        let now_ns = self.started.elapsed().as_nanos() as Timestamp;
        let mut first_err: Option<IoError> = None;
        pipeline.shutdown(now_ns, &mut |b| {
            if let Err(e) = out.send(b) {
                first_err.get_or_insert(e);
            }
        });
        match first_err {
            Some(e) => Err(e.into()),
            None => Ok(()),
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.wind_down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miditool_core::graph::{Effect, Pass};

    fn pass() -> Pipeline {
        Pipeline::new(Node::Leaf(Box::new(Pass)))
    }

    /// Feed packets through `handle` and collect each sent message.
    fn feed(p: &mut Pipeline, packets: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut sent = Vec::new();
        for (i, packet) in packets.iter().enumerate() {
            p.handle(i as Timestamp, packet, &mut |b| sent.push(b.to_vec()));
        }
        sent
    }

    #[test]
    fn note_round_trip() {
        let mut p = pass();
        let sent = feed(&mut p, &[&[0x90, 60, 100], &[0x80, 60, 0]]);
        assert_eq!(sent, vec![vec![0x90, 60, 100], vec![0x80, 60, 0]]);
    }

    #[test]
    fn running_status_reencodes_full_messages() {
        let mut p = pass();
        let sent = feed(&mut p, &[&[0x90, 60, 100, 62, 90]]);
        assert_eq!(sent, vec![vec![0x90, 60, 100], vec![0x90, 62, 90]]);
    }

    #[test]
    fn shutdown_releases_hanging_note() {
        let mut p = pass();
        feed(&mut p, &[&[0x90, 60, 100]]);
        let mut sent = Vec::new();
        p.shutdown(1, &mut |b| sent.push(b.to_vec()));
        assert!(sent.contains(&vec![0x80, 60, 0]));
    }

    #[test]
    fn shutdown_after_balanced_notes_sends_nothing() {
        let mut p = pass();
        feed(&mut p, &[&[0x90, 60, 100], &[0x80, 60, 0]]);
        let mut sent = Vec::new();
        p.shutdown(1, &mut |b| sent.push(b.to_vec()));
        assert!(sent.is_empty());
    }

    #[test]
    fn shutdown_flushes_effects_before_silencing() {
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
        let mut sent = Vec::new();
        p.shutdown(1, &mut |b| sent.push(b.to_vec()));
        // The flush's note-off both goes out and settles the tracker, so
        // the note-off appears exactly once.
        assert_eq!(sent, vec![vec![0x81, 72, 0]]);
    }

    #[test]
    fn panic_emits_channel_mode_messages_on_all_channels() {
        let mut p = pass();
        feed(&mut p, &[&[0x90, 60, 100]]);
        let mut sent = Vec::new();
        p.panic(1, &mut |b| sent.push(b.to_vec()));
        assert!(sent.contains(&vec![0x80, 60, 0]));
        for ch in 0..16u8 {
            for cc in [123, 120, 121] {
                assert!(sent.contains(&vec![0xB0 | ch, cc, 0]));
            }
        }
    }

    #[test]
    fn sysex_passes_through_verbatim() {
        let mut p = pass();
        let sent = feed(&mut p, &[&[0xF0, 1, 2, 3, 0xF7]]);
        assert_eq!(sent, vec![vec![0xF0, 1, 2, 3, 0xF7]]);
    }

    #[test]
    fn realtime_passes_through_verbatim() {
        let mut p = pass();
        let sent = feed(&mut p, &[&[0xF8]]);
        assert_eq!(sent, vec![vec![0xF8]]);
    }

    #[test]
    fn realtime_interleaved_in_a_note_packet() {
        let mut p = pass();
        let sent = feed(&mut p, &[&[0x90, 60, 0xF8, 100]]);
        assert_eq!(sent, vec![vec![0xF8], vec![0x90, 60, 100]]);
    }
}
