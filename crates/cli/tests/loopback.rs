//! End-to-end test over real virtual MIDI ports: a fake keyboard feeds the
//! engine, and a capture port plays the DAW's role. macOS only for now;
//! Linux CI runners have no sequencer device by default.

#![cfg(target_os = "macos")]

use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use miditool_core::graph::Node;
use miditool_effects::Transpose;
use miditool_engine::{Engine, SceneDef};
use miditool_io::{Output, OutputTarget, open_input, open_output};

fn one_scene() -> Vec<SceneDef> {
    vec![SceneDef {
        name: "test".to_owned(),
        kill_on_exit: false,
    }]
}

/// Block until the keyboard-to-capture path is live. CoreMIDI wires
/// virtual ports up asynchronously, so a probe note is sent (and re-sent
/// every 200ms, up to 5s) until `seen` reports an arrival; a fixed sleep
/// would be either flaky or slow. Callers clear their capture buffer
/// afterwards so the probe does not pollute the real assertions.
fn wait_until_live(keyboard: &mut Output, seen: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        keyboard.send(&[0x90, 0, 1]).expect("send probe note-on");
        keyboard.send(&[0x80, 0, 0]).expect("send probe note-off");
        let retry = Instant::now() + Duration::from_millis(200);
        while Instant::now() < retry {
            if seen() {
                // Give the probe's partner message a moment to land so
                // the caller's clear wipes both.
                sleep(Duration::from_millis(250));
                return;
            }
            sleep(Duration::from_millis(10));
        }
        assert!(
            Instant::now() < deadline,
            "the loopback never became live: no probe note arrived in 5s"
        );
    }
}

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
    let (engine, _handle) = Engine::run(
        Some("miditool test kb"),
        &OutputTarget::Virtual("miditool test out".into()),
        one_scene(),
        Box::new(|_| Ok(Node::Leaf(Box::new(Transpose::new(12))))),
        None,
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

    // Wait for CoreMIDI to finish the connections, then play.
    wait_until_live(&mut keyboard, || !received.lock().unwrap().is_empty());
    received.lock().unwrap().clear();
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
    let (engine, _handle) = Engine::run(
        Some("miditool echo kb"),
        &OutputTarget::Virtual("miditool echo out".into()),
        one_scene(),
        Box::new(|_| {
            Ok(Node::Leaf(Box::new(miditool_effects::Echo::new(
                2, 60_000_000, 1.0, 0,
            ))))
        }),
        None,
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

    wait_until_live(&mut keyboard, || !received.lock().unwrap().is_empty());
    received.lock().unwrap().clear();
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

#[test]
fn continuum_runs_on_its_own_and_stops() {
    let mut keyboard = open_output(&OutputTarget::Virtual("miditool gen kb".into()))
        .expect("create fake keyboard");

    // A brisk continuum so the test stays short: 20 notes per second.
    let (engine, _handle) = Engine::run(
        Some("miditool gen kb"),
        &OutputTarget::Virtual("miditool gen out".into()),
        one_scene(),
        Box::new(|_| {
            Ok(Node::Leaf(Box::new(miditool_effects::Continuum::new(
                20.0,
                miditool_effects::ContinuumOrder::Up,
                0.5,
                1,
            ))))
        }),
        None,
        None,
    )
    .expect("start engine");

    type Arrivals = Arc<Mutex<Vec<Vec<u8>>>>;
    let received: Arrivals = Arc::default();
    let sink = Arc::clone(&received);
    let _capture = open_input(Some("miditool gen out"), move |_stamp, bytes| {
        sink.lock().unwrap().push(bytes.to_vec());
    })
    .expect("open capture");
    sleep(Duration::from_millis(300));

    // Hold two keys; the machine should cycle them with no further input.
    keyboard.send(&[0x90, 60, 90]).unwrap();
    keyboard.send(&[0x90, 64, 90]).unwrap();
    assert!(
        wait_for(|| {
            received
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m[0] == 0x90)
                .count()
                >= 6
        }),
        "the machine should keep emitting while keys are held, got {:?}",
        received.lock().unwrap()
    );

    // Release both; the machine winds down and the stream balances.
    keyboard.send(&[0x80, 60, 0]).unwrap();
    keyboard.send(&[0x80, 64, 0]).unwrap();
    sleep(Duration::from_millis(250));
    let before = received.lock().unwrap().len();
    sleep(Duration::from_millis(300));
    let after = received.lock().unwrap().len();
    assert_eq!(before, after, "the machine must stop after release");

    let msgs = received.lock().unwrap();
    let ons = msgs
        .iter()
        .filter(|m| m[0] & 0xF0 == 0x90 && m[2] > 0)
        .count();
    let offs = msgs
        .iter()
        .filter(|m| m[0] & 0xF0 == 0x80 || (m[0] & 0xF0 == 0x90 && m[2] == 0))
        .count();
    assert_eq!(ons, offs, "every machine note must end");
    drop(msgs);
    engine.stop().expect("stop engine");
}
