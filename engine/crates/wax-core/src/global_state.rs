//! Global wax state persisted outside individual repositories.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use wax_contract::LanguageId;

/// Global state stored at `~/.wax/state.json`.
#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalState {
    /// Installed language packs by language id and version.
    #[serde(default)]
    pub installed_languages: BTreeMap<LanguageId, BTreeMap<String, InstalledLanguagePack>>,
}

/// Metadata for one installed language pack.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledLanguagePack {
    /// Directory containing the installed pack files.
    pub install_dir: PathBuf,
}

/// Errors returned while loading or saving global wax state.
#[derive(Debug, Error)]
pub enum GlobalStateError {
    /// The file could not be read from disk.
    #[error("failed to read wax global state from {path}: {source}")]
    Read {
        /// Path passed to [`load_global_state`].
        path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The file is not syntactically valid JSON.
    #[error("malformed wax global state JSON in {path}: {source}")]
    MalformedJson {
        /// Path passed to [`load_global_state`].
        path: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// The JSON is valid but does not match the supported state shape.
    #[error("invalid wax global state in {path}: {source}")]
    InvalidState {
        /// Path passed to [`load_global_state`] or [`save_global_state`].
        path: String,
        /// Underlying state decoding or encoding error.
        #[source]
        source: serde_json::Error,
    },
    /// A parent directory for the state file could not be created.
    #[error("failed to create wax global state directory for {path}: {source}")]
    CreateDir {
        /// Path passed to [`save_global_state`].
        path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The file could not be written to disk.
    #[error("failed to write wax global state to {path}: {source}")]
    Write {
        /// Path passed to [`save_global_state`].
        path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Loads global wax state from disk.
///
/// A missing state file is treated as empty state so first-run callers can
/// operate before `~/.wax/state.json` has been created.
pub fn load_global_state(path: impl AsRef<Path>) -> Result<GlobalState, GlobalStateError> {
    let path = path.as_ref();
    let path_display = path.display().to_string();
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(GlobalState::default());
        }
        Err(source) => {
            return Err(GlobalStateError::Read {
                path: path_display,
                source,
            });
        }
    };

    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|source| GlobalStateError::MalformedJson {
            path: path_display.clone(),
            source,
        })?;

    serde_json::from_value(value).map_err(|source| GlobalStateError::InvalidState {
        path: path_display,
        source,
    })
}

/// Saves global wax state to disk, creating parent directories when needed.
pub fn save_global_state(
    path: impl AsRef<Path>,
    state: &GlobalState,
) -> Result<(), GlobalStateError> {
    let path = path.as_ref();
    let path_display = path.display().to_string();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| GlobalStateError::CreateDir {
            path: path_display.clone(),
            source,
        })?;
    }

    let contents =
        serde_json::to_string_pretty(state).map_err(|source| GlobalStateError::InvalidState {
            path: path_display.clone(),
            source,
        })?;
    fs::write(path, format!("{contents}\n")).map_err(|source| GlobalStateError::Write {
        path: path_display,
        source,
    })
}
