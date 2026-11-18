//! Public catalogue discovery and exact-version bundle installation.

use crate::{
    AtomicWriteError, AtomicWriteOptions,
    advisory_lock::AdvisoryLock,
    digest::sha256_hex,
    registry_git::{RegistryGitError, fetch_git_files},
    write_atomically,
};
use crate::{
    config::{
        repo_files::discover_repo_files,
        waxrc::{WaxRcError, load_waxrc},
    },
    global_state::{GlobalStateError, load_global_state},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::PathBuf};
use thiserror::Error;
use wax_contract::{
    CatalogDiagnostic, CatalogFindResponse, CatalogFindResult, CatalogPublicStatus, LanguageId,
};

/// Built-in independent public catalogue repository.
pub const PUBLIC_REGISTRY_REPOSITORY: &str = "https://github.com/Daio-io/wax-registries.git";
/// Explicit catalogue source; callers can inject local Git repositories.
#[derive(Debug, Clone)]
pub struct CatalogSource {
    /// Git repository URL or local fixture path.
    pub repository: String,
    /// Branch refreshed by online discovery.
    pub branch: String,
    /// Repository-relative catalogue index path.
    pub index: String,
}
impl Default for CatalogSource {
    fn default() -> Self {
        Self {
            repository: PUBLIC_REGISTRY_REPOSITORY.into(),
            branch: "main".into(),
            index: "catalog/index.json".into(),
        }
    }
}
/// Discovery context and explicit storage/source overrides.
#[derive(Debug, Clone)]
pub struct CatalogOptions {
    /// Case-insensitive id/name substring, empty for all rows.
    pub query: String,
    /// Optional compatible language filter.
    pub language: Option<LanguageId>,
    /// Read only selected cached public metadata.
    pub offline: bool,
    /// Repository supplying configured systems.
    pub repo_root: PathBuf,
    /// Global installed/remembered state file.
    pub state_path: PathBuf,
    /// Immutable registry and catalogue cache root.
    pub cache_root: PathBuf,
    /// Public source, with no hidden endpoint overrides.
    pub source: CatalogSource,
}
impl Default for CatalogOptions {
    fn default() -> Self {
        Self::new(PathBuf::from("."), PathBuf::from("state.json"))
    }
}
impl CatalogOptions {
    /// Builds options using the state file's adjacent registry cache.
    #[must_use]
    pub fn new(repo_root: PathBuf, state_path: PathBuf) -> Self {
        let cache_root = state_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("cache/registries");
        Self {
            query: String::new(),
            language: None,
            offline: false,
            repo_root,
            state_path,
            cache_root,
            source: CatalogSource::default(),
        }
    }
}
/// Typed fatal local-state or catalogue operation failure.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// An exact digest did not belong to the requested public system/language.
    #[error("snapshot does not match the requested public system and language")]
    SnapshotMismatch,
    /// The requested public bundle revision is absent.
    #[error("public design system {id}@{revision} is not in the selected catalogue")]
    RevisionNotFound {
        /// Requested public id.
        id: String,
        /// Requested exact revision.
        revision: u64,
    },
    /// An installed slug is already occupied by a different source or artifact.
    #[error("design system {slug} conflicts with an existing selection; refusing overwrite")]
    Conflict {
        /// Occupied remembered slug.
        slug: String,
    },
    /// Bundle fetch or artifact validation failed.
    #[error(transparent)]
    Cache(#[from] crate::registry_cache::RegistryCacheError),
    /// Catalogue Git resolution failed.
    #[error(transparent)]
    Git(#[from] RegistryGitError),
    /// Invalid catalogue metadata.
    #[error(transparent)]
    Invalid(#[from] wax_contract::CatalogError),
    /// Catalogue/cache JSON is malformed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Catalogue cache read or lock failed.
    #[error("catalogue cache I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Atomic cache replacement failed.
    #[error(transparent)]
    Write(#[from] AtomicWriteError),
    /// No selected index is available offline.
    #[error("public catalogue is not available in the offline cache")]
    OfflineCacheMiss,
    /// Invalid local state.
    #[error(transparent)]
    State(#[from] GlobalStateError),
    /// Invalid local publisher/app configuration.
    #[error(transparent)]
    Config(#[from] WaxRcError),
}
/// Finds systems without changing installed selections or repository files.
///
/// # Errors
/// Invalid local configuration and global state are fatal; public failures are partial success.
pub fn find_design_systems(options: &CatalogOptions) -> Result<CatalogFindResponse, CatalogError> {
    let state = load_global_state(&options.state_path)?;
    let mut results = Vec::new();
    let path = discover_repo_files(&options.repo_root).config_path;
    if path.exists() {
        let config = load_waxrc(&path)?;
        for language in config.languages {
            let Some(source) = language.registry_source else {
                continue;
            };
            let id = match source {
                crate::config::waxrc::LanguageRegistrySource::PathOrUrl { source, upstream } => {
                    upstream
                        .and_then(|u| u.split_once('/').map(|(id, _)| id.to_owned()))
                        .unwrap_or(source)
                }
                crate::config::waxrc::LanguageRegistrySource::Git { git, system, .. } => {
                    system.unwrap_or_else(|| crate::registry_git::redact_git_remote(&git))
                }
            };
            if let Some(row) = results
                .iter_mut()
                .find(|r: &&mut CatalogFindResult| r.id == id && r.origin == "configured")
            {
                row.languages.push(language.id);
                row.languages.sort();
                row.languages.dedup();
            } else {
                results.push(local_row(id.clone(), id, "configured", vec![language.id]));
            }
        }
        for (id, system) in config.design_systems {
            results.push(local_row(
                id,
                system.name,
                "configured",
                system.registries.into_keys().collect(),
            ));
        }
    }
    for (id, system) in &state.design_systems {
        let mut languages: BTreeSet<_> = system
            .snapshots
            .iter()
            .map(|s| s.language.clone())
            .collect();
        if system.source.is_none() {
            let path = system.repo_root.join(&system.last_seen_config);
            let config = load_waxrc(&path)?;
            if let Some(publisher) = config.design_systems.get(id) {
                languages.extend(publisher.registries.keys().cloned());
            }
        }
        let mut row = local_row(
            id.clone(),
            system.name.clone(),
            "remembered",
            languages.into_iter().collect(),
        );
        if let Some(pin) = &system.bundle_pin
            && let Some(revision) = id
                .strip_prefix(&format!("public-{}-r", pin.system_id))
                .and_then(|revision| revision.parse::<u64>().ok())
                .filter(|r| *r > 0)
        {
            row.revision = Some(revision);
            row.catalogue_commit = Some(pin.commit.clone());
        }
        results.push(row);
    }
    let public_status = match selected_catalog(options, !options.offline) {
        Ok((selection, catalog)) => {
            for entry in catalog.entries {
                let installed = state
                    .design_systems
                    .get(&format!("public-{}-r{}", entry.id, entry.revision))
                    .and_then(|s| s.bundle_pin.as_ref())
                    .is_some_and(|p| bundle_matches(p, &options.source.repository, &entry));
                results.push(CatalogFindResult {
                    id: entry.id,
                    name: entry.name,
                    origin: "public".into(),
                    revision: Some(entry.revision),
                    languages: entry.registries.into_keys().collect(),
                    upstream: entry.upstream,
                    coverage: entry.coverage,
                    gaps: entry.gaps,
                    installed,
                    catalogue_commit: Some(selection.commit.clone()),
                });
            }
            CatalogPublicStatus {
                complete: true,
                cached: options.offline,
                commit: Some(selection.commit),
                diagnostic: None,
            }
        }
        Err(error) => CatalogPublicStatus {
            complete: false,
            cached: false,
            commit: None,
            diagnostic: Some(CatalogDiagnostic {
                code: if matches!(error, CatalogError::OfflineCacheMiss) {
                    "offline_cache_miss"
                } else {
                    "public_unavailable"
                }
                .into(),
                message: error.to_string(),
            }),
        },
    };
    let query = options.query.to_lowercase();
    results.retain(|r| {
        (r.id.to_lowercase().contains(&query) || r.name.to_lowercase().contains(&query))
            && options
                .language
                .as_ref()
                .is_none_or(|l| r.languages.contains(l))
    });
    results.sort_by(|a, b| (&a.origin, &a.id, a.revision).cmp(&(&b.origin, &b.id, b.revision)));
    Ok(CatalogFindResponse {
        schema_version: 1,
        results,
        public_status,
    })
}
fn local_row(
    id: String,
    name: String,
    origin: &str,
    languages: Vec<LanguageId>,
) -> CatalogFindResult {
    CatalogFindResult {
        id,
        name,
        origin: origin.into(),
        revision: None,
        languages,
        upstream: vec![],
        coverage: vec![],
        gaps: vec![],
        installed: true,
        catalogue_commit: None,
    }
}

#[derive(Serialize, Deserialize)]
struct CatalogSelection {
    commit: String,
    index_digest: String,
    fetched_at: u64,
}
fn catalog_dir(options: &CatalogOptions) -> PathBuf {
    let key = sha256_hex(
        serde_json::json!([
            options.source.repository,
            options.source.branch,
            options.source.index
        ])
        .to_string()
        .as_bytes(),
    );
    options.cache_root.join("catalogues").join(key)
}
fn validate_catalog(bytes: &[u8]) -> Result<wax_contract::Catalog, CatalogError> {
    let catalog: wax_contract::Catalog = serde_json::from_slice(bytes)?;
    catalog.validate()?;
    let mut engine = semver::Version::parse(wax_lang_api::build_version())
        .map_err(|e| wax_contract::CatalogError(e.to_string()))?;
    // Local snapshot builds share the base version's compatibility floor.
    if engine.pre.as_str() == "snapshot" {
        engine.pre = semver::Prerelease::EMPTY;
    }
    for entry in &catalog.entries {
        let minimum = semver::Version::parse(&entry.minimum_wax_version)
            .map_err(|e| wax_contract::CatalogError(e.to_string()))?;
        if minimum > engine {
            return Err(wax_contract::CatalogError(format!(
                "{} requires wax >= {minimum}",
                entry.id
            ))
            .into());
        }
    }
    Ok(catalog)
}
fn selected_catalog(
    options: &CatalogOptions,
    refresh: bool,
) -> Result<(CatalogSelection, wax_contract::Catalog), CatalogError> {
    let dir = catalog_dir(options);
    if !refresh {
        let bytes = match fs::read(dir.join("selected.json")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if options.offline {
                    return Err(CatalogError::OfflineCacheMiss);
                }
                return selected_catalog(options, true);
            }
            Err(e) => return Err(e.into()),
        };
        let selection: CatalogSelection = serde_json::from_slice(&bytes)?;
        if !wax_contract::valid_git_commit(&selection.commit)
            || !wax_contract::valid_sha256(&selection.index_digest)
        {
            return Err(
                wax_contract::CatalogError("invalid cached catalogue selection".into()).into(),
            );
        }
        let bytes = fs::read(dir.join(format!("{}.json", selection.index_digest)))?;
        if sha256_hex(&bytes) != selection.index_digest {
            return Err(
                wax_contract::CatalogError("cached catalogue digest mismatch".into()).into(),
            );
        }
        return Ok((selection, validate_catalog(&bytes)?));
    }
    fs::create_dir_all(&dir)?;
    let _lock = AdvisoryLock::acquire(&dir.join("selection.lock"))?;
    let fetched = fetch_git_files(
        &options.source.repository,
        &options.source.branch,
        &[&options.source.index],
        &options.cache_root.join("git"),
        false,
    )?;
    let bytes = fetched
        .files
        .get(&options.source.index)
        .ok_or_else(|| wax_contract::CatalogError("missing index".into()))?;
    let catalog = validate_catalog(bytes)?;
    let selection = CatalogSelection {
        commit: fetched.commit,
        index_digest: sha256_hex(bytes),
        fetched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    write_atomically(
        dir.join(format!("{}.json", selection.index_digest)),
        bytes,
        AtomicWriteOptions::default(),
    )?;
    write_atomically(
        dir.join("selected.json"),
        &serde_json::to_vec(&selection)?,
        AtomicWriteOptions::default(),
    )?;
    Ok((selection, catalog))
}

/// Installs every language of one exact public bundle revision under `public-<id>-r<revision>`.
///
/// # Errors
/// Returns typed errors for absent revisions, invalid artifacts, collisions, or persistence failure.
/// Validation completes before the locked state replacement; app files are never changed.
pub fn install_design_system(
    options: &CatalogOptions,
    id: &str,
    revision: u64,
) -> Result<wax_contract::RegistryBundlePin, CatalogError> {
    let (selection, catalog) = catalog_for_revision(options, id, revision)?;
    let entry = catalog
        .entries
        .iter()
        .find(|e| e.id == id && e.revision == revision)
        .ok_or_else(|| CatalogError::RevisionNotFound {
            id: id.into(),
            revision,
        })?;
    let slug = format!("public-{id}-r{revision}");
    let parent = options
        .state_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(parent)?;
    let _lock = AdvisoryLock::acquire(&options.state_path.with_extension("json.lock"))?;
    let mut state = load_global_state(&options.state_path)?;
    if let Some(existing) = state.design_systems.get(&slug) {
        if let Some(pin) = &existing.bundle_pin
            && bundle_matches(pin, &options.source.repository, entry)
            && matches!(&existing.source, Some(crate::registry_cache::RegistrySourceSpec::GitSystem {tag, ..}) if tag == &pin.commit)
        {
            return Ok(pin.clone());
        }
        return Err(CatalogError::Conflict { slug });
    }
    let cache = crate::registry_cache::RegistryCache::open(options.cache_root.clone());
    let snapshots = cache.resolve_catalog_entry(
        &options.source.repository,
        &selection.commit,
        entry,
        &crate::registry_cache::RegistryReadPolicy {
            offline: options.offline,
            expected_snapshot: None,
        },
    )?;
    let first = snapshots
        .first()
        .ok_or_else(|| wax_contract::CatalogError("empty bundle".into()))?;
    let definition = serde_json::from_value(
        first
            .reference
            .extra
            .get("definition")
            .cloned()
            .unwrap_or_default(),
    )?;
    let pin = wax_contract::RegistryBundlePin {
        repo: options.source.repository.clone(),
        commit: selection.commit.clone(),
        subdir: entry.subdir.clone(),
        system_id: id.into(),
        definition,
        provenance: Some(entry.provenance.clone()),
        registries: snapshots
            .iter()
            .map(|s| (s.reference.language.clone(), s.reference.clone()))
            .collect(),
    };
    pin.validate()?;
    state.design_systems.insert(
        slug,
        crate::global_state::RememberedDesignSystem {
            name: entry.name.clone(),
            repo_root: PathBuf::new(),
            last_seen_config: PathBuf::new(),
            source: Some(crate::registry_cache::RegistrySourceSpec::GitSystem {
                url: options.source.repository.clone(),
                tag: selection.commit,
                subdir: entry.subdir.clone(),
                system_id: id.into(),
            }),
            snapshots: snapshots.into_iter().map(|s| s.reference).collect(),
            bundle_pin: Some(pin.clone()),
        },
    );
    crate::global_state::save_global_state(&options.state_path, &state)?;
    Ok(pin)
}
fn bundle_matches(
    pin: &wax_contract::RegistryBundlePin,
    repository: &str,
    entry: &wax_contract::CatalogEntry,
) -> bool {
    pin.repo == repository
        && pin.subdir == entry.subdir
        && pin.system_id == entry.id
        && pin.definition.path == format!("{}/{}", entry.subdir, entry.definition.path)
        && pin.definition.sha256 == entry.definition.sha256
        && pin.provenance.as_ref() == Some(&entry.provenance)
        && pin.registries.len() == entry.registries.len()
        && entry.registries.iter().all(|(language, artifact)| {
            artifact.path == crate::registry_git::conventional_git_registry_path(language)
                && pin
                    .registries
                    .get(language)
                    .is_some_and(|s| s.digest == artifact.sha256)
        })
}

/// Resolves an uninstalled public bundle through the same artifact validation as installation.
/// An exact digest is read from immutable history, independently of current catalogue selection.
///
/// # Errors
/// Rejects missing revisions, wrong system/language/digests, or invalid/unavailable artifacts.
pub fn resolve_catalog_snapshots(
    options: &CatalogOptions,
    id: &str,
    revision: u64,
    language: Option<&LanguageId>,
    digest: Option<&str>,
) -> Result<Vec<crate::registry_cache::RegistrySnapshot>, CatalogError> {
    resolve_catalog_snapshots_with_reference(options, id, revision, language, digest, None)
}

pub(crate) fn resolve_catalog_snapshots_with_reference(
    options: &CatalogOptions,
    id: &str,
    revision: u64,
    language: Option<&LanguageId>,
    digest: Option<&str>,
    reference: Option<&wax_contract::RegistrySnapshotRef>,
) -> Result<Vec<crate::registry_cache::RegistrySnapshot>, CatalogError> {
    if LanguageId::try_from(id).is_err() || revision == 0 {
        return Err(wax_contract::CatalogError(
            "public selector requires <id>@<positive-revision>".into(),
        )
        .into());
    }
    let dir = catalog_dir(options).join("bundles");
    let path = dir.join(format!("{id}-r{revision}.json"));
    let cache = crate::registry_cache::RegistryCache::open(options.cache_root.clone());
    if let Some(digest) = digest {
        if !wax_contract::valid_sha256(digest) {
            return Err(CatalogError::SnapshotMismatch);
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CatalogError::SnapshotMismatch);
            }
            Err(e) => return Err(e.into()),
        };
        let history: Vec<wax_contract::RegistrySnapshotRef> = serde_json::from_slice(&bytes)?;
        let mut snapshots = Vec::new();
        let mut seen_languages = BTreeSet::new();
        for expected in history.iter().filter(|s| {
            s.digest == digest
                && s.system_id.as_deref() == Some(id)
                && language.is_none_or(|l| l == &s.language)
                && reference.is_none_or(|r| r == *s)
        }) {
            if !seen_languages.insert(expected.language.clone()) {
                continue;
            }
            let subdir = expected
                .extra
                .get("subdir")
                .and_then(serde_json::Value::as_str)
                .ok_or(CatalogError::SnapshotMismatch)?;
            let commit = expected
                .git_commit
                .as_ref()
                .ok_or(CatalogError::SnapshotMismatch)?;
            snapshots.extend(cache.resolve(
                &crate::registry_cache::RegistrySourceSpec::GitSystem {
                    url: options.source.repository.clone(),
                    tag: commit.clone(),
                    subdir: subdir.into(),
                    system_id: id.into(),
                },
                &crate::registry_cache::RegistryReadPolicy {
                    offline: options.offline,
                    expected_snapshot: Some(expected.clone()),
                },
            )?);
        }
        snapshots.dedup_by(|a, b| a.reference == b.reference);
        if snapshots.is_empty() {
            return Err(CatalogError::SnapshotMismatch);
        }
        return Ok(snapshots);
    }
    let (selection, catalog) = catalog_for_revision(options, id, revision)?;
    let entry = catalog
        .entries
        .iter()
        .find(|e| e.id == id && e.revision == revision)
        .ok_or_else(|| CatalogError::RevisionNotFound {
            id: id.into(),
            revision,
        })?;
    let mut snapshots = cache.resolve_catalog_entry(
        &options.source.repository,
        &selection.commit,
        entry,
        &crate::registry_cache::RegistryReadPolicy {
            offline: options.offline,
            expected_snapshot: None,
        },
    )?;
    fs::create_dir_all(&dir)?;
    let _lock = AdvisoryLock::acquire(&path.with_extension("lock"))?;
    let mut history: Vec<wax_contract::RegistrySnapshotRef> = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.into()),
    };
    for snapshot in &snapshots {
        if !history.contains(&snapshot.reference) {
            history.push(snapshot.reference.clone());
        }
    }
    write_atomically(
        &path,
        &serde_json::to_vec(&history)?,
        AtomicWriteOptions::default(),
    )?;
    snapshots.retain(|s| language.is_none_or(|l| l == &s.reference.language));
    if snapshots.is_empty() {
        return Err(CatalogError::SnapshotMismatch);
    }
    Ok(snapshots)
}

fn catalog_for_revision(
    options: &CatalogOptions,
    id: &str,
    revision: u64,
) -> Result<(CatalogSelection, wax_contract::Catalog), CatalogError> {
    let selected = selected_catalog(options, false)?;
    if !options.offline
        && !selected
            .1
            .entries
            .iter()
            .any(|e| e.id == id && e.revision == revision)
    {
        return selected_catalog(options, true);
    }
    Ok(selected)
}
