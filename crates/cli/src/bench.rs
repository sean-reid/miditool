//! `miditool bench`: round-trip latency through a live engine.
//!
//! The wiring is a loopback: a virtual source ("miditool bench in") feeds
//! a pass-through engine whose output is a second virtual port ("miditool
//! bench out"), which a capture input listens to. Every message makes the
//! journey a keyboard's would: through the OS MIDI service, the engine's
//! decode-process-encode path, and back through the service to a listener.
//! What is measured is therefore the whole stack, not just the pipeline.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use miditool_core::Node;
use miditool_core::graph::Pass;
use miditool_io::OutputTarget;

/// The virtual port we send into; the engine reads from it.
const SOURCE: &str = "miditool bench in";

/// The engine's output; the capture input reads from it.
const SINK: &str = "miditool bench out";

/// One note-on/note-off pair is sent per period.
const PERIOD: Duration = Duration::from_millis(4);

/// Time for the OS to publish the virtual ports before the first send,
/// and for stragglers to arrive after the last one.
const SETTLE: Duration = Duration::from_millis(300);

/// Send `rounds` note pairs through the loopback and report round-trip
/// latency percentiles.
pub fn bench(rounds: u32) -> anyhow::Result<()> {
    let mut source = miditool_io::open_output(&OutputTarget::Virtual(SOURCE.to_owned())).context(
        "bench needs virtual MIDI ports (CoreMIDI or ALSA); \
         this platform cannot create them",
    )?;

    let scenes = vec![miditool_engine::SceneDef {
        name: "bench".to_owned(),
        kill_on_exit: false,
    }];
    let (engine, _handle) = miditool_engine::Engine::run(
        Some(SOURCE),
        &OutputTarget::Virtual(SINK.to_owned()),
        scenes,
        Box::new(|_| Ok(Node::Leaf(Box::new(Pass)))),
        None,
    )
    .context("failed to start the pass-through engine")?;

    let arrivals: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&arrivals);
    let capture = miditool_io::open_input(Some(SINK), move |_stamp, bytes| {
        let now = Instant::now();
        let mut arrivals = recorder.lock().unwrap();
        // One instant per message: the engine re-encodes everything as
        // full 3-byte channel messages, so a packet holds a whole number
        // of them even when the backend coalesces.
        for chunk in bytes.chunks(3) {
            if chunk.len() == 3 {
                arrivals.push(now);
            }
        }
    })
    .context("failed to open the capture input on the engine's output")?;

    thread::sleep(SETTLE);
    eprintln!("bench: {rounds} note pairs, {SOURCE} -> engine -> {SINK}");

    let mut sends = Vec::with_capacity(rounds as usize * 2);
    let start = Instant::now();
    for i in 0..rounds {
        // Pace against the schedule, not the previous send, so timing
        // error does not accumulate across rounds.
        let due = start + PERIOD * i;
        while Instant::now() < due {
            thread::sleep(Duration::from_micros(200));
        }
        sends.push(Instant::now());
        source.send(&[0x90, 60, 100])?;
        sends.push(Instant::now());
        source.send(&[0x80, 60, 0])?;
    }

    thread::sleep(SETTLE);
    drop(capture);
    engine.stop().context("engine wind-down failed")?;

    let arrivals = arrivals.lock().unwrap();
    report(&sends, &arrivals)
}

fn report(sends: &[Instant], arrivals: &[Instant]) -> anyhow::Result<()> {
    if arrivals.is_empty() {
        anyhow::bail!(
            "sent {} messages but received none; something else may have \
             claimed the {SINK} port",
            sends.len()
        );
    }
    let lost = sends.len().saturating_sub(arrivals.len());

    // Pair sends and arrivals by index. A lost message skews the pairs
    // after it, so the numbers are only exact when lost is 0; the lost
    // column is there to say so.
    let mut lat_us: Vec<f64> = arrivals
        .iter()
        .zip(sends)
        .map(|(arrived, sent)| arrived.duration_since(*sent).as_secs_f64() * 1e6)
        .collect();
    lat_us.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| lat_us[((lat_us.len() - 1) as f64 * p / 100.0).round() as usize];

    println!(
        "{:>7} {:>11} {:>11} {:>11} {:>11} {:>11} {:>6}",
        "count", "min", "p50", "p90", "p99", "max", "lost"
    );
    println!(
        "{:>7} {:>11} {:>11} {:>11} {:>11} {:>11} {:>6}",
        lat_us.len(),
        us(lat_us[0]),
        us(pct(50.0)),
        us(pct(90.0)),
        us(pct(99.0)),
        us(*lat_us.last().unwrap()),
        lost
    );
    Ok(())
}

fn us(value: f64) -> String {
    format!("{value:.1}us")
}
