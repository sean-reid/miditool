//! Where the config comes from.
//!
//! Resolution order, first hit wins: an explicit path argument, the
//! MIDITOOL_CONFIG environment variable, ./miditool.kdl in the working
//! directory, then the miditool home at ~/.miditool/config.kdl (the
//! directory is overridable with MIDITOOL_HOME). Explicit choices (the
//! argument or the variable) are honored even when the file is missing,
//! so a typo errors instead of silently falling through.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Argument,
    EnvVar,
    WorkingDir,
    Home,
}

impl ConfigSource {
    /// A short parenthetical for status lines.
    pub fn describe(self) -> &'static str {
        match self {
            ConfigSource::Argument => "",
            ConfigSource::EnvVar => " (from MIDITOOL_CONFIG)",
            ConfigSource::WorkingDir => " (working directory)",
            ConfigSource::Home => " (miditool home)",
        }
    }
}

/// The user's home directory, without any deprecated std shims.
fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let var = "USERPROFILE";
    #[cfg(not(windows))]
    let var = "HOME";
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The miditool home: MIDITOOL_HOME, or ~/.miditool.
pub fn miditool_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MIDITOOL_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    home_dir().map(|h| h.join(".miditool"))
}

/// The home config path, whether or not it exists yet.
pub fn home_config() -> Option<PathBuf> {
    miditool_home().map(|d| d.join("config.kdl"))
}

/// Resolve against the real environment.
pub fn resolve(arg: Option<PathBuf>) -> Option<(PathBuf, ConfigSource)> {
    let env_config = std::env::var_os("MIDITOOL_CONFIG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    resolve_with(arg, env_config, Path::new("."), home_config())
}

/// The pure resolution logic, injectable for tests.
fn resolve_with(
    arg: Option<PathBuf>,
    env_config: Option<PathBuf>,
    cwd: &Path,
    home_config: Option<PathBuf>,
) -> Option<(PathBuf, ConfigSource)> {
    if let Some(path) = arg {
        return Some((path, ConfigSource::Argument));
    }
    if let Some(path) = env_config {
        return Some((path, ConfigSource::EnvVar));
    }
    let local = cwd.join("miditool.kdl");
    if local.exists() {
        return Some((local, ConfigSource::WorkingDir));
    }
    match home_config {
        Some(path) if path.exists() => Some((path, ConfigSource::Home)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_argument_wins_even_when_missing() {
        let dir = std::env::temp_dir();
        let got = resolve_with(Some("nope.kdl".into()), Some("env.kdl".into()), &dir, None);
        assert_eq!(got, Some(("nope.kdl".into(), ConfigSource::Argument)));
    }

    #[test]
    fn env_var_beats_search_paths() {
        let dir = std::env::temp_dir();
        let got = resolve_with(None, Some("env.kdl".into()), &dir, None);
        assert_eq!(got, Some(("env.kdl".into(), ConfigSource::EnvVar)));
    }

    #[test]
    fn working_directory_beats_home() {
        let dir = tempdir("cfg-cwd");
        std::fs::write(dir.join("miditool.kdl"), "").unwrap();
        let home = tempdir("cfg-home").join("config.kdl");
        std::fs::write(&home, "").unwrap();
        let got = resolve_with(None, None, &dir, Some(home)).unwrap();
        assert_eq!(got.1, ConfigSource::WorkingDir);
    }

    #[test]
    fn home_config_is_the_last_resort() {
        let dir = tempdir("cfg-empty");
        let home = tempdir("cfg-home2").join("config.kdl");
        std::fs::write(&home, "").unwrap();
        let got = resolve_with(None, None, &dir, Some(home.clone())).unwrap();
        assert_eq!(got, (home, ConfigSource::Home));
    }

    #[test]
    fn nothing_found_is_none() {
        let dir = tempdir("cfg-none");
        let missing = dir.join("absent").join("config.kdl");
        assert_eq!(resolve_with(None, None, &dir, Some(missing)), None);
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("miditool-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
