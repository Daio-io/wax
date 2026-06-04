//! Registry discovery orchestration and safe writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use wax_lang_compose::discover::{ComposeDiscoverError, discover_registry_symbols};

use crate::config::repo_files::{DEFAULT_REGISTRY_RELATIVE_PATH, discover_repo_files};
use crate::config::waxrc::{WaxRcError, load_waxrc};

const REGISTRY_SCHEMA_VERSION: u32 = 1;
const MAX_REGISTRY_TEMP_ATTEMPTS: u32 = 1000;

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
    /// The requested language does not support in-process registry discovery.
    #[error("registry discovery is not supported for language `{language_id}`")]
    UnsupportedLanguage {
        /// Language id requested by the caller.
        language_id: String,
    },
    /// The language-specific discovery pass failed.
    #[error("failed to discover registry symbols for language `{language_id}`: {source}")]
    Discover {
        /// Language id requested by the caller.
        language_id: String,
        /// Underlying discovery failure.
        #[source]
        source: ComposeDiscoverError,
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
    let output_path = options.repo_root.join(DEFAULT_REGISTRY_RELATIVE_PATH);
    let path_display = output_path.display().to_string();

    let (roots, used_config_roots) =
        resolve_discovery_roots(options.repo_root, options.language_id, &options.roots)?;

    let registry = build_registry(options.language_id, &roots)?;
    let registry = serde_json::to_value(&registry)
        .map_err(|source| RegistryDiscoverError::Serialize { source })?;

    if !options.dry_run {
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
    }

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
) -> Result<(Vec<PathBuf>, bool), RegistryDiscoverError> {
    if !roots.is_empty() {
        return Ok((roots.to_vec(), false));
    }

    let repo_files = discover_repo_files(repo_root);
    let config_path = repo_files.config_path.display().to_string();
    let waxrc = match load_waxrc(&repo_files.config_path) {
        Ok(waxrc) => waxrc,
        Err(WaxRcError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            return Err(RegistryDiscoverError::MissingRoots);
        }
        Err(err) => return Err(RegistryDiscoverError::Config(err)),
    };

    let language = waxrc
        .languages
        .iter()
        .find(|language| language.enabled && language.id.as_ref() == language_id)
        .ok_or_else(|| RegistryDiscoverError::LanguageNotConfigured {
            language_id: language_id.to_owned(),
            config_path: config_path.clone(),
        })?;

    let roots_value =
        language
            .extra
            .get("roots")
            .ok_or_else(|| RegistryDiscoverError::NoConfiguredRoots {
                language_id: language_id.to_owned(),
                config_path: config_path.clone(),
            })?;
    let roots_array =
        roots_value
            .as_array()
            .ok_or_else(|| RegistryDiscoverError::InvalidRootsShape {
                language_id: language_id.to_owned(),
                config_path: config_path.clone(),
            })?;
    if roots_array.is_empty() {
        return Err(RegistryDiscoverError::NoConfiguredRoots {
            language_id: language_id.to_owned(),
            config_path,
        });
    }

    let mut resolved = Vec::with_capacity(roots_array.len());
    for entry in roots_array {
        let root = entry
            .as_str()
            .ok_or_else(|| RegistryDiscoverError::InvalidRootsShape {
                language_id: language_id.to_owned(),
                config_path: config_path.clone(),
            })?;
        resolved.push(resolve_configured_root(
            repo_root,
            language_id,
            &config_path,
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

fn build_registry(
    language_id: &str,
    roots: &[PathBuf],
) -> Result<DiscoveredRegistry, RegistryDiscoverError> {
    let symbols = match language_id {
        "compose" => {
            discover_registry_symbols(roots).map_err(|source| RegistryDiscoverError::Discover {
                language_id: language_id.to_owned(),
                source,
            })?
        }
        _ => {
            return Err(RegistryDiscoverError::UnsupportedLanguage {
                language_id: language_id.to_owned(),
            });
        }
    };

    let mut seen_ids = BTreeMap::new();
    let mut components = Vec::new();
    for symbol in normalized_symbols(symbols) {
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
