//! Hot reload: watch the config file and rebuild the graph off the hot
//! path.
//!
//! Editors commonly save by writing a temporary file and renaming it over
//! the original, so the watch is on the parent directory, filtered by the
//! config's file name, with a debounce to coalesce the flurry each save
//! produces. A successful build is handed to the MIDI callback thread
//! over a channel; a failed build is reported to stderr and the running
//! graph is kept, so a broken edit never kills a performance.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use miditool_core::Node;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::BuildGraph;

/// How long a save's burst of file events is coalesced before rebuilding.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// The live watcher; dropping it stops the watch thread.
pub(crate) type Watcher = Debouncer<RecommendedWatcher, RecommendedCache>;

/// Watch `path` and, on each debounced change, run `build` and send the
/// new graph down `graphs`. Building happens on the watcher's own thread,
/// never the MIDI thread.
pub(crate) fn watch(
    path: PathBuf,
    build: BuildGraph,
    graphs: mpsc::Sender<Node>,
) -> Result<Watcher, notify::Error> {
    let file_name: OsString = path
        .file_name()
        .ok_or_else(|| notify::Error::generic("config path has no file name"))?
        .to_owned();
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
        let events = match result {
            Ok(events) => events,
            Err(errors) => {
                for e in errors {
                    eprintln!("miditool: config watcher: {e}");
                }
                return;
            }
        };
        let touched = events
            .iter()
            .any(|e| e.paths.iter().any(|p| p.file_name() == Some(&file_name)));
        if !touched {
            return;
        }
        match build() {
            // A closed receiver just means the engine already stopped.
            Ok(root) => {
                let _ = graphs.send(root);
            }
            Err(e) => eprintln!("miditool: config reload failed, keeping current graph: {e}"),
        }
    })?;
    debouncer.watch(&dir, RecursiveMode::NonRecursive)?;
    Ok(debouncer)
}
