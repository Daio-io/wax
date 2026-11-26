//! Global filesystem paths used by the wax engine.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use wax_contract::LanguageId;

/// Errors returned while resolving global wax paths.
#[derive(Debug, Error)]
pub enum PathsError {
    /// The current user's home directory could not be resolved.
    #[error("could not resolve wax home; set WAX_HOME or configure a user home directory")]
    HomeUnavailable,
    /// A language pack version is not a single normal path segment.
    #[error("invalid language pack version path component {version:?}")]
    InvalidVersion {
        /// Version string passed to [`WaxHome::lang_install_dir`].
        version: String,
    },
}

/// Root and filesystem layout for wax global state, caches, and language packs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaxHome {
    root: PathBuf,
}

impl WaxHome {
    /// Uses the given root for all global wax paths.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves the wax home from `WAX_HOME` or the current user's `~/.wax`.
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::HomeUnavailable`] when neither can be resolved.
    pub fn from_env() -> Result<Self, PathsError> {
        match non_empty_os_var("WAX_HOME") {
            Some(path) => Ok(Self::new(path)),
            None => Ok(Self::new(home_dir()?.join(".wax"))),
        }
    }

    /// Returns the root of this wax home.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the global state file path.
    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    /// Returns the immutable registry snapshot cache directory.
    #[must_use]
    pub fn registry_cache_dir(&self) -> PathBuf {
        self.root.join("cache").join("registries")
    }

    /// Returns the directory that holds all installed versions of a language pack.
    #[must_use]
    pub fn langs_dir(&self, id: &LanguageId) -> PathBuf {
        self.root.join("langs").join(id.as_str())
    }

    /// Returns the install directory for one language pack version.
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::InvalidVersion`] when the version is not one normal path segment.
    pub fn lang_install_dir(&self, id: &LanguageId, version: &str) -> Result<PathBuf, PathsError> {
        validate_version_segment(version)?;
        Ok(self.langs_dir(id).join(version))
    }
}

pub(crate) fn validate_version_segment(version: &str) -> Result<(), PathsError> {
    let mut components = Path::new(version).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(segment)), None) if segment == OsStr::new(version) => Ok(()),
        _ => Err(PathsError::InvalidVersion {
            version: version.to_owned(),
        }),
    }
}

fn home_dir() -> Result<PathBuf, PathsError> {
    #[cfg(windows)]
    {
        if let Some(profile) = non_empty_os_var("USERPROFILE") {
            return Ok(PathBuf::from(profile));
        }
        match (non_empty_os_var("HOMEDRIVE"), non_empty_os_var("HOMEPATH")) {
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
        non_empty_os_var("HOME")
            .map(PathBuf::from)
            .ok_or(PathsError::HomeUnavailable)
    }
}

fn non_empty_os_var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}
