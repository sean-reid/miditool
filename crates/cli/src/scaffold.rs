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

/// `miditool new script <name>`: write ./<name>.lua.
pub fn script(name: &str) -> anyhow::Result<()> {
    let file = with_extension(name, "lua");
    write_fresh(Path::new(&file), SCRIPT_TEMPLATE)?;
    eprintln!("wrote {file}");
    eprintln!("next: add `script \"{file}\" seed=1` to your config, then `miditool run`.");
    eprintln!("the event reference: https://sean-reid.github.io/miditool/configuration/scripting/");
    Ok(())
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
