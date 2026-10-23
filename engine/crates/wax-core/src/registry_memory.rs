//! Remembered design-system locations in global wax state.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use thiserror::Error;
use wax_contract::LanguageId;

use crate::config::repo_files::PREFERRED_CONFIG_RELATIVE_PATH;
use crate::global_state::{
    GlobalStateError, RememberedDesignSystem, load_global_state, save_global_state,
};
use crate::registry_cache::{
    RegistryCache, RegistryCacheError, RegistrySnapshot, RegistrySourceSpec,
};
use crate::registry_source::{
    RegistrySourceError, is_external_registry_source, reject_repo_relative_registry_path_escape,
    validate_repo_relative_registry_path_within_repo,
};
use crate::{AtomicWriteError, AtomicWriteOptions, write_atomically};

/// Repo-relative path recorded for remembered design systems.
pub const LAST_SEEN_CONFIG_RELATIVE_PATH: &str = PREFERRED_CONFIG_RELATIVE_PATH;

/// Relative registry path for a design-system-authored language registry.
pub fn design_system_registry_relative_path(language_id: &LanguageId) -> String {
    format!(".wax/registries/{}.json", language_id.as_str())
}

/// App-local registry path when copying from a remembered design system.
pub fn app_registry_relative_path(design_system_id: &str, language_id: &LanguageId) -> String {
    format!(
        ".wax/registries/{}/{}.json",
        design_system_id,
        language_id.as_str()
    )
}

/// Resolved registry inputs from a remembered design system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRememberedRegistry {
    /// Source written to app config `registry.source` when not using Git mode.
    pub config_source: String,
    /// Git remote written to app config `registry.git` when using Git mode.
    pub git: Option<String>,
    /// Git tag or pinned commit written to app config `registry.tag`.
    pub tag: Option<String>,
    /// Upstream id in `<design-system-id>/<language-id>` form.
    pub upstream: String,
    /// Local registry path relative to the design-system repo when copying is required.
    pub design_system_local_source: Option<String>,
    /// Snapshot digest used to pin the app lock for typed remote sources.
    pub digest: Option<String>,
    /// Selected Git commit when the remembered source is Git-backed.
    pub git_commit: Option<String>,
}

/// Returns the immutable blob path for a registry digest under a wax home.
#[must_use]
pub fn registry_blob_path(state_path: &Path, digest: &str) -> PathBuf {
    state_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("cache/registries/blobs")
        .join(format!("{digest}.json"))
}

/// Returns a `file://` URL for an immutable registry blob under a wax home.
#[must_use]
pub fn registry_blob_file_url(state_path: &Path, digest: &str) -> String {
    format!(
        "file://{}",
        registry_blob_path(state_path, digest).display()
    )
}

fn path_to_file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn path_or_url_resolution(
    config_source: String,
    upstream: String,
    design_system_local_source: Option<String>,
) -> ResolvedRememberedRegistry {
    ResolvedRememberedRegistry {
        config_source,
        git: None,
        tag: None,
        upstream,
        design_system_local_source,
        digest: None,
        git_commit: None,
    }
}

/// Errors returned while reading or updating remembered design systems.
#[derive(Debug, Error)]
pub enum RegistryMemoryError {
    /// Global wax state could not be loaded or saved.
    #[error(transparent)]
    GlobalState(#[from] GlobalStateError),
    /// The design-system id is not a valid lowercase ASCII slug.
    #[error(
        "design system id `{design_system_id}` is not valid; expected lowercase ASCII slug [a-z][a-z0-9-]*"
    )]
    InvalidDesignSystemId {
        /// Invalid design-system id string.
        design_system_id: String,
    },
    /// The remembered design system does not exist in global state.
    #[error("design system `{design_system_id}` is not remembered")]
    NotFound {
        /// Requested design-system id.
        design_system_id: String,
    },
    /// The repository root could not be canonicalized.
    #[error("failed to resolve repository root {path}: {source}")]
    ResolveRepoRoot {
        /// Repository root passed by the caller.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Wax config could not be read or updated.
    #[error("failed to update wax config at {path}: {source}")]
    ConfigUpdate {
        /// Config path that failed to update.
        path: String,
        /// Underlying I/O or parse failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Atomic replacement of a Wax config or registry file failed.
    #[error("failed to atomically update wax file at {path}: {source}")]
    AtomicWrite {
        /// Destination path.
        path: String,
        /// Atomic-write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// Remembered registry source path is invalid or escapes the design-system repo.
    #[error(transparent)]
    RegistrySource(#[from] RegistrySourceError),
    /// Immutable registry source resolution failed.
    #[error(transparent)]
    RegistryCache(#[from] RegistryCacheError),
    /// A remembered id is already associated with a different display name.
    #[error(
        "design system `{design_system_id}` is already remembered as `{existing_name}`; refusing conflicting name `{requested_name}`"
    )]
    ConflictingName {
        /// Existing remembered id.
        design_system_id: String,
        /// Existing display name.
        existing_name: String,
        /// Requested replacement display name.
        requested_name: String,
    },
    /// The remembered entry is local and cannot be refreshed as a typed source.
    #[error(
        "design system `{design_system_id}` is a local publisher; use `registry update --repo-root`"
    )]
    NotRemote {
        /// Requested remembered id.
        design_system_id: String,
    },
    /// The remembered entry is source-backed and has no publisher checkout root.
    #[error("design system `{design_system_id}` is source-backed; use `registry refresh`")]
    NotLocal {
        /// Requested remembered id.
        design_system_id: String,
    },
    /// A typed remembered source has no snapshot for the requested language.
    #[error("design system `{design_system_id}` has no registry for language `{language_id}`")]
    MissingLanguage {
        /// Remembered design-system id.
        design_system_id: String,
        /// Requested language id.
        language_id: LanguageId,
    },
    /// A typed remembered snapshot could not be resolved for app materialization.
    #[error("design system `{design_system_id}` snapshot `{digest}` is unavailable")]
    SnapshotUnavailable {
        /// Remembered design-system id.
        design_system_id: String,
        /// Missing snapshot digest.
        digest: String,
    },
}

/// Summary of one remembered design system for list/show output.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RememberedDesignSystemSummary {
    /// Design-system id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Canonical repository root.
    pub repo_root: PathBuf,
    /// Repo-relative path to the last seen config file.
    pub last_seen_config: PathBuf,
    /// Typed independent registry source, when this is not a legacy local publisher.
    pub source: Option<RegistrySourceSpec>,
    /// Immutable snapshots selected by the most recent add or refresh.
    pub snapshots: Vec<wax_contract::RegistrySnapshotRef>,
    /// State file used to locate the shared immutable snapshot cache.
    pub state_path: PathBuf,
}

/// Stores or refreshes a remembered design system in global state.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::InvalidDesignSystemId`] for an invalid id,
/// [`RegistryMemoryError::ResolveRepoRoot`] when the repository cannot be
/// canonicalized, or [`RegistryMemoryError::GlobalState`] when state cannot be
/// loaded or saved.
pub fn remember_design_system(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
    name: &str,
    repo_root: impl AsRef<Path>,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let repo_root = canonicalize_repo_root(repo_root.as_ref())?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    state.design_systems.insert(
        design_system_id.to_owned(),
        RememberedDesignSystem {
            name: name.to_owned(),
            repo_root,
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
            source: None,
            snapshots: Vec::new(),
        },
    );
    save_global_state(state_path, &state)?;
    Ok(())
}

/// Fetches and remembers an independent registry source under a design-system id.
///
/// Existing entries may be refreshed with the same display name. A different name
/// is rejected so a remembered id cannot silently change identity.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::ConflictingName`] for an existing id with a
/// different name, or a typed state/cache error when persistence or source
/// resolution fails.
pub fn add_remembered_registry(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
    name: &str,
    source: &RegistrySourceSpec,
) -> Result<Vec<RegistrySnapshot>, RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    if let Some(existing) = state.design_systems.get(design_system_id)
        && existing.name != name
    {
        return Err(RegistryMemoryError::ConflictingName {
            design_system_id: design_system_id.to_owned(),
            existing_name: existing.name.clone(),
            requested_name: name.to_owned(),
        });
    }
    let snapshots = registry_cache_for_state(state_path).refresh(source)?;
    state.design_systems.insert(
        design_system_id.to_owned(),
        RememberedDesignSystem {
            name: name.to_owned(),
            repo_root: PathBuf::new(),
            last_seen_config: PathBuf::new(),
            source: Some(source.clone()),
            snapshots: snapshots
                .iter()
                .map(|snapshot| snapshot.reference.clone())
                .collect(),
        },
    );
    save_global_state(state_path, &state)?;
    Ok(snapshots)
}

/// Refetches a typed remembered source and updates its selected snapshots.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::NotFound`] for an unknown id,
/// [`RegistryMemoryError::NotRemote`] for a legacy local publisher, or a typed
/// state/cache error when refresh or persistence fails.
pub fn refresh_remembered_registry(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
) -> Result<Vec<RegistrySnapshot>, RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    let entry = state
        .design_systems
        .get_mut(design_system_id)
        .ok_or_else(|| RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        })?;
    let source = entry
        .source
        .as_ref()
        .ok_or_else(|| RegistryMemoryError::NotRemote {
            design_system_id: design_system_id.to_owned(),
        })?;
    let snapshots = registry_cache_for_state(state_path).refresh(source)?;
    entry.snapshots = snapshots
        .iter()
        .map(|snapshot| snapshot.reference.clone())
        .collect();
    save_global_state(state_path, &state)?;
    Ok(snapshots)
}

fn registry_cache_for_state(state_path: &Path) -> RegistryCache {
    let home = state_path.parent().unwrap_or_else(|| Path::new("."));
    RegistryCache::open(home.join("cache/registries"))
}

/// Lists remembered design systems sorted by id.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::GlobalState`] when global state cannot be
/// loaded or validated.
pub fn list_remembered_design_systems(
    state_path: impl AsRef<Path>,
) -> Result<Vec<RememberedDesignSystemSummary>, RegistryMemoryError> {
    let state_path = state_path.as_ref();
    let state = load_global_state(state_path)?;
    Ok(state
        .design_systems
        .iter()
        .map(|(id, entry)| RememberedDesignSystemSummary {
            id: id.clone(),
            name: entry.name.clone(),
            repo_root: entry.repo_root.clone(),
            last_seen_config: entry.last_seen_config.clone(),
            source: entry.source.clone(),
            snapshots: entry.snapshots.clone(),
            state_path: state_path.to_owned(),
        })
        .collect())
}

/// Returns one remembered design system by id.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::InvalidDesignSystemId`] for an invalid id,
/// [`RegistryMemoryError::NotFound`] when the id is not remembered, or
/// [`RegistryMemoryError::GlobalState`] when state cannot be loaded.
pub fn show_remembered_design_system(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
) -> Result<RememberedDesignSystemSummary, RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state_path = state_path.as_ref();
    let state = load_global_state(state_path)?;
    let Some(entry) = state.design_systems.get(design_system_id) else {
        return Err(RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        });
    };
    Ok(RememberedDesignSystemSummary {
        id: design_system_id.to_owned(),
        name: entry.name.clone(),
        repo_root: entry.repo_root.clone(),
        last_seen_config: entry.last_seen_config.clone(),
        source: entry.source.clone(),
        snapshots: entry.snapshots.clone(),
        state_path: state_path.to_owned(),
    })
}

/// Updates the remembered repository root for a design system.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::InvalidDesignSystemId`] for an invalid id,
/// [`RegistryMemoryError::ResolveRepoRoot`] for an inaccessible repository,
/// [`RegistryMemoryError::NotFound`] for an unknown id, or
/// [`RegistryMemoryError::GlobalState`] when state cannot be loaded or saved.
pub fn update_remembered_design_system_repo_root(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
    repo_root: impl AsRef<Path>,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    let Some(entry) = state.design_systems.get_mut(design_system_id) else {
        return Err(RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        });
    };
    if entry.source.is_some() {
        return Err(RegistryMemoryError::NotLocal {
            design_system_id: design_system_id.to_owned(),
        });
    }
    entry.repo_root = canonicalize_repo_root(repo_root.as_ref())?;
    save_global_state(state_path, &state)?;
    Ok(())
}

/// Removes a remembered design system from global state.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::InvalidDesignSystemId`] for an invalid id,
/// [`RegistryMemoryError::NotFound`] for an unknown id, or
/// [`RegistryMemoryError::GlobalState`] when state cannot be loaded or saved.
pub fn delete_remembered_design_system(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    if state.design_systems.remove(design_system_id).is_none() {
        return Err(RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        });
    };
    save_global_state(state_path, &state)?;
    Ok(())
}

/// Resolves registry source and upstream metadata for one remembered design-system language.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::InvalidDesignSystemId`] for an invalid
/// remembered id, [`RegistryMemoryError::MissingLanguage`] or
/// [`RegistryMemoryError::SnapshotUnavailable`] for typed remote resolution
/// misses, [`RegistryMemoryError::ConfigUpdate`] for missing or malformed
/// publication config, or [`RegistryMemoryError::RegistrySource`] for an unsafe
/// local registry source.
pub fn resolve_remembered_registry(
    remembered: &RememberedDesignSystemSummary,
    language_id: &LanguageId,
) -> Result<ResolvedRememberedRegistry, RegistryMemoryError> {
    validate_design_system_id(&remembered.id)?;
    if let Some(source) = remembered.source.as_ref() {
        return resolve_source_backed_remembered(remembered, source, language_id);
    }
    let config_path = remembered.repo_root.join(&remembered.last_seen_config);
    let path_display = config_path.display().to_string();
    let config = load_or_create_config(&config_path, &path_display)?;
    let design_systems = config
        .get("design_systems")
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "wax config missing design_systems object",
            )),
        })?;
    let entry =
        design_systems
            .get(&remembered.id)
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("design system `{}` missing from wax config", remembered.id),
                )),
            })?;
    let registries = entry
        .get("registries")
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "design system `{}` missing registries object",
                    remembered.id
                ),
            )),
        })?;
    let registry = registries
        .get(language_id.as_str())
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "design system `{}` missing registry for language `{}`",
                    remembered.id,
                    language_id.as_str()
                ),
            )),
        })?;
    let local_source = registry
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "design system `{}` registry for `{}` missing source",
                    remembered.id,
                    language_id.as_str()
                ),
            )),
        })?
        .to_owned();
    let published_source = registry
        .get("published_source")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let upstream = format!("{}/{}", remembered.id, language_id.as_str());
    if let Some(published_source) = published_source.filter(|value| !value.trim().is_empty()) {
        Ok(path_or_url_resolution(published_source, upstream, None))
    } else if is_external_registry_source(&local_source) {
        Ok(path_or_url_resolution(local_source, upstream, None))
    } else {
        reject_repo_relative_registry_path_escape(&local_source)?;
        Ok(path_or_url_resolution(
            app_registry_relative_path(&remembered.id, language_id),
            upstream,
            Some(local_source),
        ))
    }
}

fn resolve_source_backed_remembered(
    remembered: &RememberedDesignSystemSummary,
    source: &RegistrySourceSpec,
    language_id: &LanguageId,
) -> Result<ResolvedRememberedRegistry, RegistryMemoryError> {
    let snapshot = remembered
        .snapshots
        .iter()
        .find(|snapshot| &snapshot.language == language_id)
        .ok_or_else(|| RegistryMemoryError::MissingLanguage {
            design_system_id: remembered.id.clone(),
            language_id: language_id.clone(),
        })?;
    let upstream = format!("{}/{}", remembered.id, language_id.as_str());
    match source {
        RegistrySourceSpec::HttpJson { url, .. } => Ok(ResolvedRememberedRegistry {
            config_source: url.clone(),
            git: None,
            tag: None,
            upstream,
            design_system_local_source: None,
            digest: Some(snapshot.digest.clone()),
            git_commit: None,
        }),
        RegistrySourceSpec::LocalFile { path, .. } => Ok(ResolvedRememberedRegistry {
            config_source: path_to_file_url(path),
            git: None,
            tag: None,
            upstream,
            design_system_local_source: None,
            digest: Some(snapshot.digest.clone()),
            git_commit: None,
        }),
        RegistrySourceSpec::Git { url, tag, .. } => {
            let pinned_tag = tag
                .clone()
                .or_else(|| snapshot.git_commit.clone())
                .ok_or_else(|| RegistryMemoryError::SnapshotUnavailable {
                    design_system_id: remembered.id.clone(),
                    digest: snapshot.digest.clone(),
                })?;
            Ok(ResolvedRememberedRegistry {
                config_source: String::new(),
                git: Some(url.clone()),
                tag: Some(pinned_tag),
                upstream,
                design_system_local_source: None,
                digest: Some(snapshot.digest.clone()),
                git_commit: snapshot.git_commit.clone(),
            })
        }
        RegistrySourceSpec::PublisherRepository { .. } => {
            Err(RegistryMemoryError::MissingLanguage {
                design_system_id: remembered.id.clone(),
                language_id: language_id.clone(),
            })
        }
    }
}

/// Copies a design-system registry artifact into an app repository.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::RegistrySource`] when the source path is
/// unsafe or escapes the design-system repository, or
/// [`RegistryMemoryError::ConfigUpdate`] when the source cannot be read, or
/// [`RegistryMemoryError::AtomicWrite`] when the app destination cannot be
/// created or written atomically.
pub fn copy_design_system_registry_to_app(
    remembered: &RememberedDesignSystemSummary,
    design_system_local_source: &str,
    app_repo_root: &Path,
    app_registry_relative: &str,
) -> Result<(), RegistryMemoryError> {
    if remembered.source.is_some() {
        let snapshot = remembered
            .snapshots
            .iter()
            .find(|snapshot| snapshot.digest == design_system_local_source)
            .ok_or_else(|| RegistryMemoryError::SnapshotUnavailable {
                design_system_id: remembered.id.clone(),
                digest: design_system_local_source.to_owned(),
            })?;
        let source_path = registry_blob_path(&remembered.state_path, &snapshot.digest);
        let destination_path = app_repo_root.join(app_registry_relative);
        let contents =
            fs::read(&source_path).map_err(|source| RegistryMemoryError::ConfigUpdate {
                path: source_path.display().to_string(),
                source: Box::new(source),
            })?;
        return write_atomically(&destination_path, &contents, AtomicWriteOptions::default())
            .map_err(|source| RegistryMemoryError::AtomicWrite {
                path: destination_path.display().to_string(),
                source,
            });
    }
    let source_path = validate_repo_relative_registry_path_within_repo(
        &remembered.repo_root,
        design_system_local_source,
    )?;
    let destination_path = app_repo_root.join(app_registry_relative);
    let destination_display = destination_path.display().to_string();
    let contents =
        fs::read_to_string(&source_path).map_err(|source| RegistryMemoryError::ConfigUpdate {
            path: source_path.display().to_string(),
            source: Box::new(source),
        })?;
    write_atomically(
        &destination_path,
        format!("{contents}\n").as_bytes(),
        AtomicWriteOptions::default(),
    )
    .map_err(|source| RegistryMemoryError::AtomicWrite {
        path: destination_display,
        source,
    })?;
    Ok(())
}

/// Ensures a design-system registry source exists in `.wax/wax.config.json`.
///
/// Creates the config file when missing and preserves unrelated fields.
///
/// # Errors
///
/// Returns [`RegistryMemoryError::InvalidDesignSystemId`] for an invalid id or
/// [`RegistryMemoryError::ConfigUpdate`] when config JSON cannot be read,
/// normalized, serialized, or written.
pub fn ensure_design_system_registry_source(
    repo_root: impl AsRef<Path>,
    design_system_id: &str,
    design_system_name: &str,
    language_id: &LanguageId,
    registry_source: &str,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let repo_root = repo_root.as_ref();
    let config_path = repo_root.join(LAST_SEEN_CONFIG_RELATIVE_PATH);
    let path_display = config_path.display().to_string();

    let mut config = load_or_create_config(&config_path, &path_display)?;
    let root_object = config
        .as_object_mut()
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "wax config root must be an object",
            )),
        })?;
    if !root_object.contains_key("schema_version") {
        root_object.insert("schema_version".to_owned(), Value::Number(2.into()));
    }
    let design_systems = root_object
        .entry("design_systems".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let design_systems =
        design_systems
            .as_object_mut()
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "wax config design_systems must be an object",
                )),
            })?;

    let entry = design_systems
        .entry(design_system_id.to_owned())
        .or_insert_with(|| {
            Value::Object(Map::from_iter([
                (
                    "name".to_owned(),
                    Value::String(design_system_name.to_owned()),
                ),
                ("registries".to_owned(), Value::Object(Map::new())),
            ]))
        });
    let entry_object = entry
        .as_object_mut()
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "design_systems entry is not an object",
            )),
        })?;
    entry_object.insert(
        "name".to_owned(),
        Value::String(design_system_name.to_owned()),
    );

    let registries = entry_object
        .entry("registries".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let registries_object =
        registries
            .as_object_mut()
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "design_systems registries is not an object",
                )),
            })?;
    let language_key = language_id.as_str().to_owned();
    let registry_entry = registries_object
        .entry(language_key)
        .or_insert_with(|| Value::Object(Map::new()));
    let registry_object =
        registry_entry
            .as_object_mut()
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "design_systems registry entry is not an object",
                )),
            })?;
    registry_object.insert(
        "source".to_owned(),
        Value::String(registry_source.to_owned()),
    );

    write_config(&config_path, &path_display, &config)?;
    Ok(())
}

fn validate_design_system_id(design_system_id: &str) -> Result<(), RegistryMemoryError> {
    if LanguageId::try_from(design_system_id).is_err() {
        return Err(RegistryMemoryError::InvalidDesignSystemId {
            design_system_id: design_system_id.to_owned(),
        });
    }
    Ok(())
}

fn canonicalize_repo_root(repo_root: &Path) -> Result<PathBuf, RegistryMemoryError> {
    fs::canonicalize(repo_root).map_err(|source| RegistryMemoryError::ResolveRepoRoot {
        path: repo_root.display().to_string(),
        source,
    })
}

fn load_or_create_config(
    config_path: &Path,
    path_display: &str,
) -> Result<Value, RegistryMemoryError> {
    match fs::read_to_string(config_path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|source| RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Ok(Value::Object(Map::from_iter([
                ("schema_version".to_owned(), Value::Number(2.into())),
                ("design_systems".to_owned(), Value::Object(Map::new())),
            ])))
        }
        Err(source) => Err(RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }),
    }
}

fn write_config(
    config_path: &Path,
    path_display: &str,
    config: &Value,
) -> Result<(), RegistryMemoryError> {
    let serialized = serde_json::to_string_pretty(config).map_err(|source| {
        RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }
    })?;
    write_atomically(
        config_path,
        format!("{serialized}\n").as_bytes(),
        AtomicWriteOptions::default(),
    )
    .map_err(|source| RegistryMemoryError::AtomicWrite {
        path: path_display.to_owned(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod registry_memory_tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "wax-core-registry-memory-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn remember_list_show_update_delete_roundtrip() {
        let _guard = env_lock();
        let dir = TestDir::new("roundtrip");
        let state_path = dir.path.join("state.json");
        let repo = dir.path.join("acme-ds");
        std::fs::create_dir_all(&repo).unwrap();

        remember_design_system(&state_path, "acme", "Acme Design System", &repo).unwrap();

        let listed = list_remembered_design_systems(&state_path).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "acme");
        assert_eq!(listed[0].name, "Acme Design System");
        assert_eq!(
            listed[0].last_seen_config,
            PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH)
        );

        let shown = show_remembered_design_system(&state_path, "acme").unwrap();
        assert_eq!(shown.name, "Acme Design System");
        assert!(shown.repo_root.ends_with("acme-ds"));

        let updated_repo = dir.path.join("acme-ds-moved");
        std::fs::create_dir_all(&updated_repo).unwrap();
        update_remembered_design_system_repo_root(&state_path, "acme", &updated_repo).unwrap();
        let updated = show_remembered_design_system(&state_path, "acme").unwrap();
        assert!(updated.repo_root.ends_with("acme-ds-moved"));

        delete_remembered_design_system(&state_path, "acme").unwrap();
        assert!(
            list_remembered_design_systems(&state_path)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_design_system_ids() {
        let _guard = env_lock();
        let dir = TestDir::new("invalid-id");
        let state_path = dir.path.join("state.json");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let err = remember_design_system(&state_path, "Acme", "Acme", &repo).unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::InvalidDesignSystemId { .. }
        ));
    }

    #[test]
    fn show_and_delete_report_missing_entries() {
        let _guard = env_lock();
        let dir = TestDir::new("missing");
        let state_path = dir.path.join("state.json");

        let err = show_remembered_design_system(&state_path, "acme").unwrap_err();
        assert!(matches!(err, RegistryMemoryError::NotFound { .. }));

        let err = delete_remembered_design_system(&state_path, "acme").unwrap_err();
        assert!(matches!(err, RegistryMemoryError::NotFound { .. }));
    }

    #[test]
    fn ensure_design_system_registry_source_creates_config() {
        let _guard = env_lock();
        let dir = TestDir::new("config-create");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let language_id = LanguageId::try_from("react").unwrap();

        ensure_design_system_registry_source(
            &repo,
            "acme",
            "Acme Design System",
            &language_id,
            ".wax/registries/react.json",
        )
        .unwrap();

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(config["schema_version"], 2);
        assert_eq!(
            config["design_systems"]["acme"]["name"],
            "Acme Design System"
        );
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["source"],
            ".wax/registries/react.json"
        );
    }

    #[test]
    fn ensure_design_system_registry_source_merges_existing_config() {
        let _guard = env_lock();
        let dir = TestDir::new("config-merge");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "engine": { "scan_concurrency": 4 },
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "compose": {
          "source": ".wax/registries/compose.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
        let react_id = LanguageId::try_from("react").unwrap();

        ensure_design_system_registry_source(
            &repo,
            "acme",
            "Acme Design System",
            &react_id,
            ".wax/registries/react.json",
        )
        .unwrap();

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(config["engine"]["scan_concurrency"], 4);
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["compose"]["source"],
            ".wax/registries/compose.json"
        );
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["source"],
            ".wax/registries/react.json"
        );
    }

    #[test]
    fn ensure_design_system_registry_source_preserves_published_source() {
        let _guard = env_lock();
        let dir = TestDir::new("preserve-published-source");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react-old.json",
          "published_source": "https://cdn.example.com/acme/react.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
        let react_id = LanguageId::try_from("react").unwrap();

        ensure_design_system_registry_source(
            &repo,
            "acme",
            "Acme Design System",
            &react_id,
            ".wax/registries/react.json",
        )
        .unwrap();

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["source"],
            ".wax/registries/react.json"
        );
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["published_source"],
            "https://cdn.example.com/acme/react.registry.json"
        );
    }

    #[test]
    fn design_system_registry_relative_path_matches_spec() {
        let language_id = LanguageId::try_from("react").unwrap();
        assert_eq!(
            design_system_registry_relative_path(&language_id),
            ".wax/registries/react.json"
        );
    }

    #[test]
    fn resolve_remembered_registry_uses_external_source_directly() {
        let _guard = env_lock();
        let dir = TestDir::new("remembered-external-source");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": "file:///tmp/acme-react.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();

        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme Design System".to_owned(),
            repo_root: repo,
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
            source: None,
            snapshots: Vec::new(),
            state_path: dir.path.join("state.json"),
        };
        let language_id = LanguageId::try_from("react").unwrap();

        let resolved = resolve_remembered_registry(&remembered, &language_id).unwrap();
        assert_eq!(
            resolved.config_source,
            "file:///tmp/acme-react.registry.json"
        );
        assert_eq!(resolved.upstream, "acme/react");
        assert!(resolved.design_system_local_source.is_none());
        assert!(resolved.git.is_none());
    }

    #[test]
    fn resolve_source_backed_remembered_writes_explicit_file_url() {
        let _guard = env_lock();
        let dir = TestDir::new("source-backed-file-url");
        let source = dir.path.join("compose.json");
        let bytes = br#"{"schema_version":1,"language":"compose","components":[{"id":"one","symbol":"One"}]}"#;
        std::fs::write(&source, bytes).unwrap();
        let state_path = dir.path.join("state.json");
        let snapshots = add_remembered_registry(
            &state_path,
            "acme",
            "Acme",
            &RegistrySourceSpec::LocalFile {
                path: source.clone(),
                language: None,
            },
        )
        .unwrap();
        let remembered = show_remembered_design_system(&state_path, "acme").unwrap();
        let language_id = LanguageId::try_from("compose").unwrap();

        let resolved = resolve_remembered_registry(&remembered, &language_id).unwrap();

        assert_eq!(
            resolved.config_source,
            format!("file://{}", source.display())
        );
        assert!(resolved.git.is_none());
        assert!(resolved.design_system_local_source.is_none());
        assert_eq!(
            resolved.digest.as_deref(),
            Some(snapshots[0].reference.digest.as_str())
        );
        assert_eq!(resolved.upstream, "acme/compose");
    }

    #[test]
    fn resolve_source_backed_remembered_pins_default_branch_git_commit() {
        let _guard = env_lock();
        let dir = TestDir::new("source-backed-git");
        let language_id = LanguageId::try_from("compose").unwrap();
        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme".to_owned(),
            repo_root: PathBuf::new(),
            last_seen_config: PathBuf::new(),
            source: Some(RegistrySourceSpec::Git {
                url: "https://example.com/acme.git".to_owned(),
                tag: None,
                language: language_id.clone(),
            }),
            snapshots: vec![wax_contract::RegistrySnapshotRef {
                source: "git:https://example.com/acme.git".to_owned(),
                language: language_id.clone(),
                digest: "a".repeat(64),
                system_id: None,
                git_commit: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()),
                extra: serde_json::Map::new(),
            }],
            state_path: dir.path.join("state.json"),
        };

        let resolved = resolve_remembered_registry(&remembered, &language_id).unwrap();

        assert_eq!(
            resolved.git.as_deref(),
            Some("https://example.com/acme.git")
        );
        assert_eq!(
            resolved.tag.as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert_eq!(
            resolved.git_commit.as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert!(resolved.design_system_local_source.is_none());
        assert!(resolved.config_source.is_empty());
    }

    #[test]
    fn resolve_source_backed_missing_language_is_typed() {
        let _guard = env_lock();
        let dir = TestDir::new("source-backed-missing-lang");
        let language_id = LanguageId::try_from("compose").unwrap();
        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme".to_owned(),
            repo_root: PathBuf::new(),
            last_seen_config: PathBuf::new(),
            source: Some(RegistrySourceSpec::HttpJson {
                url: "https://cdn.example.com/compose.json".to_owned(),
                language: Some(language_id.clone()),
            }),
            snapshots: Vec::new(),
            state_path: dir.path.join("state.json"),
        };

        let err = resolve_remembered_registry(&remembered, &language_id).unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::MissingLanguage {
                design_system_id,
                ..
            } if design_system_id == "acme"
        ));
    }

    #[test]
    fn resolve_remembered_registry_rejects_path_that_escapes_design_system_repo() {
        let _guard = env_lock();
        let dir = TestDir::new("remembered-path-escape");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": "../outside.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();

        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme Design System".to_owned(),
            repo_root: repo,
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
            source: None,
            snapshots: Vec::new(),
            state_path: dir.path.join("state.json"),
        };
        let language_id = LanguageId::try_from("react").unwrap();

        let err = resolve_remembered_registry(&remembered, &language_id).unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::RegistrySource(RegistrySourceError::PathEscapesRepo { .. })
        ));
    }

    #[test]
    fn copy_design_system_registry_rejects_path_that_escapes_design_system_repo() {
        let _guard = env_lock();
        let dir = TestDir::new("copy-path-escape");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let outside = dir.path.join("outside.registry.json");
        std::fs::write(&outside, r#"{"schema_version":1,"components":[]}"#).unwrap();

        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme Design System".to_owned(),
            repo_root: repo,
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
            source: None,
            snapshots: Vec::new(),
            state_path: dir.path.join("state.json"),
        };
        let language_id = LanguageId::try_from("react").unwrap();

        let err = copy_design_system_registry_to_app(
            &remembered,
            "../outside.registry.json",
            dir.path.join("app").as_path(),
            &app_registry_relative_path("acme", &language_id),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::RegistrySource(RegistrySourceError::PathEscapesRepo { .. })
        ));
    }

    #[test]
    fn remembered_entries_do_not_copy_registry_json() {
        let _guard = env_lock();
        let dir = TestDir::new("no-registry-copy");
        let state_path = dir.path.join("state.json");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        remember_design_system(&state_path, "acme", "Acme", &repo).unwrap();

        let raw = std::fs::read_to_string(&state_path).unwrap();
        assert!(!raw.contains("components"));
        assert!(!raw.contains("registries/react.json"));

        let state = load_global_state(&state_path).unwrap();
        assert_eq!(state.design_systems.len(), 1);
        assert!(state.installed_languages.is_empty());
        assert!(state.design_systems.contains_key("acme"));
    }

    #[test]
    fn old_local_entries_load_and_preserve_installed_languages() {
        let _guard = env_lock();
        let dir = TestDir::new("legacy-state");
        let state_path = dir.path.join("state.json");
        let language_id = LanguageId::try_from("compose").unwrap();
        std::fs::write(
            &state_path,
            format!(
                r#"{{
  "installed_languages": {{
    "compose": {{
      "0.1.0": {{ "install_dir": "{}/langs/compose/0.1.0" }}
    }}
  }},
  "design_systems": {{
    "acme": {{
      "name": "Acme Design System",
      "repo_root": "{}/acme-ds",
      "last_seen_config": ".wax/wax.config.json"
    }}
  }}
}}"#,
                dir.path.display(),
                dir.path.display()
            ),
        )
        .unwrap();

        let loaded = load_global_state(&state_path).unwrap();
        assert!(loaded.design_systems["acme"].source.is_none());
        assert_eq!(
            loaded.installed_languages[&language_id]["0.1.0"].install_dir,
            dir.path.join("langs/compose/0.1.0")
        );

        let source = dir.path.join("remote.json");
        std::fs::write(
            &source,
            br#"{"schema_version":1,"language":"compose","components":[{"id":"one","symbol":"One"}]}"#,
        )
        .unwrap();
        add_remembered_registry(
            &state_path,
            "remote",
            "Remote",
            &RegistrySourceSpec::LocalFile {
                path: source,
                language: None,
            },
        )
        .unwrap();

        let after = load_global_state(&state_path).unwrap();
        assert!(after.design_systems["acme"].source.is_none());
        assert!(after.design_systems["remote"].source.is_some());
        assert_eq!(
            after.installed_languages[&language_id]["0.1.0"].install_dir,
            dir.path.join("langs/compose/0.1.0")
        );
    }

    #[test]
    fn conflicting_remembered_name_is_rejected() {
        let _guard = env_lock();
        let dir = TestDir::new("conflict-name");
        let state_path = dir.path.join("state.json");
        let source = dir.path.join("compose.json");
        std::fs::write(
            &source,
            br#"{"schema_version":1,"language":"compose","components":[{"id":"one","symbol":"One"}]}"#,
        )
        .unwrap();
        add_remembered_registry(
            &state_path,
            "acme",
            "Acme",
            &RegistrySourceSpec::LocalFile {
                path: source.clone(),
                language: None,
            },
        )
        .unwrap();

        let err = add_remembered_registry(
            &state_path,
            "acme",
            "Other Name",
            &RegistrySourceSpec::LocalFile {
                path: source,
                language: None,
            },
        )
        .unwrap_err();

        assert!(matches!(
            err,
            RegistryMemoryError::ConflictingName {
                design_system_id,
                existing_name,
                requested_name,
            } if design_system_id == "acme"
                && existing_name == "Acme"
                && requested_name == "Other Name"
        ));
    }

    #[test]
    fn forgetting_name_leaves_shared_blobs_intact() {
        let _guard = env_lock();
        let dir = TestDir::new("forget-blobs");
        let state_path = dir.path.join("state.json");
        let source = dir.path.join("compose.json");
        let bytes =
            br#"{"schema_version":1,"language":"compose","components":[{"id":"one","symbol":"One"}]}"#;
        std::fs::write(&source, bytes).unwrap();
        let snapshots = add_remembered_registry(
            &state_path,
            "acme",
            "Acme",
            &RegistrySourceSpec::LocalFile {
                path: source,
                language: None,
            },
        )
        .unwrap();
        let digest = &snapshots[0].reference.digest;
        let blob = dir
            .path
            .join("cache/registries/blobs")
            .join(format!("{digest}.json"));
        assert!(blob.is_file());

        delete_remembered_design_system(&state_path, "acme").unwrap();

        assert!(
            list_remembered_design_systems(&state_path)
                .unwrap()
                .is_empty()
        );
        assert!(blob.is_file(), "shared immutable blobs must survive forget");
        assert_eq!(std::fs::read(&blob).unwrap(), bytes);
    }

    #[test]
    fn update_repo_root_rejects_source_backed_entries() {
        let _guard = env_lock();
        let dir = TestDir::new("update-remote");
        let state_path = dir.path.join("state.json");
        let source = dir.path.join("compose.json");
        std::fs::write(
            &source,
            br#"{"schema_version":1,"language":"compose","components":[{"id":"one","symbol":"One"}]}"#,
        )
        .unwrap();
        add_remembered_registry(
            &state_path,
            "acme",
            "Acme",
            &RegistrySourceSpec::LocalFile {
                path: source,
                language: None,
            },
        )
        .unwrap();

        let err =
            update_remembered_design_system_repo_root(&state_path, "acme", dir.path.join("moved"))
                .unwrap_err();

        assert!(matches!(
            err,
            RegistryMemoryError::NotLocal {
                design_system_id
            } if design_system_id == "acme"
        ));
    }
}
