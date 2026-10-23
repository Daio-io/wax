//! Registry snapshot selection across app, publisher, remembered, and explicit sources.

use std::path::{Path, PathBuf};

use serde_json::Map;
use wax_contract::{LanguageId, RegistryLookupError, RegistryLookupErrorCode, RegistrySnapshotRef};

use crate::config::lockfile::{LockedRegistry, load_lockfile};
use crate::config::repo_files::discover_repo_files;
use crate::config::waxrc::{LanguageEntry, LanguageRegistrySource, load_waxrc};
use crate::global_state::load_global_state;
use crate::paths::state_file;
use crate::registry_cache::{
    RegistryCache, RegistryCacheError, RegistryReadPolicy, RegistrySnapshot, RegistrySourceSpec,
};

/// Inputs used to select immutable registry snapshots.
#[derive(Debug, Clone)]
pub struct RegistryLookupOptions {
    /// Repository or standalone directory establishing lookup context.
    pub repo_root: PathBuf,
    /// Remembered id, app/publisher id, or explicit path/URL.
    pub selector: Option<String>,
    /// Optional language filter.
    pub language: Option<LanguageId>,
    /// Optional Git tag or commit for an explicit Git selector.
    pub tag: Option<String>,
    /// Optional exact immutable snapshot requested by the caller.
    pub snapshot: Option<RegistrySnapshotRef>,
    /// Refuse source I/O and resolve only already cached bytes.
    pub offline: bool,
    /// Global state path override. Its parent also owns the registry cache.
    pub state_path: Option<PathBuf>,
}

/// Resolves registry snapshots using repository context and explicit selection precedence.
///
/// An explicit selector matching an app upstream id resolves the app's locked inputs.
/// Otherwise a selector is resolved as a remembered id before publisher defaults.
/// Without a selector, app inputs take precedence over publisher inputs. Explicit
/// path/URL selectors remain independent lookups. `state_path` overrides the default
/// global state file used for remembered ids.
///
/// # Errors
///
/// Returns a stable [`RegistryLookupError`] when context is missing, configuration is
/// invalid, or an exact locked snapshot cannot be reproduced.
pub fn resolve_lookup_snapshots(
    options: &RegistryLookupOptions,
) -> Result<Vec<RegistrySnapshot>, RegistryLookupError> {
    let repo_files = discover_repo_files(&options.repo_root);
    let waxrc = if repo_files.config_path.is_file() {
        Some(load_waxrc(&repo_files.config_path).map_err(invalid_registry)?)
    } else {
        None
    };
    if let Some(waxrc) = waxrc.as_ref()
        && repo_files.lockfile_path.is_file()
    {
        let lock = load_lockfile(&repo_files.lockfile_path).map_err(invalid_registry)?;
        let matching_app_entries: Vec<_> = waxrc
            .languages
            .iter()
            .filter(|entry| options.language.as_ref().is_none_or(|id| id == &entry.id))
            .filter(|entry| app_selector_matches(entry, options.selector.as_deref()))
            .filter_map(|entry| lock.registries.get(&entry.id).map(|locked| (entry, locked)))
            .collect();
        if !matching_app_entries.is_empty() {
            return resolve_app_entries(options, &matching_app_entries);
        }
    }

    if let Some(selector) = options.selector.as_deref()
        && let Some(snapshots) = resolve_remembered_selector(options, selector)?
    {
        return Ok(snapshots);
    }

    if let Some(waxrc) = waxrc.as_ref() {
        let publisher_id = options
            .selector
            .as_ref()
            .filter(|selector| waxrc.design_systems.contains_key(selector.as_str()))
            .cloned();
        if (options.selector.is_none() && !waxrc.design_systems.is_empty())
            || publisher_id.is_some()
        {
            return lookup_cache(options)?
                .resolve(
                    &RegistrySourceSpec::PublisherRepository {
                        root: options.repo_root.clone(),
                        system_id: publisher_id,
                        language: options.language.clone(),
                    },
                    &RegistryReadPolicy {
                        offline: options.offline,
                        expected_snapshot: options.snapshot.clone(),
                    },
                )
                .map_err(map_cache_error);
        }
    }

    let Some(selector) = options.selector.as_deref() else {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::InvalidArgument,
            "registry lookup outside app or publisher context requires a selector",
        ));
    };
    resolve_explicit_source(options, selector)
}

fn resolve_remembered_selector(
    options: &RegistryLookupOptions,
    selector: &str,
) -> Result<Option<Vec<RegistrySnapshot>>, RegistryLookupError> {
    let state_path = match &options.state_path {
        Some(path) => path.clone(),
        None => state_file().map_err(invalid_registry)?,
    };
    if !state_path.is_file() {
        return Ok(None);
    }
    let state = load_global_state(&state_path).map_err(invalid_registry)?;
    let Some(remembered) = state.design_systems.get(selector) else {
        return Ok(None);
    };
    let local_source;
    let source = if let Some(source) = remembered.source.as_ref() {
        source
    } else {
        local_source = RegistrySourceSpec::PublisherRepository {
            root: remembered.repo_root.clone(),
            system_id: Some(selector.to_owned()),
            language: options.language.clone(),
        };
        &local_source
    };
    let mut snapshots = lookup_cache(options)?
        .resolve(
            source,
            &RegistryReadPolicy {
                offline: options.offline,
                expected_snapshot: options.snapshot.clone(),
            },
        )
        .map_err(map_cache_error)?;
    if let Some(language) = &options.language {
        snapshots.retain(|snapshot| &snapshot.reference.language == language);
    }
    if snapshots.is_empty() {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::SourceNotFound,
            format!("remembered design system `{selector}` has no matching registry"),
        ));
    }
    Ok(Some(snapshots))
}

fn app_selector_matches(entry: &LanguageEntry, selector: Option<&str>) -> bool {
    match selector {
        None => true,
        Some(selector) => entry
            .registry_source
            .as_ref()
            .and_then(LanguageRegistrySource::upstream)
            .and_then(|upstream| upstream.split_once('/'))
            .is_some_and(|(system_id, _)| system_id == selector),
    }
}

fn resolve_app_entries(
    options: &RegistryLookupOptions,
    entries: &[(&LanguageEntry, &LockedRegistry)],
) -> Result<Vec<RegistrySnapshot>, RegistryLookupError> {
    let cache = lookup_cache(options)?;
    let mut snapshots = Vec::new();
    for (entry, locked) in entries {
        let source = app_source_spec(&options.repo_root, entry, locked)?;
        let expected = app_snapshot_ref(entry, locked, &source);
        reject_conflicting_request(options, &expected, entry)?;
        let mut resolved = cache
            .resolve(
                &source,
                &RegistryReadPolicy {
                    offline: options.offline,
                    expected_snapshot: Some(expected),
                },
            )
            .map_err(map_cache_error)?;
        snapshots.append(&mut resolved);
    }
    Ok(snapshots)
}

fn app_source_spec(
    repo_root: &Path,
    entry: &LanguageEntry,
    locked: &LockedRegistry,
) -> Result<RegistrySourceSpec, RegistryLookupError> {
    match entry.registry_source.as_ref() {
        Some(LanguageRegistrySource::Git { git, tag }) => Ok(RegistrySourceSpec::Git {
            url: git.clone(),
            tag: Some(tag.clone()),
            language: entry.id.clone(),
        }),
        Some(LanguageRegistrySource::PathOrUrl { source, .. }) => {
            let source = if source.contains("://") {
                source.clone()
            } else {
                repo_root.join(source).display().to_string()
            };
            RegistrySourceSpec::parse(&source, Some(entry.id.clone())).map_err(map_cache_error)
        }
        None => {
            let source = repo_root.join(&locked.source).display().to_string();
            RegistrySourceSpec::parse(&source, Some(entry.id.clone())).map_err(map_cache_error)
        }
    }
}

fn app_snapshot_ref(
    entry: &LanguageEntry,
    locked: &LockedRegistry,
    source: &RegistrySourceSpec,
) -> RegistrySnapshotRef {
    let system_id = entry
        .registry_source
        .as_ref()
        .and_then(LanguageRegistrySource::upstream)
        .and_then(|upstream| upstream.split_once('/'))
        .map(|(system_id, _)| system_id.to_owned());
    RegistrySnapshotRef {
        source: source.canonical_identity(),
        language: entry.id.clone(),
        digest: locked.sha256.clone(),
        system_id,
        git_commit: locked.commit.clone(),
        extra: Map::new(),
    }
}

fn reject_conflicting_request(
    options: &RegistryLookupOptions,
    expected: &RegistrySnapshotRef,
    entry: &LanguageEntry,
) -> Result<(), RegistryLookupError> {
    if let Some(snapshot) = &options.snapshot
        && snapshot != expected
    {
        return Err(RegistryLookupError::new(
            RegistryLookupErrorCode::SnapshotMismatch,
            format!(
                "requested snapshot conflicts with the app lock for {}",
                entry.id
            ),
        ));
    }
    if let Some(tag) = options.tag.as_deref() {
        let configured = entry
            .registry_source
            .as_ref()
            .and_then(LanguageRegistrySource::git_parts)
            .map(|(_, tag)| tag);
        if configured != Some(tag) {
            return Err(RegistryLookupError::new(
                RegistryLookupErrorCode::SnapshotMismatch,
                format!(
                    "requested tag `{tag}` conflicts with the app lock for {}",
                    entry.id
                ),
            ));
        }
    }
    Ok(())
}

fn resolve_explicit_source(
    options: &RegistryLookupOptions,
    selector: &str,
) -> Result<Vec<RegistrySnapshot>, RegistryLookupError> {
    let mut source =
        RegistrySourceSpec::parse(selector, options.language.clone()).map_err(map_cache_error)?;
    if let Some(tag) = &options.tag {
        source = source.with_tag(tag.clone()).map_err(map_cache_error)?;
    }
    lookup_cache(options)?
        .resolve(
            &source,
            &RegistryReadPolicy {
                offline: options.offline,
                expected_snapshot: options.snapshot.clone(),
            },
        )
        .map_err(map_cache_error)
}

fn lookup_cache(options: &RegistryLookupOptions) -> Result<RegistryCache, RegistryLookupError> {
    if let Some(state_path) = &options.state_path {
        let Some(home) = state_path.parent() else {
            return Err(RegistryLookupError::new(
                RegistryLookupErrorCode::InvalidArgument,
                "global state path must have a parent directory",
            ));
        };
        Ok(RegistryCache::open(home.join("cache/registries")))
    } else {
        RegistryCache::open_default().map_err(invalid_registry)
    }
}

fn invalid_registry(error: impl std::fmt::Display) -> RegistryLookupError {
    RegistryLookupError::new(RegistryLookupErrorCode::InvalidRegistry, error.to_string())
}

fn map_cache_error(error: RegistryCacheError) -> RegistryLookupError {
    let code = match error {
        RegistryCacheError::OfflineCacheMiss { .. } => RegistryLookupErrorCode::OfflineCacheMiss,
        RegistryCacheError::SnapshotUnavailable { .. } => {
            RegistryLookupErrorCode::LockedSnapshotUnavailable
        }
        RegistryCacheError::SnapshotMismatch { .. }
        | RegistryCacheError::DigestMismatch { .. }
        | RegistryCacheError::LanguageMismatch { .. } => RegistryLookupErrorCode::SnapshotMismatch,
        RegistryCacheError::ReadSource { .. }
        | RegistryCacheError::PublisherNotFound { .. }
        | RegistryCacheError::MissingPublishedRegistry { .. } => {
            RegistryLookupErrorCode::SourceNotFound
        }
        RegistryCacheError::InvalidSourceSpec { .. }
        | RegistryCacheError::MissingLanguage { .. }
        | RegistryCacheError::InvalidLanguage { .. }
        | RegistryCacheError::InvalidSourceUrl { .. } => RegistryLookupErrorCode::InvalidArgument,
        _ => RegistryLookupErrorCode::InvalidRegistry,
    };
    RegistryLookupError::new(code, error.to_string())
}
