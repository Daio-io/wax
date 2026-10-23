//! Registry discovery orchestration and safe writes.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use wax_contract::{
    Diagnostic, LanguageId, REGISTRY_SCHEMA_VERSION, RegistryComponent, RegistryDocument,
    RegistryError, registry_from_json,
};
use wax_lang_api::{DiscoverRegistryRequest, WIRE_API_VERSION};

use crate::atomic_file::write_atomically_no_clobber;
use crate::auto_install::{InstalledManifest, installed_manifest_matches_locked};
use crate::config::lockfile::{
    LockedRegistry, LockfileError, WAX_LOCK_SCHEMA_VERSION, WaxLock, load_lockfile,
};
use crate::config::repo_files::{default_registry_path_for_language, discover_repo_files};
use crate::config::waxrc::{LanguageEntry, WaxRc, WaxRcError, load_waxrc};
use crate::global_state::{GlobalStateError, load_global_state};
use crate::paths::{PathsError, state_file};
use crate::registry_attachments::{RegistryAttachmentError, materialize_registry_attachments};
use crate::registry_memory::{
    RegistryMemoryError, design_system_registry_relative_path,
    ensure_design_system_registry_source, remember_design_system,
};
use crate::registry_merge::{
    RegistryMergeChange, RegistryMergeDiagnostic, RegistryMergeError, merge_registry,
};
use crate::registry_source::is_external_registry_source;
use crate::subprocess_discover::{DiscoverError, SubprocessLanguageDiscoverer};
use crate::subprocess_lang::SubprocessLanguageManifest;
use crate::{AtomicWriteError, AtomicWriteOptions, write_atomically};

const DEFAULT_DISCOVER_TIMEOUT: Duration = Duration::from_secs(120);

/// Inputs for registry discovery orchestration.
#[derive(Debug, Clone)]
pub struct RegistryDiscoverOptions<'a> {
    /// Repository root where the registry should be written.
    pub repo_root: &'a Path,
    /// Language pack identifier to use for discovery.
    pub language_id: &'a str,
    /// Source roots inspected by language-specific discovery.
    pub roots: Vec<PathBuf>,
    /// When true, generate JSON but do not write a file.
    pub dry_run: bool,
    /// Compatibility flag; existing registries are always refreshed by lossless merge.
    pub force: bool,
    /// Design-system id to remember after discovery.
    pub design_system_id: Option<&'a str>,
    /// Display name for the remembered design system.
    pub design_system_name: Option<&'a str>,
}

/// Outputs returned after generating registry JSON.
#[derive(Debug, Clone)]
pub struct RegistryDiscoverResult {
    /// Resolved output path for the generated registry.
    pub output_path: PathBuf,
    /// Generated registry JSON value.
    pub registry: Value,
    /// Losslessly merged registry document.
    pub document: RegistryDocument,
    /// Structured entry-level changes made by discovery and merging.
    pub changes: Vec<RegistryMergeChange>,
    /// Structured identity diagnostics emitted by lossless merging.
    pub merge_diagnostics: Vec<RegistryMergeDiagnostic>,
    /// Whether discovery roots were resolved from wax config instead of CLI `--root`.
    pub used_config_roots: bool,
    /// Number of source roots scanned during discovery.
    pub root_count: usize,
    /// Whether `.wax/wax.config.json` was loaded.
    pub wax_config_present: bool,
    /// Whether `.wax/wax.lock.json` was loaded.
    pub lockfile_present: bool,
    /// Structured diagnostics returned by discovery and attachment materialization.
    pub diagnostics: Vec<Diagnostic>,
    /// Whether the design system was remembered in global state.
    pub remembered_design_system: bool,
}

/// Errors returned while discovering and optionally writing a registry file.
#[derive(Debug, Error)]
pub enum RegistryDiscoverError {
    /// No discovery roots were provided and wax config did not supply usable roots.
    #[error("no discovery roots configured; pass --root path/to/design-system")]
    MissingRoots,
    /// Wax config could not be loaded while resolving discovery roots.
    #[error(transparent)]
    Config(#[from] WaxRcError),
    /// Lockfile could not be loaded.
    #[error(transparent)]
    Lockfile(#[from] LockfileError),
    /// Global state could not be loaded.
    #[error(transparent)]
    GlobalState(#[from] GlobalStateError),
    /// Global path resolution failed.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Caller passed an invalid language identifier.
    #[error("invalid language id `{language_id}`")]
    InvalidLanguageId {
        /// Unparsed language id string.
        language_id: String,
    },
    /// The requested language is not enabled in wax config.
    #[error(
        "language `{language_id}` is not enabled in wax config at {config_path}; pass --root path/to/design-system"
    )]
    LanguageNotConfigured {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
    },
    /// The enabled language entry has no configured roots.
    #[error(
        "language `{language_id}` in {config_path} has no configured roots; pass --root path/to/design-system"
    )]
    NoConfiguredRoots {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
    },
    /// Configured roots are not a JSON array of repo-relative path strings.
    #[error(
        "language `{language_id}` roots in {config_path} must be a JSON array of repo-relative path strings"
    )]
    InvalidRootsShape {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
    },
    /// A configured root is not repo-relative.
    #[error("language `{language_id}` root `{root}` in {config_path} must be a repo-relative path")]
    InvalidRootPath {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
        /// Invalid configured root path.
        root: String,
    },
    /// A configured root resolves outside the repository.
    #[error(
        "language `{language_id}` root `{root}` in {config_path} must stay within the repository"
    )]
    RootEscapesRepo {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
        /// Configured root path that escaped the repository.
        root: String,
    },
    /// A configured root does not exist on disk.
    #[error("language `{language_id}` root `{root}` in {config_path} does not exist")]
    RootNotFound {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
        /// Missing configured root path.
        root: String,
    },
    /// A configured root path could not be canonicalized on disk.
    #[error("failed to resolve language `{language_id}` root `{root}` in {config_path}: {source}")]
    ResolveRoot {
        /// Language id requested by the caller.
        language_id: String,
        /// Config path used for resolution.
        config_path: String,
        /// Configured root path that failed to resolve.
        root: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Registry discovery requires an installed pack version from lockfile + global state.
    #[error(
        "registry discovery requires language pack `{language_id}` to be installed; run `wax language install {language_id}`"
    )]
    PackNotInstalled {
        /// Language requested by the caller.
        language_id: LanguageId,
    },
    /// Subprocess discover invocation failed.
    #[error(transparent)]
    DiscoverSubprocess(#[from] DiscoverError),
    /// The language pack reported that discover is unsupported.
    #[error("registry discovery is not supported for language `{language_id}`")]
    DiscoverUnsupported {
        /// Language id requested by the caller.
        language_id: LanguageId,
    },
    /// Registry source points to an external location that discover cannot write.
    #[error(
        "language `{language_id}` registry source `{registry_source}` is external; discover can only write repo-relative registry paths"
    )]
    RegistryExternalSource {
        /// Language id requested by the caller.
        language_id: LanguageId,
        /// Configured external source string.
        registry_source: String,
    },
    /// Generated registry JSON could not be serialized.
    #[error("failed to serialize discovered registry JSON: {source}")]
    Serialize {
        /// Underlying serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// The target registry appeared concurrently during first publication.
    #[error("registry appeared at {path} while publishing; rerun to merge the current file")]
    OutputExists {
        /// Existing output path.
        path: String,
    },
    /// Two discovered symbols generated the same stable registry id.
    #[error(
        "discovered symbols `{first_symbol}` and `{second_symbol}` both map to registry id `{id}`"
    )]
    IdCollision {
        /// Colliding stable registry id.
        id: String,
        /// First symbol seen for the id.
        first_symbol: String,
        /// Second symbol that collided with the same id.
        second_symbol: String,
    },
    /// Atomic registry-output replacement failed.
    #[error(transparent)]
    AtomicWrite(#[from] AtomicWriteError),
    /// Config could not be patched after writing discover output.
    #[error("failed to patch wax config at {path}: {source}")]
    ConfigPatch {
        /// Config path that failed to update.
        path: String,
        /// Underlying I/O or parse failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Config replacement failed after patching registry metadata.
    #[error("failed to patch wax config at {path}: {source}")]
    ConfigPatchAtomicWrite {
        /// Config path that failed to update.
        path: String,
        /// Atomic-write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// Lockfile could not be patched after writing discover output.
    #[error("failed to patch lockfile at {path}: {source}")]
    LockfilePatch {
        /// Lockfile path that failed to update.
        path: String,
        /// Underlying I/O or parse failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Lockfile replacement failed after patching registry metadata.
    #[error("failed to patch lockfile at {path}: {source}")]
    LockfilePatchAtomicWrite {
        /// Lockfile path that failed to update.
        path: String,
        /// Atomic-write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// Design-system remember/update failed after discovery.
    #[error(transparent)]
    RegistryMemory(#[from] RegistryMemoryError),
    /// `--design-system` and `--name` must be provided together.
    #[error("--design-system and --name must be provided together")]
    IncompleteDesignSystemOptions,
    /// An existing registry file could not be read.
    #[error("failed to read existing registry at {path}: {source}")]
    ReadExistingRegistry {
        /// Registry path that could not be read.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// An existing registry file violated the public registry contract.
    #[error("existing registry at {path} is invalid: {source}")]
    InvalidExistingRegistry {
        /// Registry path that contained invalid data.
        path: String,
        /// Contract parsing or validation failure.
        #[source]
        source: RegistryError,
    },
    /// Creator-associated attachment materialization failed.
    #[error(transparent)]
    Attachment(#[from] RegistryAttachmentError),
    /// Lossless registry merging failed.
    #[error(transparent)]
    Merge(#[from] RegistryMergeError),
}

/// Discovers a registry for a single language and optionally writes it to disk.
///
/// # Errors
///
/// Returns [`RegistryDiscoverError::IncompleteDesignSystemOptions`] for a
/// partial remember request; [`RegistryDiscoverError::Config`],
/// [`RegistryDiscoverError::Lockfile`], [`RegistryDiscoverError::GlobalState`],
/// or [`RegistryDiscoverError::Paths`] when required state cannot be loaded;
/// [`RegistryDiscoverError::InvalidLanguageId`],
/// [`RegistryDiscoverError::MissingRoots`],
/// [`RegistryDiscoverError::LanguageNotConfigured`],
/// [`RegistryDiscoverError::NoConfiguredRoots`],
/// [`RegistryDiscoverError::InvalidRootPath`],
/// [`RegistryDiscoverError::RootEscapesRepo`],
/// [`RegistryDiscoverError::RootNotFound`], or
/// [`RegistryDiscoverError::ResolveRoot`] for invalid discovery inputs;
/// [`RegistryDiscoverError::PackNotInstalled`],
/// [`RegistryDiscoverError::DiscoverSubprocess`],
/// [`RegistryDiscoverError::DiscoverUnsupported`], or
/// [`RegistryDiscoverError::RegistryExternalSource`] when discovery cannot run;
/// [`RegistryDiscoverError::ReadExistingRegistry`] or
/// [`RegistryDiscoverError::InvalidExistingRegistry`] when the publisher seed
/// cannot be loaded; [`RegistryDiscoverError::Attachment`] or
/// [`RegistryDiscoverError::Merge`] when enrichment cannot be safely merged;
/// [`RegistryDiscoverError::Serialize`],
/// [`RegistryDiscoverError::IdCollision`],
/// [`RegistryDiscoverError::OutputExists`],
/// [`RegistryDiscoverError::AtomicWrite`] when output cannot be generated or
/// published; and [`RegistryDiscoverError::ConfigPatch`],
/// [`RegistryDiscoverError::ConfigPatchAtomicWrite`],
/// [`RegistryDiscoverError::LockfilePatch`],
/// [`RegistryDiscoverError::LockfilePatchAtomicWrite`], or
/// [`RegistryDiscoverError::RegistryMemory`] when follow-up metadata cannot be
/// persisted.
pub fn discover_registry(
    options: RegistryDiscoverOptions<'_>,
) -> Result<RegistryDiscoverResult, RegistryDiscoverError> {
    let remembered_design_system = match (options.design_system_id, options.design_system_name) {
        (Some(id), Some(name)) => Some((id, name)),
        (None, None) => None,
        _ => return Err(RegistryDiscoverError::IncompleteDesignSystemOptions),
    };
    let remember_design_system_mode = remembered_design_system.is_some();
    let language_id = LanguageId::try_from(options.language_id).map_err(|_| {
        RegistryDiscoverError::InvalidLanguageId {
            language_id: options.language_id.to_owned(),
        }
    })?;
    let repo_files = discover_repo_files(options.repo_root);
    let config_path_display = repo_files.config_path.display().to_string();
    let waxrc = load_optional_waxrc(&repo_files.config_path)?;
    let wax_config_present = waxrc.is_some();
    let configured_entry = waxrc
        .as_ref()
        .and_then(|waxrc| find_enabled_language(waxrc, options.language_id));
    if waxrc.is_some() && configured_entry.is_none() && options.roots.is_empty() {
        return Err(RegistryDiscoverError::LanguageNotConfigured {
            language_id: options.language_id.to_owned(),
            config_path: config_path_display,
        });
    }
    let fallback_entry = LanguageEntry {
        id: language_id.clone(),
        roots: Vec::new(),
        root_groups: Default::default(),
        registry_source: None,
        extra: serde_json::Map::new(),
    };
    let language_entry = configured_entry.unwrap_or(&fallback_entry);

    let output_path = if remember_design_system_mode {
        let repo_relative = design_system_registry_relative_path(&language_id);
        validate_repo_relative_registry_path(&language_id, &repo_relative)?;
        options.repo_root.join(&repo_relative)
    } else {
        resolve_discover_output_path(options.repo_root, &language_id, language_entry)?
    };
    let path_display = output_path.display().to_string();
    let output_source = repo_relative_output_source(options.repo_root, &output_path);

    let (roots, used_config_roots) = resolve_discovery_roots(
        options.repo_root,
        options.language_id,
        &options.roots,
        waxrc.as_ref(),
        &config_path_display,
    )?;

    let lockfile = load_optional_lockfile(&repo_files.lockfile_path)?;
    let lockfile_present = lockfile.is_some();
    let state = load_global_state(state_file()?)?;
    let pack_command = resolve_discover_pack_command(&state, lockfile.as_ref(), &language_id)?;
    let existing = load_existing_registry(&output_path)?;
    // `force` remains a compatibility flag; publishing always uses the lossless merge path.
    let _ = options.force;
    let seed = existing.clone().unwrap_or_else(|| RegistryDocument {
        schema_version: REGISTRY_SCHEMA_VERSION,
        components: Vec::new(),
        tokens: Vec::new(),
        extra: serde_json::Map::new(),
    });
    let creator_seed = seed.clone();
    let mut discovered = discover_registry_document(
        options.repo_root,
        &language_id,
        &roots,
        pack_command,
        seed,
        DEFAULT_DISCOVER_TIMEOUT,
    )?;
    // Packs may emit provisional metadata; only creator-authored associations are
    // eligible for attachment materialization and publication.
    retain_creator_metadata(&creator_seed, &mut discovered.document);
    let mut diagnostics = discovered.diagnostics;
    diagnostics.extend(materialize_registry_attachments(
        options.repo_root,
        &mut discovered.document,
    )?);
    let merged = merge_registry(existing.as_ref(), &discovered.document, true)?;
    let document = merged.document;
    let changes = merged.changes;
    let merge_diagnostics = merged.diagnostics;
    let registry = serde_json::to_value(&document)
        .map_err(|source| RegistryDiscoverError::Serialize { source })?;

    if options.dry_run {
        return Ok(RegistryDiscoverResult {
            output_path,
            registry,
            document,
            changes,
            merge_diagnostics,
            used_config_roots,
            root_count: roots.len(),
            wax_config_present,
            lockfile_present,
            diagnostics,
            remembered_design_system: false,
        });
    }

    let contents = serde_json::to_string_pretty(&registry)
        .map_err(|source| RegistryDiscoverError::Serialize { source })?;
    let written_bytes = format!("{contents}\n");
    if existing.is_some() {
        write_atomically(
            &output_path,
            written_bytes.as_bytes(),
            AtomicWriteOptions::default(),
        )?;
    } else {
        write_atomically_no_clobber(
            &output_path,
            written_bytes.as_bytes(),
            AtomicWriteOptions::default(),
        )
        .map_err(|source| match source {
            AtomicWriteError::DestinationExists { .. } => RegistryDiscoverError::OutputExists {
                path: path_display.clone(),
            },
            source => RegistryDiscoverError::AtomicWrite(source),
        })?;
    }

    if configured_entry.is_some()
        && should_patch_config_registry(language_entry)
        && !remember_design_system_mode
    {
        patch_config_registry(&repo_files.config_path, &language_id, &output_source)?;
    }

    if let Some((design_system_id, design_system_name)) = remembered_design_system {
        ensure_design_system_registry_source(
            options.repo_root,
            design_system_id,
            design_system_name,
            &language_id,
            &output_source,
        )?;
        remember_design_system(
            state_file()?,
            design_system_id,
            design_system_name,
            options.repo_root,
        )?;
    }

    if let Some(lockfile) = lockfile.as_ref() {
        patch_lockfile_registry(
            &repo_files.lockfile_path,
            lockfile,
            language_id,
            output_source,
            crate::digest::sha256_hex(written_bytes.as_bytes()),
        )?;
    }

    Ok(RegistryDiscoverResult {
        output_path,
        registry,
        document,
        changes,
        merge_diagnostics,
        used_config_roots,
        root_count: roots.len(),
        wax_config_present,
        lockfile_present,
        diagnostics,
        remembered_design_system: remember_design_system_mode,
    })
}

fn resolve_discovery_roots(
    repo_root: &Path,
    language_id: &str,
    roots: &[PathBuf],
    waxrc: Option<&WaxRc>,
    config_path: &str,
) -> Result<(Vec<PathBuf>, bool), RegistryDiscoverError> {
    if !roots.is_empty() {
        return Ok((roots.to_vec(), false));
    }

    let waxrc = waxrc.ok_or(RegistryDiscoverError::MissingRoots)?;

    let language = waxrc
        .languages
        .iter()
        .find(|language| language.id.as_ref() == language_id)
        .ok_or_else(|| RegistryDiscoverError::LanguageNotConfigured {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
        })?;

    let configured_roots = if language.roots.is_empty() {
        language
            .root_groups
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
    } else {
        language.roots.clone()
    };

    if configured_roots.is_empty() {
        return Err(RegistryDiscoverError::NoConfiguredRoots {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
        });
    }

    let mut resolved = Vec::with_capacity(configured_roots.len());
    for root in &configured_roots {
        resolved.push(resolve_configured_root(
            repo_root,
            language_id,
            config_path,
            root,
        )?);
    }

    Ok((resolved, true))
}

fn resolve_configured_root(
    repo_root: &Path,
    language_id: &str,
    config_path: &str,
    root: &str,
) -> Result<PathBuf, RegistryDiscoverError> {
    let relative_path = Path::new(root);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(RegistryDiscoverError::InvalidRootPath {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
            root: root.to_owned(),
        });
    }

    let candidate = repo_root.join(relative_path);
    if !candidate.exists() {
        return Err(RegistryDiscoverError::RootNotFound {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
            root: root.to_owned(),
        });
    }

    let canonical_repo_root =
        fs::canonicalize(repo_root).map_err(|source| RegistryDiscoverError::ResolveRoot {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
            root: root.to_owned(),
            source,
        })?;
    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|source| RegistryDiscoverError::ResolveRoot {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
            root: root.to_owned(),
            source,
        })?;

    if !canonical_candidate.starts_with(&canonical_repo_root) {
        return Err(RegistryDiscoverError::RootEscapesRepo {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
            root: root.to_owned(),
        });
    }

    Ok(canonical_candidate)
}

fn load_optional_waxrc(path: &Path) -> Result<Option<WaxRc>, RegistryDiscoverError> {
    match load_waxrc(path) {
        Ok(waxrc) => Ok(Some(waxrc)),
        Err(WaxRcError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(err) => Err(RegistryDiscoverError::Config(err)),
    }
}

fn load_optional_lockfile(path: &Path) -> Result<Option<WaxLock>, RegistryDiscoverError> {
    match load_lockfile(path) {
        Ok(lockfile) => Ok(Some(lockfile)),
        Err(LockfileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(err) => Err(RegistryDiscoverError::Lockfile(err)),
    }
}

fn load_existing_registry(path: &Path) -> Result<Option<RegistryDocument>, RegistryDiscoverError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(RegistryDiscoverError::ReadExistingRegistry {
                path: path.display().to_string(),
                source,
            });
        }
    };
    registry_from_json(&raw).map(Some).map_err(|source| {
        RegistryDiscoverError::InvalidExistingRegistry {
            path: path.display().to_string(),
            source,
        }
    })
}

fn find_enabled_language<'a>(waxrc: &'a WaxRc, language_id: &str) -> Option<&'a LanguageEntry> {
    waxrc
        .languages
        .iter()
        .find(|entry| entry.id.as_ref() == language_id)
}

fn resolve_discover_output_path(
    repo_root: &Path,
    language_id: &LanguageId,
    entry: &LanguageEntry,
) -> Result<PathBuf, RegistryDiscoverError> {
    let repo_relative = match &entry.registry_source {
        Some(source) => {
            if let Some((git, tag)) = source.git_parts() {
                return Err(RegistryDiscoverError::RegistryExternalSource {
                    language_id: language_id.clone(),
                    registry_source: format!(
                        "git registry resolution is not wired yet: {git}@{tag}"
                    ),
                });
            }
            let Some((path_or_url, _)) = source.path_or_url_parts() else {
                return Err(RegistryDiscoverError::RegistryExternalSource {
                    language_id: language_id.clone(),
                    registry_source: "unsupported registry source".to_owned(),
                });
            };
            if is_external_registry_source(path_or_url) {
                return Err(RegistryDiscoverError::RegistryExternalSource {
                    language_id: language_id.clone(),
                    registry_source: path_or_url.to_owned(),
                });
            }
            path_or_url.to_owned()
        }
        None => default_registry_path_for_language(language_id),
    };
    validate_repo_relative_registry_path(language_id, &repo_relative)?;
    Ok(repo_root.join(&repo_relative))
}

fn should_patch_config_registry(entry: &LanguageEntry) -> bool {
    entry.registry_source.is_none()
}

fn validate_repo_relative_registry_path(
    language_id: &LanguageId,
    repo_relative: &str,
) -> Result<(), RegistryDiscoverError> {
    let path = Path::new(repo_relative);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(RegistryDiscoverError::RegistryExternalSource {
            language_id: language_id.clone(),
            registry_source: repo_relative.to_owned(),
        });
    }
    Ok(())
}

fn repo_relative_output_source(repo_root: &Path, output_path: &Path) -> String {
    output_path
        .strip_prefix(repo_root)
        .unwrap_or(output_path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[derive(Debug, serde::Deserialize)]
struct InstalledManifestFile {
    id: LanguageId,
    version: String,
    api_version: u32,
    command: Vec<String>,
    #[serde(default)]
    target: String,
    #[serde(default)]
    sha256: String,
}

fn resolve_discover_pack_command(
    state: &crate::global_state::GlobalState,
    lockfile: Option<&WaxLock>,
    language_id: &LanguageId,
) -> Result<Vec<String>, RegistryDiscoverError> {
    match lockfile {
        Some(lockfile) => resolve_locked_pack_command(state, lockfile, language_id),
        None => resolve_latest_global_pack_command(state, language_id),
    }
}

fn resolve_locked_pack_command(
    state: &crate::global_state::GlobalState,
    lockfile: &WaxLock,
    language_id: &LanguageId,
) -> Result<Vec<String>, RegistryDiscoverError> {
    let Some(locked) = lockfile.languages.get(language_id) else {
        return Err(RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        });
    };
    let Some(pack) = state
        .installed_languages
        .get(language_id)
        .and_then(|versions| versions.get(&locked.version))
    else {
        return Err(RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        });
    };
    let manifest_path = pack.install_dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|_| {
        RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        }
    })?;
    let manifest: InstalledManifestFile =
        serde_json::from_str(&raw).map_err(|_| RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        })?;
    if manifest.id != *language_id
        || manifest.version != locked.version
        || manifest.command.is_empty()
        || !installed_manifest_matches_locked(
            &InstalledManifest {
                version: manifest.version.clone(),
                api_version: manifest.api_version,
                target: manifest.target.clone(),
                sha256: manifest.sha256.clone(),
            },
            locked,
        )
    {
        return Err(RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        });
    }

    Ok(resolve_manifest_command(
        pack.install_dir.as_path(),
        manifest.command,
    ))
}

fn resolve_latest_global_pack_command(
    state: &crate::global_state::GlobalState,
    language_id: &LanguageId,
) -> Result<Vec<String>, RegistryDiscoverError> {
    let Some(versions) = state.installed_languages.get(language_id) else {
        return Err(RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        });
    };
    let Some((_, pack)) = versions
        .iter()
        .max_by(|(left, _), (right, _)| compare_discover_versions(left, right))
    else {
        return Err(RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        });
    };

    let manifest_path = pack.install_dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|_| {
        RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        }
    })?;
    let manifest: InstalledManifestFile =
        serde_json::from_str(&raw).map_err(|_| RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        })?;
    if manifest.id != *language_id || manifest.command.is_empty() {
        return Err(RegistryDiscoverError::PackNotInstalled {
            language_id: language_id.clone(),
        });
    }

    Ok(resolve_manifest_command(
        pack.install_dir.as_path(),
        manifest.command,
    ))
}

fn compare_discover_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (semver::Version::parse(left), semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn resolve_manifest_command(install_dir: &Path, mut command: Vec<String>) -> Vec<String> {
    if let Some(primary) = command.first_mut()
        && let Some(relative) = primary.strip_prefix("./")
    {
        *primary = install_dir.join(relative).display().to_string();
    }
    command
}

fn discover_registry_document(
    repo_root: &Path,
    language_id: &LanguageId,
    roots: &[PathBuf],
    pack_command: Vec<String>,
    registry: RegistryDocument,
    timeout: Duration,
) -> Result<wax_lang_api::DiscoverRegistryResult, RegistryDiscoverError> {
    let request = DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: language_id.clone(),
        repo_root: repo_root.display().to_string(),
        roots: request_roots(repo_root, roots),
        registry,
    };
    let discoverer = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: pack_command,
        timeout,
    });
    match discoverer.discover_registry(request) {
        Ok(result) => Ok(result),
        Err(DiscoverError::Unsupported { .. }) => Err(RegistryDiscoverError::DiscoverUnsupported {
            language_id: language_id.clone(),
        }),
        Err(DiscoverError::IdCollision {
            id,
            first_symbol,
            second_symbol,
        }) => Err(RegistryDiscoverError::IdCollision {
            id,
            first_symbol,
            second_symbol,
        }),
        Err(error) => Err(RegistryDiscoverError::DiscoverSubprocess(error)),
    }
}

fn repo_relative_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn retain_creator_metadata(seed: &RegistryDocument, discovered: &mut RegistryDocument) {
    for component in &mut discovered.components {
        component.metadata = seed
            .components
            .iter()
            .find(|seed_component| seed_component.id == component.id)
            .or_else(|| unique_component_identity_match(&seed.components, component))
            .and_then(|seed_component| seed_component.metadata.clone());
    }
    for token in &mut discovered.tokens {
        token.metadata = seed
            .tokens
            .iter()
            .find(|seed_token| seed_token.id == token.id)
            .or_else(|| unique_token_identity_match(&seed.tokens, &token.key))
            .and_then(|seed_token| seed_token.metadata.clone());
    }
}

fn unique_component_identity_match<'a>(
    components: &'a [RegistryComponent],
    discovered: &RegistryComponent,
) -> Option<&'a RegistryComponent> {
    let mut matches = components.iter().filter(|component| {
        component.symbol == discovered.symbol
            && match &discovered.package {
                Some(package) => component.package.as_ref() == Some(package),
                None => true,
            }
    });
    let candidate = matches.next()?;
    matches.next().is_none().then_some(candidate)
}

fn unique_token_identity_match<'a>(
    tokens: &'a [wax_contract::RegistryToken],
    key: &str,
) -> Option<&'a wax_contract::RegistryToken> {
    let mut matches = tokens.iter().filter(|token| token.key == key);
    let candidate = matches.next()?;
    matches.next().is_none().then_some(candidate)
}

fn request_roots(repo_root: &Path, roots: &[PathBuf]) -> Vec<String> {
    roots
        .iter()
        .map(|root| repo_relative_path(repo_root, root))
        .collect()
}

fn patch_config_registry(
    config_path: &Path,
    language_id: &LanguageId,
    output_source: &str,
) -> Result<(), RegistryDiscoverError> {
    let path_display = config_path.display().to_string();
    let raw =
        fs::read_to_string(config_path).map_err(|source| RegistryDiscoverError::ConfigPatch {
            path: path_display.clone(),
            source: Box::new(source),
        })?;
    let mut config: Value =
        serde_json::from_str(&raw).map_err(|source| RegistryDiscoverError::ConfigPatch {
            path: path_display.clone(),
            source: Box::new(source),
        })?;
    let Some(languages) = config.get_mut("languages").and_then(Value::as_object_mut) else {
        return Err(RegistryDiscoverError::ConfigPatch {
            path: path_display,
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "wax config missing languages object",
            )),
        });
    };
    let Some(entry) = languages.get_mut(language_id.as_str()) else {
        return Err(RegistryDiscoverError::ConfigPatch {
            path: path_display,
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "language entry missing from wax config",
            )),
        });
    };
    let Some(entry_object) = entry.as_object_mut() else {
        return Err(RegistryDiscoverError::ConfigPatch {
            path: path_display,
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "language entry is not an object",
            )),
        });
    };
    entry_object.insert(
        "registry".to_owned(),
        Value::String(output_source.to_owned()),
    );
    let serialized = serde_json::to_string_pretty(&config).map_err(|source| {
        RegistryDiscoverError::ConfigPatch {
            path: path_display.clone(),
            source: Box::new(source),
        }
    })?;
    write_atomically(
        config_path,
        format!("{serialized}\n").as_bytes(),
        AtomicWriteOptions::default(),
    )
    .map_err(|source| RegistryDiscoverError::ConfigPatchAtomicWrite {
        path: path_display,
        source,
    })?;
    Ok(())
}

fn patch_lockfile_registry(
    lockfile_path: &Path,
    existing_lockfile: &WaxLock,
    language_id: LanguageId,
    source: String,
    sha256: String,
) -> Result<(), RegistryDiscoverError> {
    let path_display = lockfile_path.display().to_string();
    let mut lockfile = existing_lockfile.clone();
    lockfile.registries.insert(
        language_id,
        LockedRegistry {
            source,
            sha256,
            git: None,
            tag: None,
            commit: None,
        },
    );
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    let serialized = serde_json::to_string_pretty(&lockfile).map_err(|source| {
        RegistryDiscoverError::LockfilePatch {
            path: path_display.clone(),
            source: Box::new(source),
        }
    })?;
    write_atomically(
        lockfile_path,
        format!("{serialized}\n").as_bytes(),
        AtomicWriteOptions::default(),
    )
    .map_err(|source| RegistryDiscoverError::LockfilePatchAtomicWrite {
        path: path_display,
        source,
    })?;
    Ok(())
}
