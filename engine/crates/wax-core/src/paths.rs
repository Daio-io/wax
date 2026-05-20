//! Global filesystem paths used by the wax engine.

use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use wax_contract::LanguageId;

/// Errors returned while resolving global wax paths.
#[derive(Debug, Error)]
pub enum PathsError {
    /// The current user's home directory could not be resolved.
    #[error("could not resolve wax home; set WAX_HOME or HOME")]
    HomeUnavailable,
    /// A language pack version is not a single normal path segment.
    #[error("invalid language pack version path component {version:?}")]
    InvalidVersion {
        /// Version string passed to [`lang_install_dir`].
        version: String,
    },
}

/// Returns the global wax home directory.
///
/// `WAX_HOME` overrides the default. When the override is absent, the path is
/// resolved as `~/.wax` using the current user's home directory environment.
pub fn wax_home() -> Result<PathBuf, PathsError> {
    match std::env::var_os("WAX_HOME") {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Ok(home_dir()?.join(".wax")),
    }
}

/// Returns the global state file path.
pub fn state_file() -> Result<PathBuf, PathsError> {
    Ok(wax_home()?.join("state.json"))
}

/// Returns the install directory for one language pack version.
pub fn lang_install_dir(id: &LanguageId, version: &str) -> Result<PathBuf, PathsError> {
    validate_version_segment(version)?;
    Ok(wax_home()?.join("langs").join(id.as_str()).join(version))
}

fn validate_version_segment(version: &str) -> Result<(), PathsError> {
    let mut components = Path::new(version).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(PathsError::InvalidVersion {
            version: version.to_owned(),
        }),
    }
}

fn home_dir() -> Result<PathBuf, PathsError> {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return Ok(PathBuf::from(profile));
        }
        match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
            (Some(drive), Some(path)) => {
                let mut home = PathBuf::from(drive);
                home.push(path);
                Ok(home)
            }
            _ => Err(PathsError::HomeUnavailable),
        }
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(PathsError::HomeUnavailable)
    }
}
