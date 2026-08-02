//! App registry sync from remembered design systems.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;
use wax_contract::LanguageId;

use crate::config::lockfile::{LockedRegistry, WAX_LOCK_SCHEMA_VERSION, WaxLock, load_lockfile};
use crate::config::repo_files::discover_repo_files;
use crate::config::waxrc::{LanguageEntry, WaxRc, WaxRcError, load_waxrc};
use crate::registry_memory::{
    RegistryMemoryError, copy_design_system_registry_to_app, resolve_remembered_registry,
    show_remembered_design_system,
};
use crate::registry_source::resolve_language_registry_source;
use crate::{AtomicWriteError, AtomicWriteOptions, paths::PathsError, write_atomically};

/// Options for syncing app registries from remembered design systems.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Repository root containing `.wax/wax.config.json` and `.wax/wax.lock.json`.
    pub repo_root: PathBuf,
    /// Optional global wax state path override containing remembered design systems.
    ///
    /// When absent, global state is resolved only if an upstream registry needs it.
    pub state_path: Option<PathBuf>,
    /// Refresh Git-backed registry tags instead of using their locked commits.
    pub upgrade: bool,
}

/// One language registry refreshed during sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncUpdate {
    /// Language id whose registry inputs were refreshed.
    pub language_id: LanguageId,
    /// Upstream reference in `<design-system-id>/<language-id>` form.
    pub upstream: String,
    /// Registry source written to app config after sync.
    pub source: String,
    /// Configured Git remote when this update came from a Git registry.
    pub git: Option<String>,
    /// Configured Git tag when this update came from a Git registry.
    pub tag: Option<String>,
    /// Commit pinned before this sync, when present.
    pub old_commit: Option<String>,
    /// Commit pinned by this sync, when this is a Git registry.
    pub new_commit: Option<String>,
}

#[derive(Debug, Clone)]
struct RegistryCopyPlan {
    remembered: crate::registry_memory::RememberedDesignSystemSummary,
    design_system_local_source: String,
    app_registry_relative: String,
}

#[derive(Debug, Clone)]
struct PreparedLockFields {
    source: String,
    sha256: String,
    git_commit: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedSync {
    update: SyncUpdate,
    registry_copy: Option<RegistryCopyPlan>,
    /// Prepare-time resolution reused when writing the lock, when present.
    prepared_lock: Option<PreparedLockFields>,
}

#[derive(Debug, Clone)]
struct RegistryFileBackup {
    path: PathBuf,
    previous: Option<Vec<u8>>,
}

struct PersistSyncInput<'a> {
    options: &'a SyncOptions,
    repo_files: &'a crate::config::repo_files::RepoFileSet,
    config_path_display: &'a str,
    config_json: &'a Value,
    config_changed: bool,
    original_config_json: &'a Value,
    lockfile: &'a mut WaxLock,
    prepared_updates: &'a [PreparedSync],
}

type BestEffortSyncResult = Result<(Vec<SyncUpdate>, Vec<(String, SyncError)>), SyncError>;

/// Errors returned while syncing app registries.
#[derive(Debug, Error)]
pub enum SyncError {
    /// Global wax paths could not be resolved for an upstream registry.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Wax config could not be loaded.
    #[error(transparent)]
    Config(#[from] WaxRcError),
    /// Lockfile could not be loaded or saved.
    #[error(transparent)]
    Lockfile(#[from] crate::config::lockfile::LockfileError),
    /// Remembered design-system resolution failed.
    #[error(transparent)]
    RegistryMemory(#[from] RegistryMemoryError),
    /// Registry source resolution failed.
    #[error(transparent)]
    RegistrySource(#[from] crate::registry_source::RegistrySourceError),
    /// Wax config is missing from the repository.
    #[error("wax config not found at {path}")]
    MissingConfig {
        /// Expected config path.
        path: PathBuf,
    },
    /// Wax lockfile is missing from the repository.
    #[error("wax lockfile not found at {path}")]
    MissingLockfile {
        /// Expected lockfile path.
        path: PathBuf,
    },
    /// Upstream metadata could not be parsed for a language entry.
    #[error(
        "invalid registry upstream `{upstream}` for language `{language_id}`; expected `<design-system-id>/<language-id>`"
    )]
    InvalidUpstream {
        /// Upstream string from config.
        upstream: String,
        /// Language id from config.
        language_id: LanguageId,
    },
    /// Wax config could not be read or updated on disk.
    #[error("failed to update wax config at {path}: {source}")]
    ConfigUpdate {
        /// Config path that failed to update.
        path: String,
        /// Underlying failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Atomic config replacement failed.
    #[error("failed to atomically update wax config at {path}: {source}")]
    ConfigAtomicWrite {
        /// Config path that failed to update.
        path: String,
        /// Atomic-write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// Lockfile could not be written to disk.
    #[error("failed to write wax lockfile at {path}: {source}")]
    LockfileWrite {
        /// Lockfile path that failed to write.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Atomic lockfile replacement failed.
    #[error("failed to atomically write wax lockfile at {path}: {source}")]
    LockfileAtomicWrite {
        /// Lockfile path that failed to write.
        path: String,
        /// Atomic-write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// Atomic registry rollback restoration failed.
    #[error("failed to atomically restore registry backup at {path}: {source}")]
    RegistryRestoreAtomicWrite {
        /// Registry path that failed to restore.
        path: String,
        /// Atomic-write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// Registry lock refresh failed after resolving upstream registry inputs.
    #[error("failed to refresh registry lock for {upstream}: {message}")]
    LockRefreshFailed {
        /// Upstream reference that failed to refresh.
        upstream: String,
        /// Underlying failure summary.
        message: String,
    },
}

/// Refreshes app registry inputs for configured upstream and Git references.
///
/// # Errors
///
/// Returns [`SyncError::MissingConfig`] or [`SyncError::MissingLockfile`] for
/// missing inputs; [`SyncError::Config`], [`SyncError::Lockfile`],
/// [`SyncError::RegistryMemory`], [`SyncError::RegistrySource`], or
/// [`SyncError::InvalidUpstream`] while preparing updates; and
/// [`SyncError::ConfigUpdate`], [`SyncError::ConfigAtomicWrite`],
/// [`SyncError::LockfileWrite`], [`SyncError::LockfileAtomicWrite`],
/// [`SyncError::RegistryRestoreAtomicWrite`], or [`SyncError::LockRefreshFailed`]
/// when persistence or rollback fails.
pub fn sync_app_registries(options: &SyncOptions) -> Result<Vec<SyncUpdate>, SyncError> {
    let repo_files = discover_repo_files(&options.repo_root);
    ensure_repo_files_exist(&repo_files)?;

    let waxrc = load_waxrc(&repo_files.config_path)?;
    let mut lockfile = load_lockfile(&repo_files.lockfile_path)?;
    let config_path_display = repo_files.config_path.display().to_string();
    let original_config_json = read_config_json(&repo_files.config_path, &config_path_display)?;
    let mut config_json = original_config_json.clone();
    let mut config_changed = false;
    let mut prepared_updates = Vec::new();
    let mut resolved_state_path = options.state_path.clone();

    for entry in &waxrc.languages {
        if let Some(upstream) = entry
            .registry_source
            .as_ref()
            .and_then(|registry| registry.upstream())
            .filter(|upstream| !upstream.trim().is_empty())
        {
            let state_path = resolve_upstream_state_path(&mut resolved_state_path)?;
            prepared_updates.push(prepare_language_upstream_sync(
                state_path,
                entry,
                upstream,
                &mut config_json,
                &mut config_changed,
            )?);
        } else if entry.registry_source.as_ref().is_some_and(|registry| {
            matches!(
                registry,
                crate::config::waxrc::LanguageRegistrySource::Git { .. }
            )
        }) {
            prepared_updates.push(prepare_language_git_sync(options, entry, &lockfile)?);
        }
    }

    let updates: Vec<SyncUpdate> = prepared_updates
        .iter()
        .map(|prepared| prepared.update.clone())
        .collect();

    persist_sync_updates(&mut PersistSyncInput {
        options,
        repo_files: &repo_files,
        config_path_display: &config_path_display,
        config_json: &config_json,
        config_changed,
        original_config_json: &original_config_json,
        lockfile: &mut lockfile,
        prepared_updates: &prepared_updates,
    })?;

    Ok(updates)
}

/// Attempts sync for each configured upstream, applying successful updates.
///
/// # Errors
///
/// Returns [`SyncError::MissingConfig`] or [`SyncError::MissingLockfile`] when a
/// required repository input is absent; [`SyncError::Config`] or
/// [`SyncError::Lockfile`] when those inputs are invalid; or
/// [`SyncError::ConfigUpdate`] when the original config JSON cannot be read.
/// Per-upstream preparation and persistence failures are returned in the
/// successful result's failure list.
pub fn best_effort_sync_app_registries(options: &SyncOptions) -> BestEffortSyncResult {
    let repo_files = discover_repo_files(&options.repo_root);
    ensure_repo_files_exist(&repo_files)?;

    let waxrc = load_waxrc(&repo_files.config_path)?;
    let mut lockfile = load_lockfile(&repo_files.lockfile_path)?;
    let config_path_display = repo_files.config_path.display().to_string();
    let original_config_json = read_config_json(&repo_files.config_path, &config_path_display)?;
    let mut config_json = original_config_json.clone();
    let mut config_changed = false;
    let mut failures = Vec::new();
    let mut prepared_updates = Vec::new();
    let mut resolved_state_path = options.state_path.clone();

    for entry in &waxrc.languages {
        if let Some(upstream) = entry
            .registry_source
            .as_ref()
            .and_then(|registry| registry.upstream())
            .filter(|upstream| !upstream.trim().is_empty())
        {
            let prepared =
                resolve_upstream_state_path(&mut resolved_state_path).and_then(|state_path| {
                    prepare_language_upstream_sync(
                        state_path,
                        entry,
                        upstream,
                        &mut config_json,
                        &mut config_changed,
                    )
                });
            match prepared {
                Ok(prepared) => prepared_updates.push(prepared),
                Err(error) => failures.push((upstream.to_owned(), error)),
            }
        } else if entry.registry_source.as_ref().is_some_and(|registry| {
            matches!(
                registry,
                crate::config::waxrc::LanguageRegistrySource::Git { .. }
            )
        }) {
            match prepare_language_git_sync(options, entry, &lockfile) {
                Ok(prepared) => prepared_updates.push(prepared),
                Err(error) => failures.push((git_sync_label(entry), error)),
            }
        }
    }

    if prepared_updates.is_empty() {
        return Ok((Vec::new(), failures));
    }

    match persist_sync_updates(&mut PersistSyncInput {
        options,
        repo_files: &repo_files,
        config_path_display: &config_path_display,
        config_json: &config_json,
        config_changed,
        original_config_json: &original_config_json,
        lockfile: &mut lockfile,
        prepared_updates: &prepared_updates,
    }) {
        Ok(()) => Ok((
            prepared_updates
                .iter()
                .map(|prepared| prepared.update.clone())
                .collect(),
            failures,
        )),
        Err(error) => {
            let message = error.to_string();
            for prepared in prepared_updates {
                failures.push((
                    prepared.update.upstream.clone(),
                    SyncError::LockRefreshFailed {
                        upstream: prepared.update.upstream.clone(),
                        message: message.clone(),
                    },
                ));
            }
            Ok((Vec::new(), failures))
        }
    }
}

fn persist_sync_updates(input: &mut PersistSyncInput<'_>) -> Result<(), SyncError> {
    if input.prepared_updates.is_empty() {
        return Ok(());
    }

    let mut registry_backups = Vec::new();
    for prepared in input.prepared_updates {
        if let Some(copy) = &prepared.registry_copy {
            registry_backups.push(backup_registry_file(
                &input.options.repo_root,
                &copy.app_registry_relative,
            )?);
        }
    }

    if input.config_changed {
        write_config_json(
            &input.repo_files.config_path,
            input.config_path_display,
            input.config_json,
        )?;
    }

    for prepared in input.prepared_updates {
        if let Some(copy) = &prepared.registry_copy {
            copy_design_system_registry_to_app(
                &copy.remembered,
                &copy.design_system_local_source,
                &input.options.repo_root,
                &copy.app_registry_relative,
            )?;
        }
    }

    let waxrc = load_waxrc(&input.repo_files.config_path)?;
    let lockfile_changed = match refresh_registry_locks(
        input.lockfile,
        &input.options.repo_root,
        &waxrc,
        input.prepared_updates,
        input.options.upgrade,
    ) {
        Ok(changed) => changed,
        Err(error) => {
            restore_sync_rollback(
                &input.repo_files.config_path,
                input.config_path_display,
                input.original_config_json,
                input.config_changed,
                &registry_backups,
            )?;
            return Err(error);
        }
    };

    if lockfile_changed
        && let Err(error) = save_lockfile(&input.repo_files.lockfile_path, input.lockfile)
    {
        restore_sync_rollback(
            &input.repo_files.config_path,
            input.config_path_display,
            input.original_config_json,
            input.config_changed,
            &registry_backups,
        )?;
        return Err(error);
    }

    Ok(())
}

fn backup_registry_file(
    repo_root: &Path,
    app_registry_relative: &str,
) -> Result<RegistryFileBackup, SyncError> {
    let path = repo_root.join(app_registry_relative);
    let path_display = path.display().to_string();
    let previous = if path.is_file() {
        Some(fs::read(&path).map_err(|source| SyncError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(source),
        })?)
    } else {
        None
    };
    Ok(RegistryFileBackup { path, previous })
}

fn restore_registry_backup(backup: &RegistryFileBackup) -> Result<(), SyncError> {
    let path_display = backup.path.display().to_string();
    match &backup.previous {
        Some(contents) => write_atomically(&backup.path, contents, AtomicWriteOptions::default())
            .map_err(|source| SyncError::RegistryRestoreAtomicWrite {
                path: path_display,
                source,
            }),
        None if backup.path.exists() => {
            fs::remove_file(&backup.path).map_err(|source| SyncError::ConfigUpdate {
                path: path_display,
                source: Box::new(source),
            })
        }
        None => Ok(()),
    }
}

fn restore_sync_rollback(
    config_path: &Path,
    config_path_display: &str,
    original_config_json: &Value,
    config_changed: bool,
    registry_backups: &[RegistryFileBackup],
) -> Result<(), SyncError> {
    if config_changed {
        restore_config_json(config_path, config_path_display, original_config_json)?;
    }
    for backup in registry_backups {
        restore_registry_backup(backup)?;
    }
    Ok(())
}

fn restore_config_json(
    config_path: &Path,
    config_path_display: &str,
    config_json: &Value,
) -> Result<(), SyncError> {
    write_config_json(config_path, config_path_display, config_json)
}

fn ensure_repo_files_exist(
    repo_files: &crate::config::repo_files::RepoFileSet,
) -> Result<(), SyncError> {
    if !repo_files.config_path.is_file() {
        return Err(SyncError::MissingConfig {
            path: repo_files.config_path.clone(),
        });
    }
    if !repo_files.lockfile_path.is_file() {
        return Err(SyncError::MissingLockfile {
            path: repo_files.lockfile_path.clone(),
        });
    }
    Ok(())
}

fn prepare_language_upstream_sync(
    state_path: &Path,
    entry: &LanguageEntry,
    upstream: &str,
    config_json: &mut Value,
    config_changed: &mut bool,
) -> Result<PreparedSync, SyncError> {
    let design_system_id = parse_upstream_design_system_id(upstream, &entry.id)?;
    let remembered = show_remembered_design_system(state_path, design_system_id)?;
    let resolved = resolve_remembered_registry(&remembered, &entry.id)?;
    let registry_copy = resolved
        .design_system_local_source
        .as_ref()
        .map(|local_source| RegistryCopyPlan {
            remembered: remembered.clone(),
            design_system_local_source: local_source.clone(),
            app_registry_relative: resolved.config_source.clone(),
        });
    if update_config_registry_source(config_json, &entry.id, &resolved.config_source) {
        *config_changed = true;
    }
    Ok(PreparedSync {
        update: SyncUpdate {
            language_id: entry.id.clone(),
            upstream: resolved.upstream,
            source: resolved.config_source,
            git: None,
            tag: None,
            old_commit: None,
            new_commit: None,
        },
        registry_copy,
        prepared_lock: None,
    })
}

fn resolve_upstream_state_path(
    resolved_state_path: &mut Option<PathBuf>,
) -> Result<&Path, SyncError> {
    if resolved_state_path.is_none() {
        *resolved_state_path = Some(crate::paths::state_file()?);
    }
    Ok(resolved_state_path
        .as_deref()
        .expect("state path is initialized before borrowing"))
}

fn prepare_language_git_sync(
    options: &SyncOptions,
    entry: &LanguageEntry,
    lockfile: &WaxLock,
) -> Result<PreparedSync, SyncError> {
    let resolved = resolve_language_registry_source(
        &options.repo_root,
        entry.id.as_str(),
        entry.registry_source.as_ref(),
        lockfile.registries.get(&entry.id),
        options.upgrade,
    )?;
    let (git, tag) = match entry.registry_source.as_ref() {
        Some(crate::config::waxrc::LanguageRegistrySource::Git { git, tag }) => {
            (Some(git.clone()), Some(tag.clone()))
        }
        _ => (None, None),
    };
    Ok(PreparedSync {
        update: SyncUpdate {
            language_id: entry.id.clone(),
            upstream: git_sync_label(entry),
            source: resolved.source.clone(),
            git,
            tag,
            old_commit: lockfile
                .registries
                .get(&entry.id)
                .and_then(|lock| lock.commit.clone()),
            new_commit: resolved.git_commit.clone(),
        },
        registry_copy: None,
        prepared_lock: Some(PreparedLockFields {
            source: resolved.source,
            sha256: resolved.sha256,
            git_commit: resolved.git_commit,
        }),
    })
}

fn git_sync_label(entry: &LanguageEntry) -> String {
    match entry.registry_source.as_ref() {
        Some(crate::config::waxrc::LanguageRegistrySource::Git { git, tag }) => {
            format!("{}@{tag}", redact_git_remote(git))
        }
        _ => unreachable!("git sync labels require a Git registry source"),
    }
}

/// Strips URL userinfo so sync labels and warnings never echo embedded credentials.
fn redact_git_remote(git: &str) -> String {
    let Some(scheme_end) = git.find("://") else {
        return git.to_owned();
    };
    let after_scheme = scheme_end + 3;
    let Some(at_offset) = git[after_scheme..].find('@') else {
        return git.to_owned();
    };
    let host_start = after_scheme + at_offset + 1;
    if git[host_start..].is_empty() {
        return git.to_owned();
    }
    format!("{}{}", &git[..after_scheme], &git[host_start..])
}

fn parse_upstream_design_system_id<'a>(
    upstream: &'a str,
    language_id: &LanguageId,
) -> Result<&'a str, SyncError> {
    let (design_system_id, upstream_language) =
        upstream
            .split_once('/')
            .ok_or_else(|| SyncError::InvalidUpstream {
                upstream: upstream.to_owned(),
                language_id: language_id.clone(),
            })?;
    if design_system_id.is_empty()
        || upstream_language.is_empty()
        || upstream_language != language_id.as_str()
    {
        return Err(SyncError::InvalidUpstream {
            upstream: upstream.to_owned(),
            language_id: language_id.clone(),
        });
    }
    Ok(design_system_id)
}

fn read_config_json(path: &Path, path_display: &str) -> Result<Value, SyncError> {
    let contents = fs::read_to_string(path).map_err(|source| SyncError::ConfigUpdate {
        path: path_display.to_owned(),
        source: Box::new(source),
    })?;
    serde_json::from_str(&contents).map_err(|source| SyncError::ConfigUpdate {
        path: path_display.to_owned(),
        source: Box::new(source),
    })
}

fn write_config_json(path: &Path, path_display: &str, config: &Value) -> Result<(), SyncError> {
    let serialized =
        serde_json::to_string_pretty(config).map_err(|source| SyncError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        })?;
    write_atomically(
        path,
        format!("{serialized}\n").as_bytes(),
        AtomicWriteOptions::default(),
    )
    .map_err(|source| SyncError::ConfigAtomicWrite {
        path: path_display.to_owned(),
        source,
    })
}

fn update_config_registry_source(
    config: &mut Value,
    language_id: &LanguageId,
    source: &str,
) -> bool {
    let Some(language) = config
        .get_mut("languages")
        .and_then(Value::as_object_mut)
        .and_then(|languages| languages.get_mut(language_id.as_str()))
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let Some(registry) = language.get_mut("registry").and_then(Value::as_object_mut) else {
        return false;
    };
    let current = registry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if current == source {
        return false;
    }
    registry.insert("source".to_owned(), Value::String(source.to_owned()));
    true
}

fn refresh_registry_locks(
    lockfile: &mut WaxLock,
    repo_root: &Path,
    waxrc: &WaxRc,
    refreshed_languages: &[PreparedSync],
    upgrade: bool,
) -> Result<bool, SyncError> {
    let mut changed = false;
    for entry in waxrc.languages.iter().filter(|entry| {
        refreshed_languages
            .iter()
            .any(|prepared| prepared.update.language_id == entry.id)
    }) {
        let prepared = refreshed_languages
            .iter()
            .find(|prepared| prepared.update.language_id == entry.id);
        let (source, sha256, git_commit) = if let Some(prepared_lock) =
            prepared.and_then(|prepared| prepared.prepared_lock.as_ref())
        {
            (
                prepared_lock.source.clone(),
                prepared_lock.sha256.clone(),
                prepared_lock.git_commit.clone(),
            )
        } else {
            let is_git = entry.registry_source.as_ref().is_some_and(|source| {
                matches!(
                    source,
                    crate::config::waxrc::LanguageRegistrySource::Git { .. }
                )
            });
            let resolved = resolve_language_registry_source(
                repo_root,
                entry.id.as_str(),
                entry.registry_source.as_ref(),
                lockfile.registries.get(&entry.id),
                is_git && upgrade,
            )?;
            (resolved.source, resolved.sha256, resolved.git_commit)
        };
        let refreshed = LockedRegistry {
            source,
            sha256,
            git: entry
                .registry_source
                .as_ref()
                .and_then(|source| match source {
                    crate::config::waxrc::LanguageRegistrySource::Git { git, .. } => {
                        Some(git.clone())
                    }
                    _ => None,
                }),
            tag: entry
                .registry_source
                .as_ref()
                .and_then(|source| match source {
                    crate::config::waxrc::LanguageRegistrySource::Git { tag, .. } => {
                        Some(tag.clone())
                    }
                    _ => None,
                }),
            commit: git_commit,
        };
        if lockfile.registries.get(&entry.id) != Some(&refreshed) {
            lockfile.registries.insert(entry.id.clone(), refreshed);
            changed = true;
        }
    }
    if changed && lockfile.schema_version != WAX_LOCK_SCHEMA_VERSION {
        lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    }
    Ok(changed)
}

fn save_lockfile(path: &Path, lockfile: &WaxLock) -> Result<(), SyncError> {
    let mut lockfile = lockfile.clone();
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    let contents =
        serde_json::to_string_pretty(&lockfile).map_err(|source| SyncError::LockfileWrite {
            path: path.display().to_string(),
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;
    write_atomically(
        path,
        format!("{contents}\n").as_bytes(),
        AtomicWriteOptions::default(),
    )
    .map_err(|source| SyncError::LockfileAtomicWrite {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::registry_memory::remember_design_system;
    use std::process::Command;
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
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("wax-core-sync-{name}-{nonce}"));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_app_repo(app_repo: &Path, upstream: &str, source: &str) {
        fs::create_dir_all(app_repo.join(".wax/registries/acme")).expect("create registries dir");
        fs::write(
            app_repo.join(".wax/registries/acme/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
        )
        .expect("write app registry");
        fs::write(
            app_repo.join(".wax/wax.config.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "languages": {{
    "react": {{
      "roots": ["src"],
      "registry": {{
        "source": "{source}",
        "upstream": "{upstream}"
      }}
    }}
  }}
}}
"#
            ),
        )
        .expect("write app config");
        fs::write(
            app_repo.join(".wax/wax.lock.json"),
            r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {
    "react": {
      "source": ".wax/registries/acme/react.json",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  },
  "languages": {}
}
"#,
        )
        .expect("write app lockfile");
    }

    fn setup_remembered_local_ds(root: &Path) -> (PathBuf, PathBuf) {
        let ds_repo = root.join("acme-ds");
        fs::create_dir_all(ds_repo.join(".wax/registries")).expect("create ds registries dir");
        fs::write(
            ds_repo.join(".wax/registries/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
        )
        .expect("write ds registry");
        fs::write(
            ds_repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json"
        }
      }
    }
  }
}
"#,
        )
        .expect("write ds config");

        let wax_home = root.join("wax-home");
        fs::create_dir_all(&wax_home).expect("create wax home");
        let state_path = wax_home.join("state.json");
        remember_design_system(&state_path, "acme", "Acme Design System", &ds_repo)
            .expect("remember design system");
        (ds_repo, state_path)
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_git_registry(root: &Path) -> PathBuf {
        let git_repo = root.join("git-registry");
        fs::create_dir_all(git_repo.join(".wax/registries"))
            .expect("create git registry directory");
        run_git(
            root,
            &["init", git_repo.to_str().expect("git registry path")],
        );
        run_git(&git_repo, &["config", "user.email", "wax@example.invalid"]);
        run_git(&git_repo, &["config", "user.name", "Wax Test"]);
        fs::write(
            git_repo.join(".wax/registries/compose.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
        )
        .expect("write Git registry");
        run_git(&git_repo, &["add", "."]);
        run_git(&git_repo, &["commit", "-m", "initial registry"]);
        run_git(&git_repo, &["tag", "v1"]);
        git_repo
    }

    fn write_git_app_repo(app_repo: &Path, git: &Path) {
        fs::create_dir_all(app_repo.join(".wax")).expect("create app wax directory");
        fs::write(
            app_repo.join(".wax/wax.config.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "languages": {{
    "compose": {{"registry": {{"git": "{}", "tag": "v1"}}}}
  }}
}}
"#,
                git.display()
            ),
        )
        .expect("write git app config");
        fs::write(
            app_repo.join(".wax/wax.lock.json"),
            r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {},
  "languages": {}
}
"#,
        )
        .expect("write git app lockfile");
    }

    #[test]
    fn restore_registry_backup_preserves_exact_bytes() {
        let root = TestDir::new("restore-exact-bytes");
        let path = root.path.join("registry.json");
        let original = b"{\"schema_version\":1,\"components\":[]}\r\n";
        fs::write(&path, b"replacement").expect("write replacement");

        restore_registry_backup(&RegistryFileBackup {
            path: path.clone(),
            previous: Some(original.to_vec()),
        })
        .expect("restore backup");

        assert_eq!(fs::read(path).expect("read restored registry"), original);
    }

    #[test]
    fn restore_registry_backup_maps_atomic_failures_to_typed_error() {
        let root = TestDir::new("restore-error-mapping");
        let parent = root.path.join("not-a-directory");
        fs::write(&parent, b"file").expect("create parent file");

        let error = restore_registry_backup(&RegistryFileBackup {
            path: parent.join("registry.json"),
            previous: Some(b"registry".to_vec()),
        })
        .expect_err("restore should fail when parent is a file");

        assert!(matches!(
            error,
            SyncError::RegistryRestoreAtomicWrite { .. }
        ));
    }

    #[test]
    fn sync_copies_local_design_system_registry_changes_into_app_repo() {
        let _guard = env_lock();
        let root = TestDir::new("copy-local");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let (ds_repo, state_path) = setup_remembered_local_ds(&root.path);

        fs::write(
            ds_repo.join(".wax/registries/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"},{"name":"Card"}]}"#,
        )
        .expect("update ds registry");

        let updates = sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path: Some(state_path),
            upgrade: false,
        })
        .expect("sync app registries");

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].upstream, "acme/react");
        let copied = fs::read_to_string(app_repo.join(".wax/registries/acme/react.json"))
            .expect("read copied registry");
        assert!(copied.contains("Card"));
    }

    #[test]
    fn sync_switches_app_registry_source_to_published_source() {
        let _guard = env_lock();
        let root = TestDir::new("published-source");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let (ds_repo, state_path) = setup_remembered_local_ds(&root.path);
        let published_registry = ds_repo.join("published-react.registry.json");
        fs::write(
            &published_registry,
            r#"{"schema_version":1,"components":[{"name":"PublishedButton"}]}"#,
        )
        .expect("write published registry");
        let published_source = format!("file://{}", published_registry.display());
        fs::write(
            ds_repo.join(".wax/wax.config.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "design_systems": {{
    "acme": {{
      "name": "Acme Design System",
      "registries": {{
        "react": {{
          "source": ".wax/registries/react.json",
          "published_source": "{published_source}"
        }}
      }}
    }}
  }}
}}
"#
            ),
        )
        .expect("write ds config with published source");

        let updates = sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path: Some(state_path),
            upgrade: false,
        })
        .expect("sync app registries");

        assert_eq!(updates[0].source, published_source);
        let app_config =
            fs::read_to_string(app_repo.join(".wax/wax.config.json")).expect("read app config");
        assert!(app_config.contains(&published_source));
    }

    #[test]
    fn best_effort_sync_leaves_config_unchanged_when_lock_refresh_fails() {
        let _guard = env_lock();
        let root = TestDir::new("best-effort-lock-failure");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let (ds_repo, state_path) = setup_remembered_local_ds(&root.path);
        fs::write(
            ds_repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json",
          "published_source": "https://cdn.example.invalid/acme/react.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .expect("write ds config with unreachable published source");
        let original_config =
            fs::read_to_string(app_repo.join(".wax/wax.config.json")).expect("read config");

        let result = best_effort_sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path: Some(state_path),
            upgrade: false,
        })
        .expect("best-effort sync should not abort");

        assert!(result.0.is_empty());
        assert_eq!(result.1.len(), 1);
        assert_eq!(result.1[0].0, "acme/react");
        let config_after: Value = serde_json::from_str(
            &fs::read_to_string(app_repo.join(".wax/wax.config.json")).expect("read config"),
        )
        .expect("parse config");
        let original_value: Value = serde_json::from_str(&original_config).expect("parse config");
        assert_eq!(config_after, original_value);
    }

    #[test]
    fn best_effort_sync_restores_copied_registry_when_lock_refresh_fails() {
        use sha2::{Digest, Sha256};

        let _guard = env_lock();
        let root = TestDir::new("restore-copied-registry");
        let app_repo = root.path.join("app");
        let original_registry =
            b"{\"schema_version\":1,\"components\":[{\"name\":\"Button\"}]}\r\n";
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        fs::write(
            app_repo.join(".wax/registries/acme/react.json"),
            original_registry,
        )
        .expect("write exact original registry");
        let registry_sha256 = Sha256::digest(original_registry).iter().fold(
            String::with_capacity(64),
            |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            },
        );
        fs::write(
            app_repo.join(".wax/wax.lock.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {{
    "react": {{
      "source": ".wax/registries/acme/react.json",
      "sha256": "{registry_sha256}"
    }}
  }},
  "languages": {{}}
}}
"#
            ),
        )
        .expect("write lockfile with matching digest");

        let (ds_repo, state_path) = setup_remembered_local_ds(&root.path);
        fs::write(
            ds_repo.join(".wax/registries/react.json"),
            "{not valid registry json",
        )
        .expect("write malformed ds registry");

        let result = best_effort_sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path: Some(state_path),
            upgrade: false,
        })
        .expect("best-effort sync should not abort");

        assert!(result.0.is_empty());
        assert_eq!(result.1.len(), 1);
        let restored = fs::read(app_repo.join(".wax/registries/acme/react.json"))
            .expect("read restored registry");
        assert_eq!(restored, original_registry);
    }

    #[test]
    fn sync_fails_when_upstream_design_system_is_not_remembered() {
        let _guard = env_lock();
        let root = TestDir::new("missing-memory");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let wax_home = root.path.join("wax-home");
        fs::create_dir_all(&wax_home).expect("create wax home");
        let state_path = wax_home.join("state.json");
        fs::write(
            &state_path,
            r#"{"installed_languages":{},"design_systems":{}}"#,
        )
        .expect("write empty state");

        let error = sync_app_registries(&SyncOptions {
            repo_root: app_repo,
            state_path: Some(state_path),
            upgrade: false,
        })
        .expect_err("sync should fail for missing remembered design system");

        assert!(error.to_string().contains("acme"));
    }

    #[test]
    fn sync_git_registry_uses_no_global_state_and_skips_unchanged_lockfile_writes() {
        if !git_available() {
            eprintln!("skipping git-dependent sync test: system git was not found");
            return;
        }
        let root = TestDir::new("git-no-global-state");
        let git_registry = setup_git_registry(&root.path);
        let app_repo = root.path.join("app");
        write_git_app_repo(&app_repo, &git_registry);

        let updates = sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path: Some(root.path.join("missing-global-state.json")),
            upgrade: false,
        })
        .expect("git-only sync should not resolve global state");

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].language_id.as_str(), "compose");
        assert_eq!(
            updates[0].upstream,
            format!("{}@v1", git_registry.display())
        );
        let lockfile_path = app_repo.join(".wax/wax.lock.json");
        let first_lockfile = fs::read(&lockfile_path).expect("read first git lockfile");
        let lockfile = load_lockfile(&lockfile_path).expect("load git lockfile");
        let registry = lockfile.registries.get("compose").expect("git lock entry");
        assert_eq!(
            registry.git.as_deref(),
            Some(git_registry.to_str().unwrap())
        );
        assert_eq!(registry.tag.as_deref(), Some("v1"));
        assert!(registry.commit.is_some());

        sync_app_registries(&SyncOptions {
            repo_root: app_repo,
            state_path: Some(root.path.join("missing-global-state.json")),
            upgrade: false,
        })
        .expect("locked git sync should succeed");
        assert_eq!(
            fs::read(&lockfile_path).expect("read unchanged git lockfile"),
            first_lockfile,
            "unchanged Git pins must not rewrite the lockfile"
        );
    }

    #[test]
    fn redact_git_remote_strips_https_userinfo() {
        assert_eq!(
            redact_git_remote("https://user:ghp_secret@github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            redact_git_remote("https://github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            redact_git_remote("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
    }

    #[test]
    fn git_sync_label_redacts_credentials_in_failure_labels() {
        let entry = LanguageEntry {
            id: LanguageId::try_from("compose").expect("language id"),
            roots: Vec::new(),
            registry_source: Some(crate::config::waxrc::LanguageRegistrySource::Git {
                git: "https://x-access-token:secret@github.com/org/repo.git".to_owned(),
                tag: "v1".to_owned(),
            }),
            extra: serde_json::Map::new(),
        };
        assert_eq!(git_sync_label(&entry), "https://github.com/org/repo.git@v1");
        assert!(!git_sync_label(&entry).contains("secret"));
    }

    #[test]
    fn refresh_registry_locks_reuses_prepared_git_commit_without_resolving_again() {
        let language_id = LanguageId::try_from("compose").expect("language id");
        let mut lockfile = WaxLock {
            schema_version: WAX_LOCK_SCHEMA_VERSION,
            engine_api_version: 1,
            wax_version: "0.0.0-test".to_owned(),
            locked_at: None,
            registries: Default::default(),
            languages: Default::default(),
        };
        let waxrc = WaxRc {
            schema_version: 2,
            engine: crate::config::waxrc::EngineConfig::default(),
            adoption: crate::config::waxrc::AdoptionConfig::default(),
            token_inference: crate::config::waxrc::TokenInferenceConfig::default(),
            languages: vec![LanguageEntry {
                id: language_id.clone(),
                roots: Vec::new(),
                registry_source: Some(crate::config::waxrc::LanguageRegistrySource::Git {
                    git: "https://example.invalid/repo.git".to_owned(),
                    tag: "v1".to_owned(),
                }),
                extra: serde_json::Map::new(),
            }],
            design_systems: Default::default(),
        };
        let prepared = [PreparedSync {
            update: SyncUpdate {
                language_id: language_id.clone(),
                upstream: "https://example.invalid/repo.git@v1".to_owned(),
                source: "git:https://example.invalid/repo.git#v1".to_owned(),
                git: Some("https://example.invalid/repo.git".to_owned()),
                tag: Some("v1".to_owned()),
                old_commit: None,
                new_commit: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
            },
            registry_copy: None,
            prepared_lock: Some(PreparedLockFields {
                source: "git:https://example.invalid/repo.git#v1".to_owned(),
                sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_owned(),
                git_commit: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
            }),
        }];

        let changed = refresh_registry_locks(
            &mut lockfile,
            Path::new("/tmp/unused-repo-root"),
            &waxrc,
            &prepared,
            true,
        )
        .expect("prepared lock fields should skip remote resolution");

        assert!(changed);
        let registry = lockfile
            .registries
            .get(&language_id)
            .expect("lock entry written from prepared fields");
        assert_eq!(
            registry.commit.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            registry.sha256,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(registry.source, "git:https://example.invalid/repo.git#v1");
    }
}
