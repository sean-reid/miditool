//! Core types for miditool: the event model, the MIDI wire codec, the
//! composable effect graph, note tracking, and seeded randomness.
//!
//! Everything here is realtime-safe: no allocation, no locking, no syscalls
//! in any code path reachable from `Node::process`.

pub mod event;
pub mod graph;
pub mod notemap;
pub mod rng;
pub mod sieve;
pub mod tracker;
pub mod wire;

pub use event::{Event, EventKind, Timestamp};
pub use graph::{Effect, EventBuf, Filter, MAX_FANOUT, Node, ProcCx};
pub use notemap::PerNote;
pub use rng::Prng;
pub use sieve::Sieve;
pub use tracker::NoteTracker;
