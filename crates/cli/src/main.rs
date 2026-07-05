mod build;
mod pretty;

use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "miditool",
    version,
    about = "A MIDI mixing layer between your keyboard and your DAW"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run an effect graph between an input port and an output port.
    Run {
        /// Path to a KDL config. Defaults to ./miditool.kdl.
        config: Option<PathBuf>,
    },
    /// List MIDI input and output ports.
    Ports,
    /// Print incoming MIDI events from an input port.
    Monitor {
        /// Substring of the input port name. Defaults to the first
        /// non-miditool port.
        #[arg(long)]
        input: Option<String>,
    },
    /// List the built-in effects and their parameters.
    Effects,
    /// Hide a MIDI source from other apps until Ctrl-C (macOS only).
    /// Useful for testing; `run` does this itself when the config says
    /// `input "..." hide=true`.
    Hide {
        /// Substring of the source name.
        name: String,
    },
    /// Restore sources hidden by a crashed run (macOS only).
    Unhide {
        /// Substring of the source name; restores every source when
        /// omitted.
        name: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Run { config } => run(config),
        Cmd::Ports => ports(),
        Cmd::Monitor { input } => monitor(input),
        Cmd::Effects => {
            print!("{}", pretty::EFFECTS_HELP);
            Ok(())
        }
        Cmd::Hide { name } => hide(name),
        Cmd::Unhide { name } => unhide(name),
    }
}

#[cfg(target_os = "macos")]
fn hide(name: String) -> anyhow::Result<()> {
    let hidden = miditool_io::hide::hide_source(&name)?;
    eprintln!(
        "{} is hidden from other apps; restart any app that was already \
         listening. Ctrl-C to restore.",
        hidden.name()
    );
    wait_for_interrupt()?;
    hidden.restore()?;
    eprintln!("\nrestored.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn unhide(name: Option<String>) -> anyhow::Result<()> {
    let touched = miditool_io::hide::unhide_sources(name.as_deref())?;
    if touched.is_empty() {
        eprintln!("no matching sources found.");
    }
    for name in touched {
        println!("restored {name}");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn hide(_name: String) -> anyhow::Result<()> {
    bail!("hiding MIDI sources is a CoreMIDI feature; macOS only");
}

#[cfg(not(target_os = "macos"))]
fn unhide(_name: Option<String>) -> anyhow::Result<()> {
    bail!("hiding MIDI sources is a CoreMIDI feature; macOS only");
}

fn run(config: Option<PathBuf>) -> anyhow::Result<()> {
    let path = config.unwrap_or_else(|| PathBuf::from("miditool.kdl"));
    if !path.exists() {
        bail!(
            "no config at {}. Pass a path or create one; see `miditool effects` and the examples directory.",
            path.display()
        );
    }
    let cfg = miditool_config::parse_file(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let root = build::build_graph(cfg.chain);
    let target = build::output_target(cfg.output);
    let engine = miditool_engine::Engine::run(cfg.input.as_deref(), &target, root)
        .context("failed to start the engine")?;

    // Hide only after the engine has connected: existing connections keep
    // receiving from a hidden source.
    #[cfg(target_os = "macos")]
    let hidden = if cfg.hide_input {
        let name = cfg
            .input
            .as_deref()
            .expect("hide=true lives on the input node");
        let hidden = miditool_io::hide::hide_source(name)?;
        eprintln!(
            "{} is hidden from other apps; restart any app that was \
             already listening to it.",
            hidden.name()
        );
        Some(hidden)
    } else {
        None
    };
    #[cfg(not(target_os = "macos"))]
    if cfg.hide_input {
        eprintln!("note: `hide=true` is a CoreMIDI feature; ignored on this platform.");
    }

    let out_name = match &target {
        miditool_io::OutputTarget::Virtual(name) => format!("{name} (virtual)"),
        miditool_io::OutputTarget::Device(name) => name.clone(),
    };
    let in_name = cfg.input.as_deref().unwrap_or("first available port");
    eprintln!("miditool: {in_name} -> {out_name}. Ctrl-C to stop.");

    wait_for_interrupt()?;
    eprintln!("\nwinding down: releasing held notes.");
    #[cfg(target_os = "macos")]
    if let Some(hidden) = hidden {
        hidden.restore().context("failed to unhide the input")?;
    }
    engine.stop().context("engine wind-down failed")?;
    Ok(())
}

fn ports() -> anyhow::Result<()> {
    let inputs = miditool_io::input_ports()?;
    let outputs = miditool_io::output_ports()?;
    println!("inputs:");
    print_ports(&inputs);
    println!("outputs:");
    print_ports(&outputs);
    Ok(())
}

fn print_ports(names: &[String]) {
    if names.is_empty() {
        println!("  (none)");
    }
    for name in names {
        println!("  {name}");
    }
}

fn monitor(input: Option<String>) -> anyhow::Result<()> {
    let mut printer = pretty::EventPrinter::new();
    let _input = miditool_io::open_input(input.as_deref(), move |stamp_us, bytes| {
        printer.print(stamp_us, bytes);
    })
    .context("failed to open the input port")?;
    eprintln!("monitoring. Ctrl-C to stop.");
    wait_for_interrupt()
}

/// Block until Ctrl-C.
fn wait_for_interrupt() -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })
    .context("failed to install the Ctrl-C handler")?;
    rx.recv().ok();
    Ok(())
}
