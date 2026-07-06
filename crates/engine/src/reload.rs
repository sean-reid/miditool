//! Hot reload: watch the config file, re-parse the scenes, and rebuild
//! the active scene's graph off the hot path.
//!
//! Editors commonly save by writing a temporary file and renaming it over
//! the original, so the watch is on the parent directory, filtered by the
//! config's file name, with a debounce to coalesce the flurry each save
//! produces. On a successful re-parse the active scene is carried across
//! the edit by name (an edit may reorder scenes), falling back to scene 0
//! when it disappeared; its rebuilt graph is handed to the graph thread
//! over a channel and the shared scene table is updated. Any
//! failure is reported to stderr and leaves the running state alone, so a
//! broken edit never kills a performance.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use miditool_core::Node;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::handle::{BuildScene, ReloadScenes, SceneState, lock};

/// How long a save's burst of file events is coalesced before rebuilding.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// The live watcher; dropping it stops the watch thread.
pub(crate) type Watcher = Debouncer<RecommendedWatcher, RecommendedCache>;

/// Watch `path` and, on each debounced change, re-parse the scenes and
/// rebuild the active graph. Everything happens on the watcher's own
/// thread, never the MIDI thread.
pub(crate) fn watch(
    path: PathBuf,
    reload: ReloadScenes,
    build: Arc<BuildScene>,
    scenes: Arc<Mutex<SceneState>>,
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
        if touched {
            apply(&reload, &build, &scenes, &graphs);
        }
    })?;
    debouncer.watch(&dir, RecursiveMode::NonRecursive)?;
    Ok(debouncer)
}

/// One debounced config change: re-parse, rebuild the active scene
/// (matched by name, falling back to scene 0), swap the graph in, and
/// publish the new scene table. Errors keep the running state as-is.
///
/// Lock order: the scenes mutex first, then whatever `reload` and `build`
/// lock internally (the spec store). `reload` must run under the scenes
/// mutex so the store refresh, the table update, and the rebuild are one
/// atomic step: [`crate::EngineHandle::set_scene`] validates an index
/// against the table and builds from the store under this same mutex, and
/// a reload slipping in between would let it build a different config
/// than the one it validated. `set_scene`'s build closure already runs
/// under the scenes mutex, so this order is the only one in the program.
fn apply(
    reload: &ReloadScenes,
    build: &BuildScene,
    scenes: &Mutex<SceneState>,
    graphs: &mpsc::Sender<Node>,
) {
    let mut state = lock(scenes);
    let new_defs = match reload() {
        Ok(defs) => defs,
        Err(e) => {
            eprintln!("miditool: config reload failed, keeping current scenes: {e}");
            return;
        }
    };
    if new_defs.is_empty() {
        eprintln!("miditool: config reload produced no scenes, keeping current scenes");
        return;
    }
    let idx = state
        .defs
        .get(state.active)
        .and_then(|active| new_defs.iter().position(|d| d.name == active.name))
        .unwrap_or(0);
    match build(idx) {
        Ok(root) => {
            // A closed receiver just means the engine already stopped.
            let _ = graphs.send(root);
            state.defs = new_defs;
            state.active = idx;
        }
        Err(e) => eprintln!("miditool: scene rebuild failed, keeping current scenes: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SceneDef;
    use miditool_core::graph::Pass;

    fn def(name: &str, kill: bool) -> SceneDef {
        SceneDef {
            name: name.into(),
            kill_on_exit: kill,
        }
    }

    /// A build closure that records the scene indices it was asked for.
    fn recording_build() -> (Arc<Mutex<Vec<usize>>>, BuildScene) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let build: BuildScene = Box::new(move |i| {
            seen.lock().unwrap().push(i);
            Ok(Node::Leaf(Box::new(Pass)))
        });
        (calls, build)
    }

    #[test]
    fn reload_follows_the_active_scene_by_name() {
        let (calls, build) = recording_build();
        let new = vec![def("a", false), def("x", false), def("b", true)];
        let expected = new.clone();
        let reload: ReloadScenes = Box::new(move || Ok(new.clone()));
        let scenes = Mutex::new(SceneState {
            defs: vec![def("a", false), def("b", false)],
            active: 1,
        });
        let (tx, rx) = mpsc::channel();
        apply(&reload, &build, &scenes, &tx);
        // "b" moved from index 1 to index 2: its graph is the one rebuilt
        // and swapped in, and the table follows it there.
        assert_eq!(*calls.lock().unwrap(), vec![2]);
        assert!(rx.try_recv().is_ok());
        let state = scenes.lock().unwrap();
        assert_eq!(state.active, 2);
        assert_eq!(state.defs, expected);
    }

    #[test]
    fn reload_falls_back_to_scene_zero_when_the_active_scene_disappears() {
        let (calls, build) = recording_build();
        let reload: ReloadScenes = Box::new(|| Ok(vec![def("x", false), def("y", false)]));
        let scenes = Mutex::new(SceneState {
            defs: vec![def("a", false), def("b", false)],
            active: 1,
        });
        let (tx, rx) = mpsc::channel();
        apply(&reload, &build, &scenes, &tx);
        assert_eq!(*calls.lock().unwrap(), vec![0]);
        assert!(rx.try_recv().is_ok());
        let state = scenes.lock().unwrap();
        assert_eq!(state.active, 0);
        assert_eq!(state.defs, vec![def("x", false), def("y", false)]);
    }

    #[test]
    fn reload_holds_the_scene_lock_across_the_store_refresh() {
        let (_calls, build) = recording_build();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (locking_tx, locking_rx) = mpsc::channel::<()>();
        let reload: ReloadScenes = Box::new(move || {
            entered_tx.send(()).unwrap();
            // Wait until the observer is about to take the scenes mutex,
            // then give it a beat to block on the lock. If the refresh ran
            // outside the lock, the observer would win the race and read
            // the stale table an in-flight reload is about to replace.
            locking_rx.recv().unwrap();
            std::thread::sleep(Duration::from_millis(100));
            Ok(vec![def("new", false)])
        });
        let scenes = Arc::new(Mutex::new(SceneState {
            defs: vec![def("old", false)],
            active: 0,
        }));
        let (tx, rx) = mpsc::channel();
        let worker = {
            let scenes = Arc::clone(&scenes);
            std::thread::spawn(move || apply(&reload, &build, &scenes, &tx))
        };
        entered_rx.recv().unwrap();
        locking_tx.send(()).unwrap();
        // Ordering is the assertion: anyone acquiring the mutex after a
        // reload started sees the finished update, never the stale table
        // set_scene would have validated an index against.
        let state = lock(&scenes);
        assert_eq!(state.defs, vec![def("new", false)]);
        drop(state);
        worker.join().unwrap();
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn a_failed_reload_keeps_the_scene_state() {
        let (calls, build) = recording_build();
        let reload: ReloadScenes = Box::new(|| Err("parse error".into()));
        let scenes = Mutex::new(SceneState {
            defs: vec![def("a", false)],
            active: 0,
        });
        let (tx, rx) = mpsc::channel();
        apply(&reload, &build, &scenes, &tx);
        assert!(calls.lock().unwrap().is_empty());
        assert!(rx.try_recv().is_err());
        let state = scenes.lock().unwrap();
        assert_eq!(state.active, 0);
        assert_eq!(state.defs, vec![def("a", false)]);
    }

    #[test]
    fn a_failed_rebuild_keeps_the_scene_state() {
        let build: BuildScene = Box::new(|_| Err("bad graph".into()));
        let reload: ReloadScenes = Box::new(|| Ok(vec![def("a", true), def("b", false)]));
        let scenes = Mutex::new(SceneState {
            defs: vec![def("a", false)],
            active: 0,
        });
        let (tx, rx) = mpsc::channel();
        apply(&reload, &build, &scenes, &tx);
        assert!(rx.try_recv().is_err());
        let state = scenes.lock().unwrap();
        assert_eq!(state.active, 0);
        assert_eq!(state.defs, vec![def("a", false)]);
    }
}
