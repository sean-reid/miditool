//! End-to-end test over real virtual MIDI ports: a fake keyboard feeds the
//! engine, and a capture port plays the DAW's role. macOS only for now;
//! Linux CI runners have no sequencer device by default.

#![cfg(target_os = "macos")]

use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use miditool_core::graph::Node;
use miditool_effects::Transpose;
use miditool_engine::Engine;
use miditool_io::{OutputTarget, open_input, open_output};

/// Wait until `pred` is true or a timeout expires. Virtual port plumbing is
/// asynchronous in CoreMIDI, so polling beats fixed sleeps.
fn wait_for(mut pred: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn transposes_notes_end_to_end() {
    // A virtual source playing the keyboard's part.
    let mut keyboard = open_output(&OutputTarget::Virtual("miditool test kb".into()))
        .expect("create fake keyboard");

    // The engine bridges the fake keyboard to its own virtual output.
    let root = Node::Leaf(Box::new(Transpose::new(12)));
    let engine = Engine::run(
        Some("miditool test kb"),
        &OutputTarget::Virtual("miditool test out".into()),
        root,
        None,
    )
    .expect("start engine");

    // A capture connection playing the DAW's part.
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let sink = Arc::clone(&received);
    let _capture = open_input(Some("miditool test out"), move |_stamp, bytes| {
        sink.lock().unwrap().push(bytes.to_vec());
    })
    .expect("open capture port");

    // Give CoreMIDI a moment to finish the connections, then play.
    sleep(Duration::from_millis(300));
    keyboard.send(&[0x90, 60, 100]).unwrap();
    keyboard.send(&[0x80, 60, 0]).unwrap();

    assert!(
        wait_for(|| received.lock().unwrap().len() >= 2),
        "expected 2 messages, got {:?}",
        received.lock().unwrap()
    );
    {
        let msgs = received.lock().unwrap();
        assert_eq!(
            msgs[0],
            vec![0x90, 72, 100],
            "note-on transposed up an octave"
        );
        assert_eq!(msgs[1], vec![0x80, 72, 0], "note-off follows the note-on");
    }

    // A hanging note must be released by the engine's wind-down.
    keyboard.send(&[0x90, 64, 90]).unwrap();
    assert!(wait_for(|| received.lock().unwrap().len() >= 3));
    engine.stop().expect("stop engine");
    assert!(
        wait_for(|| {
            received
                .lock()
                .unwrap()
                .iter()
                .any(|m| m[0] & 0xF0 == 0x80 && m[1] == 76)
        }),
        "wind-down should release the transposed hanging note, got {:?}",
        received.lock().unwrap()
    );
}

#[test]
fn echoes_arrive_on_schedule() {
    let mut keyboard = open_output(&OutputTarget::Virtual("miditool echo kb".into()))
        .expect("create fake keyboard");

    // Two echoes 60ms apart, full velocity so the copies are identical.
    let root = Node::Leaf(Box::new(miditool_effects::Echo::new(2, 60_000_000, 1.0, 0)));
    let engine = Engine::run(
        Some("miditool echo kb"),
        &OutputTarget::Virtual("miditool echo out".into()),
        root,
        None,
    )
    .expect("start engine");

    type Arrivals = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;
    let received: Arrivals = Arc::default();
    let sink = Arc::clone(&received);
    let _capture = open_input(Some("miditool echo out"), move |_stamp, bytes| {
        sink.lock().unwrap().push((Instant::now(), bytes.to_vec()));
    })
    .expect("open capture port");

    sleep(Duration::from_millis(300));
    keyboard.send(&[0x90, 60, 100]).unwrap();
    keyboard.send(&[0x80, 60, 0]).unwrap();

    // Original plus two echoes, on and off: six messages.
    assert!(
        wait_for(|| received.lock().unwrap().len() >= 6),
        "expected 6 messages, got {:?}",
        received
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b.clone())
            .collect::<Vec<_>>()
    );
    let msgs = received.lock().unwrap();
    let ons: Vec<_> = msgs.iter().filter(|(_, b)| b[0] == 0x90).collect();
    assert_eq!(ons.len(), 3, "one original and two echoed note-ons");
    let gap1 = ons[1].0.duration_since(ons[0].0).as_millis();
    let gap2 = ons[2].0.duration_since(ons[1].0).as_millis();
    for gap in [gap1, gap2] {
        assert!(
            (40..=80).contains(&gap),
            "echo gap should be near 60ms, got {gap}ms"
        );
    }
    drop(msgs);
    engine.stop().expect("stop engine");
}
