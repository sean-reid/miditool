//! The endpoint-hiding feature against real CoreMIDI objects: a virtual
//! source stands in for the keyboard, and a spawned `miditool ports`
//! checks visibility, because the private property hides an endpoint from
//! other processes while the hiding process still sees it.

#![cfg(target_os = "macos")]

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use miditool_io::{OutputTarget, hide, open_output};

/// Whether another process sees a source with this name.
fn visible_elsewhere(name: &str) -> bool {
    let out = Command::new(env!("CARGO_BIN_EXE_miditool"))
        .arg("ports")
        .output()
        .expect("run miditool ports");
    String::from_utf8_lossy(&out.stdout).contains(name)
}

fn wait_for(mut pred: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn hide_and_restore_a_source() {
    let name = "miditool hidetest";
    let _source = open_output(&OutputTarget::Virtual(name.into())).expect("create virtual source");
    assert!(
        wait_for(|| visible_elsewhere(name)),
        "virtual source should appear"
    );

    let hidden = hide::hide_source(name).expect("hide should succeed");
    assert_eq!(hidden.name(), name);
    assert!(
        wait_for(|| !visible_elsewhere(name)),
        "hidden source should disappear from other processes"
    );

    hidden.restore().expect("restore should succeed");
    assert!(
        wait_for(|| visible_elsewhere(name)),
        "restored source should reappear"
    );
}
