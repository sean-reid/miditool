//! `miditool doctor`: environment checks.
//!
//! Each check prints one status line and every check always runs, so a
//! single report shows everything that needs attention. Only hard
//! failures (an unreachable MIDI backend, a broken config) make the
//! command exit nonzero; the platform-specific checks can at worst warn.

use std::fmt::Display;
use std::path::PathBuf;

/// Collects the verdicts as they print, so `doctor` can run everything
/// and still exit nonzero when something failed.
struct Checkup {
    failed: bool,
}

impl Checkup {
    fn ok(&mut self, message: impl Display) {
        println!("ok    {message}");
    }

    fn warn(&mut self, message: impl Display) {
        println!("warn  {message}");
    }

    fn fail(&mut self, message: impl Display) {
        self.failed = true;
        println!("fail  {message}");
    }
}

/// Run every check. `config` overrides the default ./miditool.kdl.
pub fn doctor(config: Option<PathBuf>) -> anyhow::Result<()> {
    let mut checkup = Checkup { failed: false };
    check_backend(&mut checkup);
    check_config(&mut checkup, config);
    #[cfg(target_os = "macos")]
    {
        check_hidden_sources(&mut checkup);
        check_daws(&mut checkup);
    }
    #[cfg(target_os = "linux")]
    check_sequencer(&mut checkup);
    #[cfg(target_os = "windows")]
    check_loopmidi(&mut checkup);

    if checkup.failed {
        anyhow::bail!("some checks failed");
    }
    Ok(())
}

/// Windows has no native virtual ports; without loopMIDI there is nowhere
/// for miditool to send.
#[cfg(target_os = "windows")]
fn check_loopmidi(checkup: &mut Checkup) {
    match miditool_io::output_ports() {
        Ok(outputs) if outputs.iter().any(|o| o.to_lowercase().contains("loopmidi")) => {
            checkup.ok("loopMIDI port found for virtual output")
        }
        Ok(_) => checkup.warn(
            "no loopMIDI port; install loopMIDI and create one, then use `output device=\"loopMIDI\"`",
        ),
        Err(_) => {}
    }
}

/// The MIDI backend answers and enumerates ports.
fn check_backend(checkup: &mut Checkup) {
    match (miditool_io::input_ports(), miditool_io::output_ports()) {
        (Ok(inputs), Ok(outputs)) => checkup.ok(format!(
            "midi backend: {} input{} ({}), {} output{} ({})",
            inputs.len(),
            plural(inputs.len()),
            list(&inputs),
            outputs.len(),
            plural(outputs.len()),
            list(&outputs),
        )),
        (Err(e), _) | (_, Err(e)) => checkup.fail(format!("midi backend: {e}")),
    }
}

/// The config parses, if there is one to parse.
fn check_config(checkup: &mut Checkup, config: Option<PathBuf>) {
    let path = config.or_else(|| {
        let default = PathBuf::from("miditool.kdl");
        default.exists().then_some(default)
    });
    let Some(path) = path else {
        checkup.ok("config: no miditool.kdl here; nothing to check");
        return;
    };
    match miditool_config::parse_file(&path) {
        Ok(cfg) => checkup.ok(format!(
            "config {}: parses, {} top-level effect{}",
            path.display(),
            cfg.chain.len(),
            plural(cfg.chain.len()),
        )),
        Err(e) => checkup.fail(format!("config {}: {e}", path.display())),
    }
}

/// Sources present in the CoreMIDI device tree but missing from flat
/// enumeration: hidden by a crashed run, or merely offline.
#[cfg(target_os = "macos")]
fn check_hidden_sources(checkup: &mut Checkup) {
    match miditool_io::hide::hidden_sources() {
        Ok(hidden) if hidden.is_empty() => checkup.ok("no MIDI sources look hidden"),
        Ok(hidden) => checkup.warn(format!(
            "possibly hidden (or offline): {}; run `miditool unhide` to be sure",
            hidden.join(", ")
        )),
        Err(e) => checkup.warn(format!("could not scan for hidden sources: {e}")),
    }
}

/// DAWs that grab every port only rescan at launch, so one that started
/// before miditool keeps hearing the raw keyboard.
#[cfg(target_os = "macos")]
fn check_daws(checkup: &mut Checkup) {
    for app in ["GarageBand", "Logic Pro"] {
        match process_running(app) {
            Some(true) => checkup.warn(format!(
                "{app} is running; apps started before miditool keep hearing \
                 the raw keyboard until relaunched"
            )),
            Some(false) => checkup.ok(format!("{app} is not running")),
            None => checkup.warn(format!("could not check for {app} (pgrep failed)")),
        }
    }
}

/// Whether a process with exactly this name is running.
#[cfg(target_os = "macos")]
fn process_running(name: &str) -> Option<bool> {
    std::process::Command::new("pgrep")
        .args(["-x", name])
        .output()
        .ok()
        .map(|out| out.status.success())
}

/// The ALSA sequencer device exists.
#[cfg(target_os = "linux")]
fn check_sequencer(checkup: &mut Checkup) {
    if std::path::Path::new("/dev/snd/seq").exists() {
        checkup.ok("/dev/snd/seq exists");
    } else {
        checkup.warn("/dev/snd/seq is missing; load the ALSA sequencer with `modprobe snd-seq`");
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}
