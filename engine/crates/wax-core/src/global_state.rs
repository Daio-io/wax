//! Global wax state persisted outside individual repositories.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use wax_contract::LanguageId;

use crate::paths::validate_version_segment;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ptr;

#[cfg(windows)]
const ERROR_UNABLE_TO_REMOVE_REPLACED: i32 = 1175;
#[cfg(windows)]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT: i32 = 1176;
#[cfg(windows)]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

/// Global state stored at `~/.wax/state.json`.
#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalState {
    /// Installed language packs by language id and version.
    #[serde(default)]
    pub installed_languages: BTreeMap<LanguageId, BTreeMap<String, InstalledLanguagePack>>,
    /// Remembered design-system repositories keyed by design-system id.
    #[serde(default)]
    pub design_systems: BTreeMap<String, RememberedDesignSystem>,
}

/// One remembered design-system repository location.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RememberedDesignSystem {
    /// Display name shown in prompts and lists.
    pub name: String,
    /// Canonical repository root for the design-system project.
    pub repo_root: PathBuf,
    /// Repo-relative path to the last seen wax config file.
    pub last_seen_config: PathBuf,
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
    /// A temporary state file could not be created.
    #[error("failed to create temporary wax global state file {temp_path} for {path}: {source}")]
    CreateTemp {
        /// Path passed to [`save_global_state`].
        path: String,
        /// Temporary path attempted for the atomic write.
        temp_path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// A temporary state file could not be written.
    #[error("failed to write temporary wax global state file {temp_path} for {path}: {source}")]
    WriteTemp {
        /// Path passed to [`save_global_state`].
        path: String,
        /// Temporary path used for the atomic write.
        temp_path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// A temporary state file could not be flushed to disk.
    #[error("failed to sync temporary wax global state file {temp_path} for {path}: {source}")]
    SyncTemp {
        /// Path passed to [`save_global_state`].
        path: String,
        /// Temporary path used for the atomic write.
        temp_path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// A temporary state file could not replace the destination.
    #[error("failed to replace wax global state {path} with {temp_path}: {source}")]
    Rename {
        /// Path passed to [`save_global_state`].
        path: String,
        /// Temporary path used for the atomic write.
        temp_path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// A state entry used a language pack version that is not one path segment.
    #[error(
        "invalid wax global state in {path}: language {language_id} has invalid version path component {version:?}"
    )]
    InvalidVersion {
        /// Path passed to [`load_global_state`] or [`save_global_state`].
        path: String,
        /// Language id associated with the invalid version key.
        language_id: LanguageId,
        /// Invalid installed language pack version.
        version: String,
    },
    /// A remembered design-system entry used an invalid id.
    #[error(
        "invalid wax global state in {path}: design system {design_system_id:?} is not a valid id; expected lowercase ASCII slug [a-z][a-z0-9-]*"
    )]
    InvalidDesignSystemId {
        /// Path passed to [`load_global_state`] or [`save_global_state`].
        path: String,
        /// Invalid design-system id key.
        design_system_id: String,
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

    let state: GlobalState =
        serde_json::from_value(value).map_err(|source| GlobalStateError::InvalidState {
            path: path_display.clone(),
            source,
        })?;
    validate_global_state(&state, &path_display)?;

    Ok(state)
}

fn validate_global_state(state: &GlobalState, path_display: &str) -> Result<(), GlobalStateError> {
    validate_global_state_versions(state, path_display)?;
    validate_global_state_design_systems(state, path_display)?;
    Ok(())
}

fn validate_global_state_versions(
    state: &GlobalState,
    path_display: &str,
) -> Result<(), GlobalStateError> {
    for (language_id, versions) in &state.installed_languages {
        for version in versions.keys() {
            validate_version_segment(version).map_err(|_| GlobalStateError::InvalidVersion {
                path: path_display.to_owned(),
                language_id: language_id.clone(),
                version: version.clone(),
            })?;
        }
    }

    Ok(())
}

fn validate_global_state_design_systems(
    state: &GlobalState,
    path_display: &str,
) -> Result<(), GlobalStateError> {
    for design_system_id in state.design_systems.keys() {
        if LanguageId::try_from(design_system_id.as_str()).is_err() {
            return Err(GlobalStateError::InvalidDesignSystemId {
                path: path_display.to_owned(),
                design_system_id: design_system_id.clone(),
            });
        }
    }

    Ok(())
}

/// Saves global wax state to disk, creating parent directories when needed.
pub fn save_global_state(
    path: impl AsRef<Path>,
    state: &GlobalState,
) -> Result<(), GlobalStateError> {
    let path = path.as_ref();
    let path_display = path.display().to_string();
    validate_global_state(state, &path_display)?;

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
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
    write_state_atomically(path, &path_display, format!("{contents}\n").as_bytes())
}

fn write_state_atomically(
    path: &Path,
    path_display: &str,
    contents: &[u8],
) -> Result<(), GlobalStateError> {
    let (temp_path, mut temp_file) = create_temp_state_file(path, path_display)?;
    let temp_display = temp_path.display().to_string();

    if let Err(source) = temp_file.write_all(contents) {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(GlobalStateError::WriteTemp {
            path: path_display.to_owned(),
            temp_path: temp_display,
            source,
        });
    }

    if let Err(source) = temp_file.sync_all() {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(GlobalStateError::SyncTemp {
            path: path_display.to_owned(),
            temp_path: temp_display,
            source,
        });
    }
    drop(temp_file);

    replace_state_file(&temp_path, &temp_display, path, path_display)
}

fn remove_temp_file(temp_path: &Path) {
    let _ = fs::remove_file(temp_path);
}

#[cfg(not(windows))]
fn replace_state_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), GlobalStateError> {
    fs::rename(temp_path, path).map_err(|source| {
        remove_temp_file(temp_path);
        GlobalStateError::Rename {
            path: path_display.to_owned(),
            temp_path: temp_display.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn replace_state_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), GlobalStateError> {
    if !path.exists() {
        return rename_temp_state_file(temp_path, temp_display, path, path_display);
    }

    replace_existing_state_file(temp_path, temp_display, path, path_display)
}

#[cfg(windows)]
fn rename_temp_state_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), GlobalStateError> {
    fs::rename(temp_path, path).map_err(|source| {
        remove_temp_file(temp_path);
        GlobalStateError::Rename {
            path: path_display.to_owned(),
            temp_path: temp_display.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn replace_existing_state_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), GlobalStateError> {
    let replaced = wide_null(path.as_os_str());
    let replacement = wide_null(temp_path.as_os_str());

    // SAFETY: both path buffers are null-terminated and live for the duration
    // of the call; null backup/exclude/reserved pointers match ReplaceFileW.
    let replaced = unsafe {
        replace_file_w(
            replaced.as_ptr(),
            replacement.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };

    if replaced == 0 {
        let source = io::Error::last_os_error();
        if recover_windows_partial_replace_failure(&source, temp_path, path).unwrap_or(false) {
            return Ok(());
        }
        if !is_documented_windows_partial_replace_failure(&source) {
            remove_temp_file(temp_path);
        }
        return Err(GlobalStateError::Rename {
            path: path_display.to_owned(),
            temp_path: temp_display.to_owned(),
            source,
        });
    }

    Ok(())
}

#[cfg(windows)]
fn recover_windows_partial_replace_failure(
    source: &io::Error,
    temp_path: &Path,
    path: &Path,
) -> Result<bool, io::Error> {
    if source.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT)
        && !path.exists()
        && temp_path.exists()
    {
        fs::rename(temp_path, path)?;
        return Ok(true);
    }

    Ok(false)
}

#[cfg(windows)]
fn is_documented_windows_partial_replace_failure(source: &io::Error) -> bool {
    matches!(
        source.raw_os_error(),
        Some(
            ERROR_UNABLE_TO_REMOVE_REPLACED
                | ERROR_UNABLE_TO_MOVE_REPLACEMENT
                | ERROR_UNABLE_TO_MOVE_REPLACEMENT_2
        )
    )
}

fn create_temp_state_file(
    path: &Path,
    path_display: &str,
) -> Result<(PathBuf, File), GlobalStateError> {
    for attempt in 0..1000 {
        let temp_path = temp_state_path(path, attempt);
        let temp_display = temp_path.display().to_string();
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(GlobalStateError::CreateTemp {
                    path: path_display.to_owned(),
                    temp_path: temp_display,
                    source,
                });
            }
        }
    }

    let temp_path = temp_state_path(path, 999);
    Err(GlobalStateError::CreateTemp {
        path: path_display.to_owned(),
        temp_path: temp_path.display().to_string(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate unique temporary state path",
        ),
    })
}

fn temp_state_path(path: &Path, attempt: u32) -> PathBuf {
    sibling_state_path(path, attempt, "tmp")
}

#[cfg(windows)]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn sibling_state_path(path: &Path, attempt: u32, extension: &str) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "state.json".into());

    parent.join(format!(
        ".{file_name}.{}.{attempt}.{extension}",
        std::process::id(),
    ))
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "ReplaceFileW"]
    fn replace_file_w(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(test)]
mod global_state_design_systems_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn temp_state_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "wax-core-global-state-design-systems-{name}-{}",
            std::process::id()
        ))
    }

    struct TestPath(PathBuf);

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn loads_and_saves_remembered_design_systems() {
        let path = TestPath(temp_state_path("roundtrip"));
        let state = GlobalState {
            installed_languages: BTreeMap::new(),
            design_systems: BTreeMap::from([(
                "acme".to_owned(),
                RememberedDesignSystem {
                    name: "Acme Design System".to_owned(),
                    repo_root: PathBuf::from("/tmp/acme-ds"),
                    last_seen_config: PathBuf::from(".wax/wax.config.json"),
                },
            )]),
        };

        save_global_state(&path.0, &state).unwrap();
        let loaded = load_global_state(&path.0).unwrap();

        assert_eq!(loaded, state);
        assert_eq!(loaded.design_systems["acme"].name, "Acme Design System");
        assert_eq!(
            loaded.design_systems["acme"].repo_root,
            PathBuf::from("/tmp/acme-ds")
        );
        assert_eq!(
            loaded.design_systems["acme"].last_seen_config,
            PathBuf::from(".wax/wax.config.json")
        );
    }

    #[test]
    fn loads_design_systems_from_json_fixture() {
        let path = TestPath(temp_state_path("fixture"));
        std::fs::write(
            &path.0,
            r#"{
  "installed_languages": {},
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "repo_root": "/tmp/acme-ds",
      "last_seen_config": ".wax/wax.config.json"
    }
  }
}"#,
        )
        .unwrap();

        let loaded = load_global_state(&path.0).unwrap();

        assert_eq!(loaded.installed_languages.len(), 0);
        assert_eq!(loaded.design_systems.len(), 1);
        assert_eq!(loaded.design_systems["acme"].name, "Acme Design System");
    }

    #[test]
    fn rejects_invalid_design_system_ids() {
        let path = TestPath(temp_state_path("invalid-id"));
        std::fs::write(
            &path.0,
            r#"{
  "installed_languages": {},
  "design_systems": {
    "Acme": {
      "name": "Acme Design System",
      "repo_root": "/tmp/acme-ds",
      "last_seen_config": ".wax/wax.config.json"
    }
  }
}"#,
        )
        .unwrap();

        let err = load_global_state(&path.0).unwrap_err();

        assert!(matches!(
            err,
            GlobalStateError::InvalidDesignSystemId { .. }
        ));
        assert!(err.to_string().contains("Acme"));
    }

    #[test]
    fn missing_design_systems_defaults_to_empty() {
        let path = TestPath(temp_state_path("default-empty"));
        let language_id = LanguageId::try_from("compose").unwrap();
        let state = GlobalState {
            installed_languages: BTreeMap::from([(
                language_id,
                BTreeMap::from([(
                    "0.1.0".to_owned(),
                    InstalledLanguagePack {
                        install_dir: PathBuf::from("/tmp/compose"),
                    },
                )]),
            )]),
            ..GlobalState::default()
        };

        save_global_state(&path.0, &state).unwrap();
        let loaded = load_global_state(&path.0).unwrap();

        assert_eq!(loaded.installed_languages, state.installed_languages);
        assert!(loaded.design_systems.is_empty());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{
        ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
        ERROR_UNABLE_TO_REMOVE_REPLACED, is_documented_windows_partial_replace_failure,
    };
    use std::io;

    #[test]
    fn detects_documented_windows_partial_replace_errors() {
        for code in [
            ERROR_UNABLE_TO_REMOVE_REPLACED,
            ERROR_UNABLE_TO_MOVE_REPLACEMENT,
            ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
        ] {
            assert!(is_documented_windows_partial_replace_failure(
                &io::Error::from_raw_os_error(code),
            ));
        }
        assert!(!is_documented_windows_partial_replace_failure(
            &io::Error::from_raw_os_error(87),
        ));
    }
}
