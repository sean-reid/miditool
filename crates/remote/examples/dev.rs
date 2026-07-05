//! A fake backend for UI development and the e2e suite. No MIDI needed:
//!
//! ```sh
//! cargo run -p miditool-remote --example dev [PORT]   # default 8321
//! ```
//!
//! Serves the real UI over three pretend scenes, logs scene switches and
//! panics to stdout, and synthesizes a stream of plausible monitor
//! events (a couple per second, varied kinds).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use miditool_remote::{Backend, MonitorEvent, Server, Status};

const SCENES: [&str; 3] = ["scrambled", "echoes", "xenakis clouds"];

fn main() -> std::io::Result<()> {
    let port = match std::env::args().nth(1) {
        Some(arg) => arg.parse().expect("PORT must be a number"),
        None => 8321,
    };
    let backend = Arc::new(DevBackend::new());
    let feeder = Arc::clone(&backend);
    thread::spawn(move || synthesize(&feeder));
    let server = Server::start(port, backend)?;
    println!("dev remote at http://{}", server.addr());
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

/// In-memory stand-in for the engine: scene state plus a queue of
/// synthesized monitor events.
struct DevBackend {
    active: AtomicUsize,
    queue: Mutex<Vec<MonitorEvent>>,
    start: Instant,
}

impl DevBackend {
    fn new() -> DevBackend {
        DevBackend {
            active: AtomicUsize::new(0),
            queue: Mutex::new(Vec::new()),
            start: Instant::now(),
        }
    }

    fn push(&self, kind: &str, ch: u8, detail: String) {
        let event = MonitorEvent {
            t_ms: self.start.elapsed().as_millis() as u64,
            kind: kind.to_string(),
            ch,
            detail,
        };
        self.queue.lock().unwrap().push(event);
    }
}

impl Backend for DevBackend {
    fn status(&self) -> Status {
        Status {
            scenes: SCENES.iter().map(|s| s.to_string()).collect(),
            active: self.active.load(Ordering::Relaxed),
            dropped: 0,
        }
    }

    fn set_scene(&self, idx: usize) -> Result<(), String> {
        if idx >= SCENES.len() {
            return Err(format!("no scene {idx}"));
        }
        self.active.store(idx, Ordering::Relaxed);
        println!("scene -> {}", SCENES[idx]);
        Ok(())
    }

    fn panic(&self) {
        println!("panic: all notes off");
        self.push("panic", 1, "all notes off".to_string());
    }

    fn drain_events(&self) -> Vec<MonitorEvent> {
        std::mem::take(&mut self.queue.lock().unwrap())
    }
}

/// Feed the queue forever with something that looks like a performance.
fn synthesize(backend: &DevBackend) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    // Notes currently sounding, so note-offs match earlier note-ons.
    let mut held: Vec<(u8, u8)> = Vec::new();
    loop {
        thread::sleep(Duration::from_millis(250 + rng.below(400)));
        let ch = 1 + rng.below(2) as u8;
        match rng.below(10) {
            // Mostly notes: press when few are held, release when many.
            roll if roll < 6 => {
                if held.len() >= 4 || (!held.is_empty() && roll < 2) {
                    let (ch, note) = held.remove(rng.below(held.len() as u64) as usize);
                    backend.push("note-off", ch, note_name(note));
                } else {
                    let note = 36 + rng.below(48) as u8;
                    let vel = 40 + rng.below(88);
                    held.push((ch, note));
                    backend.push("note-on", ch, format!("{} vel {vel}", note_name(note)));
                }
            }
            6 | 7 => {
                let cc = if rng.below(2) == 0 { 64 } else { 1 };
                backend.push("cc", ch, format!("cc{cc} = {}", rng.below(128)));
            }
            8 => {
                let bend = rng.below(16384) as i32 - 8192;
                backend.push("bend", ch, format!("{bend:+}"));
            }
            _ => {
                if rng.below(2) == 0 {
                    backend.push("program", ch, format!("program {}", rng.below(128)));
                } else {
                    backend.push("pressure", ch, format!("pressure {}", rng.below(128)));
                }
            }
        }
    }
}

/// "C4 vel 96"-style spelling, middle C = C4.
fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], (note / 12) as i32 - 1)
}

/// xorshift64*: plenty for fake traffic, and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}
