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
    }
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

    let out_name = match &target {
        miditool_io::OutputTarget::Virtual(name) => format!("{name} (virtual)"),
        miditool_io::OutputTarget::Device(name) => name.clone(),
    };
    let in_name = cfg.input.as_deref().unwrap_or("first available port");
    eprintln!("miditool: {in_name} -> {out_name}. Ctrl-C to stop.");

    wait_for_interrupt()?;
    eprintln!("\nwinding down: releasing held notes.");
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
