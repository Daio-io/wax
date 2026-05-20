//! Global filesystem paths used by the wax engine.

use std::path::PathBuf;
use wax_contract::LanguageId;

/// Returns the global wax home directory.
///
/// `WAX_HOME` overrides the default. When the override is absent, the path is
/// resolved as `~/.wax` using the current user's home directory environment.
pub fn wax_home() -> PathBuf {
    match std::env::var_os("WAX_HOME") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => home_dir().join(".wax"),
    }
}

/// Returns the global state file path.
pub fn state_file() -> PathBuf {
    wax_home().join("state.json")
}

/// Returns the install directory for one language pack version.
pub fn lang_install_dir(id: &LanguageId, version: &str) -> PathBuf {
    wax_home().join("langs").join(id.as_str()).join(version)
}

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(profile);
        }
        match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
            (Some(drive), Some(path)) => {
                let mut home = PathBuf::from(drive);
                home.push(path);
                home
            }
            _ => PathBuf::from("."),
        }
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
