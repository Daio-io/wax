//! Registry discovery orchestration and safe writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use wax_contract::LanguageId;
use wax_lang_api::{DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION};

use crate::config::lockfile::{
    LockedRegistry, LockfileError, WAX_LOCK_SCHEMA_VERSION, load_lockfile,
};
use crate::config::repo_files::{default_registry_path_for_language, discover_repo_files};
use crate::config::waxrc::{LanguageEntry, WaxRc, WaxRcError, load_waxrc};
use crate::global_state::{GlobalStateError, load_global_state};
use crate::paths::{PathsError, state_file};
use crate::subprocess_discover::{DiscoverError, SubprocessLanguageDiscoverer};
use crate::subprocess_lang::SubprocessLanguageManifest;

const REGISTRY_SCHEMA_VERSION: u32 = 1;
const MAX_REGISTRY_TEMP_ATTEMPTS: u32 = 1000;
const DEFAULT_DISCOVER_TIMEOUT: Duration = Duration::from_secs(120);

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
    /// When true, replace an existing output file.
    pub force: bool,
}

/// Outputs returned after generating registry JSON.
#[derive(Debug, Clone)]
pub struct RegistryDiscoverResult {
    /// Resolved output path for the generated registry.
    pub output_path: PathBuf,
    /// Generated registry JSON value.
    pub registry: Value,
    /// Whether discovery roots were resolved from wax config instead of CLI `--root`.
    pub used_config_roots: bool,
    /// Number of source roots scanned during discovery.
    pub root_count: usize,
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
    /// The target registry already exists and overwrite was not forced.
    #[error(
        "registry already exists at {path}; rerun with --force to replace it or --dry-run to inspect the generated registry first"
    )]
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
    /// The output directory could not be created.
    #[error("failed to create registry output directory for {path}: {source}")]
    CreateDir {
        /// Requested output path.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// A temporary file for atomic write could not be allocated.
    #[error("failed to create temporary registry file {temp_path} for {path}: {source}")]
    CreateTemp {
        /// Final output path.
        path: String,
        /// Temporary path that failed to allocate.
        temp_path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Registry contents could not be written to the temporary file.
    #[error("failed to write temporary registry file {temp_path} for {path}: {source}")]
    WriteTemp {
        /// Final output path.
        path: String,
        /// Temporary file path.
        temp_path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The temporary file could not be synced before rename.
    #[error("failed to sync temporary registry file {temp_path} for {path}: {source}")]
    SyncTemp {
        /// Final output path.
        path: String,
        /// Temporary file path.
        temp_path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The temporary file could not be moved into place.
    #[error("failed to replace registry file {path} with temporary file {temp_path}: {source}")]
    Rename {
        /// Final output path.
        path: String,
        /// Temporary file path.
        temp_path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The temporary file could not be linked into place without overwriting.
    #[error("failed to publish registry file {path} from temporary file {temp_path}: {source}")]
    PublishNoClobber {
        /// Final output path.
        path: String,
        /// Temporary file path.
        temp_path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Config could not be patched after writing discover output.
    #[error("failed to patch wax config at {path}: {source}")]
    ConfigPatch {
        /// Config path that failed to update.
        path: String,
        /// Underlying I/O or parse failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
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
}

#[derive(Debug, Serialize)]
struct DiscoveredRegistry {
    schema_version: u32,
    components: Vec<DiscoveredComponent>,
}

#[derive(Debug, Serialize)]
struct DiscoveredComponent {
    id: String,
    symbol: String,
}

/// Discovers a registry for a single language and optionally writes it to disk.
pub fn discover_registry(
    options: RegistryDiscoverOptions<'_>,
) -> Result<RegistryDiscoverResult, RegistryDiscoverError> {
    let language_id = LanguageId::try_from(options.language_id).map_err(|_| {
        RegistryDiscoverError::InvalidLanguageId {
            language_id: options.language_id.to_owned(),
        }
    })?;
    let repo_files = discover_repo_files(options.repo_root);
    let config_path_display = repo_files.config_path.display().to_string();
    let waxrc = load_optional_waxrc(&repo_files.config_path)?;
    let configured_entry = waxrc
        .as_ref()
        .and_then(|waxrc| find_enabled_language(waxrc, options.language_id));
    if waxrc.is_some() && configured_entry.is_none() && options.roots.is_empty() {
        return Err(RegistryDiscoverError::LanguageNotConfigured {
            language_id: options.language_id.to_owned(),
            config_path: config_path_display.clone(),
        });
    }
    let fallback_entry = LanguageEntry {
        id: language_id.clone(),
        enabled: true,
        extra: serde_json::Map::new(),
    };
    let language_entry = configured_entry.unwrap_or(&fallback_entry);

    let output_path =
        resolve_discover_output_path(options.repo_root, &language_id, language_entry)?;
    let path_display = output_path.display().to_string();
    let output_source = repo_relative_output_source(options.repo_root, &output_path);

    let (roots, used_config_roots) = resolve_discovery_roots(
        options.repo_root,
        options.language_id,
        &options.roots,
        waxrc.as_ref(),
        &config_path_display,
    )?;

    let lockfile = load_lockfile(&repo_files.lockfile_path)?;
    let state = load_global_state(state_file()?)?;
    let pack_command = resolve_installed_pack_command(&state, &lockfile, &language_id)?;
    let symbols = discover_symbols(
        options.repo_root,
        &language_id,
        &roots,
        pack_command,
        DEFAULT_DISCOVER_TIMEOUT,
    )?;
    let registry = build_registry(&symbols)?;
    let registry = serde_json::to_value(&registry)
        .map_err(|source| RegistryDiscoverError::Serialize { source })?;

    if options.dry_run {
        return Ok(RegistryDiscoverResult {
            output_path,
            registry,
            used_config_roots,
            root_count: roots.len(),
        });
    }

    if !options.force && output_path.try_exists().unwrap_or(false) {
        return Err(RegistryDiscoverError::OutputExists { path: path_display });
    }

    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| RegistryDiscoverError::CreateDir {
            path: output_path.display().to_string(),
            source,
        })?;
    }

    let contents = serde_json::to_string_pretty(&registry)
        .map_err(|source| RegistryDiscoverError::Serialize { source })?;
    write_registry_atomically(
        &output_path,
        &path_display,
        format!("{contents}\n").as_bytes(),
        options.force,
    )?;

    if configured_entry.is_some() && should_patch_config_registry(language_entry) {
        patch_config_registry(
            &repo_files.config_path,
            &language_id,
            &output_source,
            options.language_id,
        )?;
    }

    patch_lockfile_registry(
        &repo_files.lockfile_path,
        language_id.clone(),
        output_source,
        sha256_hex(contents.as_bytes()),
    )?;

    Ok(RegistryDiscoverResult {
        output_path,
        registry,
        used_config_roots,
        root_count: roots.len(),
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
        .find(|language| language.enabled && language.id.as_ref() == language_id)
        .ok_or_else(|| RegistryDiscoverError::LanguageNotConfigured {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
        })?;

    let roots_value =
        language
            .extra
            .get("roots")
            .ok_or_else(|| RegistryDiscoverError::NoConfiguredRoots {
                language_id: language_id.to_owned(),
                config_path: config_path.to_owned(),
            })?;
    let roots_array =
        roots_value
            .as_array()
            .ok_or_else(|| RegistryDiscoverError::InvalidRootsShape {
                language_id: language_id.to_owned(),
                config_path: config_path.to_owned(),
            })?;
    if roots_array.is_empty() {
        return Err(RegistryDiscoverError::NoConfiguredRoots {
            language_id: language_id.to_owned(),
            config_path: config_path.to_owned(),
        });
    }

    let mut resolved = Vec::with_capacity(roots_array.len());
    for entry in roots_array {
        let root = entry
            .as_str()
            .ok_or_else(|| RegistryDiscoverError::InvalidRootsShape {
                language_id: language_id.to_owned(),
                config_path: config_path.to_owned(),
            })?;
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

fn find_enabled_language<'a>(waxrc: &'a WaxRc, language_id: &str) -> Option<&'a LanguageEntry> {
    waxrc
        .languages
        .iter()
        .find(|entry| entry.enabled && entry.id.as_ref() == language_id)
}

fn resolve_discover_output_path(
    repo_root: &Path,
    language_id: &LanguageId,
    entry: &LanguageEntry,
) -> Result<PathBuf, RegistryDiscoverError> {
    let repo_relative = match entry.registry_source() {
        Some(source) if is_external_registry_source(&source.source) => {
            return Err(RegistryDiscoverError::RegistryExternalSource {
                language_id: language_id.clone(),
                registry_source: source.source,
            });
        }
        Some(source) => source.source,
        None => default_registry_path_for_language(language_id),
    };
    validate_repo_relative_registry_path(language_id, &repo_relative)?;
    Ok(repo_root.join(&repo_relative))
}

fn should_patch_config_registry(entry: &LanguageEntry) -> bool {
    entry.registry_source().is_none()
}

fn is_external_registry_source(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://") || source.starts_with("file://")
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
    command: Vec<String>,
}

fn resolve_installed_pack_command(
    state: &crate::global_state::GlobalState,
    lockfile: &crate::config::lockfile::WaxLock,
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
    let raw = fs::read_to_string(&manifest_path).map_err(|source| {
        RegistryDiscoverError::LockfilePatch {
            path: manifest_path.display().to_string(),
            source: Box::new(source),
        }
    })?;
    let manifest: InstalledManifestFile =
        serde_json::from_str(&raw).map_err(|source| RegistryDiscoverError::LockfilePatch {
            path: manifest_path.display().to_string(),
            source: Box::new(source),
        })?;
    if manifest.id != *language_id
        || manifest.version != locked.version
        || manifest.command.is_empty()
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

fn resolve_manifest_command(install_dir: &Path, mut command: Vec<String>) -> Vec<String> {
    if let Some(primary) = command.first_mut()
        && let Some(relative) = primary.strip_prefix("./")
    {
        *primary = install_dir.join(relative).display().to_string();
    }
    command
}

fn discover_symbols(
    repo_root: &Path,
    language_id: &LanguageId,
    roots: &[PathBuf],
    pack_command: Vec<String>,
    timeout: Duration,
) -> Result<Vec<String>, RegistryDiscoverError> {
    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: language_id.clone(),
        repo_root: repo_root.display().to_string(),
        roots: roots
            .iter()
            .map(|root| {
                root.strip_prefix(repo_root)
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| root.display().to_string())
            })
            .collect(),
    };
    let discoverer = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: pack_command,
        timeout,
    });
    match discoverer.discover(request) {
        Ok(result) => Ok(result.symbols),
        Err(DiscoverError::Unsupported { .. }) => Err(RegistryDiscoverError::DiscoverUnsupported {
            language_id: language_id.clone(),
        }),
        Err(err) => Err(RegistryDiscoverError::DiscoverSubprocess(err)),
    }
}

fn patch_config_registry(
    config_path: &Path,
    language_id: &LanguageId,
    output_source: &str,
    language_id_text: &str,
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
    let Some(languages) = config.get_mut("languages").and_then(Value::as_array_mut) else {
        return Err(RegistryDiscoverError::ConfigPatch {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "wax config missing languages array",
            )),
        });
    };
    let Some(entry) = languages.iter_mut().find(|entry| {
        entry.get("id").and_then(Value::as_str) == Some(language_id.as_str())
            || entry.get("id").and_then(Value::as_str) == Some(language_id_text)
    }) else {
        return Err(RegistryDiscoverError::ConfigPatch {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "language entry missing from wax config",
            )),
        });
    };
    let Some(entry_object) = entry.as_object_mut() else {
        return Err(RegistryDiscoverError::ConfigPatch {
            path: path_display.clone(),
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
    fs::write(config_path, format!("{serialized}\n")).map_err(|source| {
        RegistryDiscoverError::ConfigPatch {
            path: path_display,
            source: Box::new(source),
        }
    })?;
    Ok(())
}

fn patch_lockfile_registry(
    lockfile_path: &Path,
    language_id: LanguageId,
    source: String,
    sha256: String,
) -> Result<(), RegistryDiscoverError> {
    let path_display = lockfile_path.display().to_string();
    let mut lockfile = load_lockfile(lockfile_path)?;
    lockfile
        .registries
        .insert(language_id, LockedRegistry { source, sha256 });
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    let serialized = serde_json::to_string_pretty(&lockfile).map_err(|source| {
        RegistryDiscoverError::LockfilePatch {
            path: path_display.clone(),
            source: Box::new(source),
        }
    })?;
    fs::write(lockfile_path, format!("{serialized}\n")).map_err(|source| {
        RegistryDiscoverError::LockfilePatch {
            path: path_display,
            source: Box::new(source),
        }
    })?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn build_registry(symbols: &[String]) -> Result<DiscoveredRegistry, RegistryDiscoverError> {
    let mut seen_ids = BTreeMap::new();
    let mut components = Vec::new();
    for symbol in normalized_symbols(symbols.to_vec()) {
        let id = format!("ds.{}", kebab_case_symbol(&symbol));
        if let Some(first_symbol) = seen_ids.insert(id.clone(), symbol.clone()) {
            return Err(RegistryDiscoverError::IdCollision {
                id,
                first_symbol,
                second_symbol: symbol,
            });
        }
        components.push(DiscoveredComponent { id, symbol });
    }

    Ok(DiscoveredRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        components,
    })
}

fn normalized_symbols(symbols: Vec<String>) -> Vec<String> {
    symbols
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn kebab_case_symbol(symbol: &str) -> String {
    let mut result = String::with_capacity(symbol.len());
    let chars: Vec<char> = symbol.chars().collect();

    for (index, character) in chars.iter().copied().enumerate() {
        if character.is_ascii_uppercase() {
            let previous = index
                .checked_sub(1)
                .and_then(|previous| chars.get(previous));
            let next = chars.get(index + 1);
            let follows_lower_or_digit = previous
                .is_some_and(|previous| previous.is_ascii_lowercase() || previous.is_ascii_digit());
            let ends_acronym_before_word = previous
                .is_some_and(|previous| previous.is_ascii_uppercase())
                && next.is_some_and(|next| next.is_ascii_lowercase());

            if !result.is_empty() && (follows_lower_or_digit || ends_acronym_before_word) {
                result.push('-');
            }
            result.push(character.to_ascii_lowercase());
        } else if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
        } else {
            if !result.ends_with('-') && !result.is_empty() {
                result.push('-');
            }
        }
    }

    result.trim_matches('-').to_owned()
}

fn write_registry_atomically(
    path: &Path,
    path_display: &str,
    contents: &[u8],
    force: bool,
) -> Result<(), RegistryDiscoverError> {
    let (temp_path, mut temp_file) = create_temp_registry_file(path, path_display)?;
    let temp_display = temp_path.display().to_string();

    if let Err(source) = temp_file.write_all(contents) {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(RegistryDiscoverError::WriteTemp {
            path: path_display.to_owned(),
            temp_path: temp_display,
            source,
        });
    }

    if let Err(source) = temp_file.sync_all() {
        drop(temp_file);
        remove_temp_file(&temp_path);
        return Err(RegistryDiscoverError::SyncTemp {
            path: path_display.to_owned(),
            temp_path: temp_display,
            source,
        });
    }
    drop(temp_file);

    if force {
        replace_registry_file(&temp_path, &temp_display, path, path_display)
    } else {
        publish_registry_file_noclobber(&temp_path, &temp_display, path, path_display)
    }
}

fn create_temp_registry_file(
    path: &Path,
    path_display: &str,
) -> Result<(PathBuf, File), RegistryDiscoverError> {
    for attempt in 0..MAX_REGISTRY_TEMP_ATTEMPTS {
        let temp_path = sibling_temp_path(path, attempt);
        let temp_display = temp_path.display().to_string();

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(RegistryDiscoverError::CreateTemp {
                    path: path_display.to_owned(),
                    temp_path: temp_display,
                    source,
                });
            }
        }
    }

    let temp_path = sibling_temp_path(path, MAX_REGISTRY_TEMP_ATTEMPTS - 1);
    Err(RegistryDiscoverError::CreateTemp {
        path: path_display.to_owned(),
        temp_path: temp_path.display().to_string(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate unique temporary registry path",
        ),
    })
}

fn sibling_temp_path(path: &Path, attempt: u32) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "wax.registry.json".into());

    parent.join(format!("{file_name}.{attempt}.tmp"))
}

fn remove_temp_file(temp_path: &Path) {
    let _ = fs::remove_file(temp_path);
}

fn publish_registry_file_noclobber(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), RegistryDiscoverError> {
    match fs::hard_link(temp_path, path) {
        Ok(()) => {
            remove_temp_file(temp_path);
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            remove_temp_file(temp_path);
            Err(RegistryDiscoverError::OutputExists {
                path: path_display.to_owned(),
            })
        }
        Err(source) => {
            remove_temp_file(temp_path);
            Err(RegistryDiscoverError::PublishNoClobber {
                path: path_display.to_owned(),
                temp_path: temp_display.to_owned(),
                source,
            })
        }
    }
}

#[cfg(not(windows))]
fn replace_registry_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), RegistryDiscoverError> {
    fs::rename(temp_path, path).map_err(|source| {
        remove_temp_file(temp_path);
        RegistryDiscoverError::Rename {
            path: path_display.to_owned(),
            temp_path: temp_display.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn replace_registry_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), RegistryDiscoverError> {
    if !path.exists() {
        return rename_temp_registry_file(temp_path, temp_display, path, path_display);
    }

    replace_existing_registry_file(temp_path, temp_display, path, path_display)
}

#[cfg(windows)]
fn rename_temp_registry_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), RegistryDiscoverError> {
    fs::rename(temp_path, path).map_err(|source| {
        remove_temp_file(temp_path);
        RegistryDiscoverError::Rename {
            path: path_display.to_owned(),
            temp_path: temp_display.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn replace_existing_registry_file(
    temp_path: &Path,
    temp_display: &str,
    path: &Path,
    path_display: &str,
) -> Result<(), RegistryDiscoverError> {
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
        return Err(RegistryDiscoverError::Rename {
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

#[cfg(windows)]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
#[link(name = "Kernel32")]
unsafe extern "system" {
    #[link_name = "ReplaceFileW"]
    fn replace_file_w(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut core::ffi::c_void,
        reserved: *mut core::ffi::c_void,
    ) -> i32;
}
