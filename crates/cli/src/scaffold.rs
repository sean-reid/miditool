//! `miditool new`: starter files written into the current directory.
//!
//! Both templates are runnable as written and commented enough to edit
//! without the docs open. Existing files are never overwritten.

use std::path::Path;

use anyhow::{Context, bail};

/// The starter Luau script: a wedge mirror around a fixed axis, with a
/// couple of alternatives to swap in, commented out.
const SCRIPT_TEMPLATE: &str = r#"-- A miditool script. Wire it into a config with:
--
--     script "NAME.lua" seed=1
--
-- miditool calls on_event(ev) for every incoming event. Return nil to
-- pass the event through unchanged, false to drop it, a table to emit
-- one event, or an array of tables to emit several. Globals persist
-- across events, and rng() / rng_range(lo, hi) draw deterministically
-- from the seed on the script node. Full reference:
-- https://sean-reid.github.io/miditool/configuration/scripting/

-- A wedge mirror: every note lands as far above the axis as it was
-- played below it, and vice versa. Note-offs mirror the same way, so
-- every mirrored note is released.
local axis = 60 -- middle C

function on_event(ev)
    if ev.kind == "note-on" or ev.kind == "note-off" then
        local key = 2 * axis - ev.key
        if key < 0 or key > 127 then
            return false -- mirrored past the end of the keyboard: drop
        end
        ev.key = key
        return ev
    end
    return nil -- everything else passes through untouched
end

-- Other bodies to try inside on_event, one at a time:
--
-- A fifth shadowing every note, 90ms late:
--
--     if ev.kind == "note-on" or ev.kind == "note-off" then
--         local shadow = { kind = ev.kind, ch = ev.ch, key = ev.key + 7,
--                          vel = ev.vel, delay_ms = 90 }
--         return { ev, shadow }
--     end
--
-- Humanized velocity, seeded so takes are reproducible:
--
--     if ev.kind == "note-on" then
--         ev.vel = math.clamp(ev.vel + rng_range(-10, 10), 1, 127)
--         return ev
--     end
"#;

/// The starter config: input hint, the default output spelled out, one
/// seeded effect to edit.
const CONFIG_TEMPLATE: &str = r#"// A miditool config. `miditool run` reads ./miditool.kdl, or pass a path.
// `miditool effects` lists every node; docs: https://sean-reid.github.io/miditool/

// input "Roland"                // optional: substring of the input port
//                               // name; `miditool ports` lists them
output virtual="miditool Out"    // where the DAW listens; this is the default

shuffle-lock seed=42             // scramble the keys; edit the seed to reroll
"#;

/// `miditool new script <name>`: write <dir>/<name>.lua. The directory
/// is the resolved config's home, so the config's relative script paths
/// find it wherever the config lives.
pub fn script(name: &str, dir: &Path) -> anyhow::Result<()> {
    let file = with_extension(name, "lua");
    let path = dir.join(&file);
    write_fresh(&path, SCRIPT_TEMPLATE)?;
    eprintln!("wrote {}", path.display());
    eprintln!("next: add `script \"{file}\" seed=1` to your config, then `miditool run`.");
    eprintln!("the event reference: https://sean-reid.github.io/miditool/configuration/scripting/");
    Ok(())
}

/// The starter home config: every line commented, which is a valid
/// config and a clean MIDI pass-through. Created on first run.
const HOME_CONFIG_TEMPLATE: &str = r#"// Your miditool config. `miditool run` reads this file when there is no
// ./miditool.kdl in the working directory and no path on the command line.
// Every line below is optional; as written, MIDI passes through untouched.
// `miditool effects` lists every node; docs: https://sean-reid.github.io/miditool/

// Pick your keyboard by name substring (`miditool ports` lists the names).
// On macOS, hide=true keeps apps like GarageBand from also hearing the
// raw keyboard:
// input "Roland" hide=true

// Where the transformed stream goes; your DAW listens to this port.
// This line is the default even when omitted:
// output virtual="miditool Out"

// Control miditool from a phone on your network:
// remote port=8320 bind="0.0.0.0"

// Try an effect (delete the // to enable):
// shuffle-lock seed=42
"#;

/// Create the home config on first run. The parent directory is created
/// as needed; an existing file is never touched.
pub fn home_config(path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    write_fresh(path, HOME_CONFIG_TEMPLATE)
}

/// `miditool new config [<name>]`: write ./<name>.kdl.
pub fn config(name: &str) -> anyhow::Result<()> {
    let file = with_extension(name, "kdl");
    write_fresh(Path::new(&file), CONFIG_TEMPLATE)?;
    eprintln!("wrote {file}");
    eprintln!(
        "next: uncomment the input line with your keyboard's name \
         (`miditool ports` lists them), then `miditool run{}`.",
        if file == "miditool.kdl" {
            String::new()
        } else {
            format!(" {file}")
        }
    );
    Ok(())
}

/// Append `ext` unless the name already ends with it, so both
/// `miditool new script wedge` and `... wedge.lua` write wedge.lua.
fn with_extension(name: &str, ext: &str) -> String {
    if name.ends_with(&format!(".{ext}")) {
        name.to_owned()
    } else {
        format!("{name}.{ext}")
    }
}

/// Write `text` to `path`, refusing to clobber anything.
fn write_fresh(path: &Path, text: &str) -> anyhow::Result<()> {
    if path.exists() {
        bail!(
            "{} already exists; pick another name or move it first",
            path.display()
        );
    }
    std::fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The config template must itself be a valid config.
    #[test]
    fn config_template_parses() {
        miditool_config::parse_str("template.kdl", CONFIG_TEMPLATE)
            .expect("the shipped config template should parse");
    }

    /// The first-run home config must parse and be a pure pass-through:
    /// one implicit scene with an empty chain.
    #[test]
    fn home_config_template_is_a_valid_passthrough() {
        let cfg = miditool_config::parse_str("config.kdl", HOME_CONFIG_TEMPLATE)
            .expect("the home config template should parse");
        assert_eq!(cfg.scenes.len(), 1);
        assert!(cfg.scenes[0].chain.is_empty());
        assert!(cfg.input.is_none());
    }

    #[test]
    fn home_config_creates_parents_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("miditool-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("config.kdl");
        home_config(&path).expect("first creation succeeds");
        std::fs::write(&path, "// edited").unwrap();
        home_config(&path).expect_err("must refuse to overwrite");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "// edited");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extensions_are_appended_once() {
        assert_eq!(with_extension("wedge", "lua"), "wedge.lua");
        assert_eq!(with_extension("wedge.lua", "lua"), "wedge.lua");
        assert_eq!(with_extension("miditool", "kdl"), "miditool.kdl");
    }

    #[test]
    fn existing_files_are_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("miditool-new-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("taken.lua");
        std::fs::write(&path, "-- already here").unwrap();
        let err = write_fresh(&path, SCRIPT_TEMPLATE).expect_err("must refuse to overwrite");
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "-- already here",
            "the existing file is untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
