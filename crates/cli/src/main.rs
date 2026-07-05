mod backend;
mod bench;
mod build;
mod doctor;
mod pretty;
mod scaffold;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

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
    /// Write a starter file: a config or a Luau script.
    New {
        #[command(subcommand)]
        what: NewWhat,
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
    /// Measure round-trip latency through a pass-through engine.
    Bench {
        /// Number of note pairs to send.
        #[arg(long, default_value_t = 500)]
        rounds: u32,
    },
    /// Check the environment: ports, config, hidden sources, DAW state.
    Doctor {
        /// Config to validate. Defaults to ./miditool.kdl when present.
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum NewWhat {
    /// Write ./<NAME>.lua, a commented starter script.
    Script {
        /// Script name; ".lua" is appended unless already there.
        name: String,
    },
    /// Write ./<NAME>.kdl, a minimal commented config.
    Config {
        /// Config name; defaults to "miditool", ".kdl" is appended
        /// unless already there.
        name: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Run { config } => run(config),
        Cmd::New { what } => match what {
            NewWhat::Script { name } => scaffold::script(&name),
            NewWhat::Config { name } => scaffold::config(name.as_deref().unwrap_or("miditool")),
        },
        Cmd::Ports => ports(),
        Cmd::Monitor { input } => monitor(input),
        Cmd::Effects => {
            print!("{}", pretty::EFFECTS_HELP);
            Ok(())
        }
        Cmd::Hide { name } => hide(name),
        Cmd::Unhide { name } => unhide(name),
        Cmd::Bench { rounds } => bench::bench(rounds),
        Cmd::Doctor { config } => doctor::doctor(config),
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
        // A named miss is an error, like hide's; a bare restore-all
        // finding nothing to do is a clean no-op.
        match name {
            Some(name) => bail!("no MIDI source matching {name:?} in the device tree"),
            None => eprintln!("nothing to restore."),
        }
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
            "no config at {}. Pass a path or create one; `miditool effects` lists the \
             building blocks, and https://sean-reid.github.io/miditool/ has examples.",
            path.display()
        );
    }
    let cfg = miditool_config::parse_file(&path).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nrun `miditool effects` for the list of config nodes, or see \
             https://sean-reid.github.io/miditool/"
        )
    })?;

    let target = build::output_target(cfg.output);

    // Script paths resolve against the config file's directory, so a
    // config runs the same from anywhere.
    let base = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };

    // Scene specs live in a store shared by the scene builder and the
    // reload closure: edits to the config swap graphs in place while held
    // notes drain through the graph that opened them. Input and output
    // changes need a restart.
    let store = Arc::new(Mutex::new((cfg.scenes.clone(), cfg.tempo)));
    let defs = scene_defs(&cfg.scenes);

    let build_store = Arc::clone(&store);
    let builder: miditool_engine::BuildScene = Box::new(move |idx| {
        let (scenes, tempo) = &*build_store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let scene = scenes
            .get(idx)
            .ok_or_else(|| format!("no scene at index {idx}"))?;
        build::build_graph(scene.chain.clone(), *tempo, &base)
    });

    let reload_store = Arc::clone(&store);
    let reload_path = path.clone();
    let reloader: miditool_engine::ReloadScenes = Box::new(move || {
        let cfg = miditool_config::parse_file(&reload_path).map_err(|e| e.to_string())?;
        let defs = scene_defs(&cfg.scenes);
        *reload_store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = (cfg.scenes, cfg.tempo);
        Ok(defs)
    });

    let (engine, mut handle) = miditool_engine::Engine::run(
        cfg.input.as_deref(),
        &target,
        defs,
        builder,
        Some((path, reloader)),
    )
    .context("failed to start the engine")?;

    let _remote = match cfg.remote {
        Some(spec) => {
            let backend = backend::EngineBackend::new(handle.clone(), handle.take_tap());
            let addr = std::net::SocketAddr::from((spec.bind, spec.port));
            let server = miditool_remote::Server::start(addr, Arc::new(backend))
                .context("failed to start the web remote")?;
            if spec.bind.is_loopback() {
                eprintln!(
                    "remote: http://{}/ (this machine only; set bind=\"0.0.0.0\" on the \
                     remote node to open it to the network for a phone)",
                    server.addr(),
                );
            } else {
                eprintln!(
                    "remote: http://{}/ (from a phone on this network, try http://{}.local:{}/)",
                    server.addr(),
                    hostname().unwrap_or_else(|| "<this-computer>".into()),
                    spec.port,
                );
            }
            Some(server)
        }
        None => None,
    };

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

fn scene_defs(scenes: &[miditool_config::SceneSpec]) -> Vec<miditool_engine::SceneDef> {
    scenes
        .iter()
        .map(|s| miditool_engine::SceneDef {
            name: s.name.clone(),
            kill_on_exit: s.kill_on_exit,
        })
        .collect()
}

/// Best-effort machine name for the "open this on your phone" hint.
fn hostname() -> Option<String> {
    let out = std::process::Command::new("hostname")
        .arg("-s")
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
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
