//! Design-system registry source resolution.

use crate::config::lockfile::LockedRegistry;
use crate::config::repo_files::{
    REGISTRY_CACHE_RELATIVE_DIR, default_registry_path_for_language_id,
};
use crate::config::waxrc::LanguageRegistrySource;
use crate::registry_git::{RegistryGitError, fetch_git_registry, fetch_git_registry_at_commit};
use crate::{AtomicWriteError, AtomicWriteOptions, write_atomically};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const HTTP_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Rejects repo-relative registry source strings that are absolute or escape via `..`.
///
/// # Errors
///
/// Returns [`RegistrySourceError::PlainAbsolutePath`] for an absolute path or
/// [`RegistrySourceError::PathEscapesRepo`] for root, prefix, or parent segments.
pub fn reject_repo_relative_registry_path_escape(source: &str) -> Result<(), RegistrySourceError> {
    let path = Path::new(source);
    if path.is_absolute() {
        return Err(RegistrySourceError::PlainAbsolutePath {
            input: source.to_owned(),
        });
    }

    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(RegistrySourceError::PathEscapesRepo {
            input: source.to_owned(),
        });
    }

    Ok(())
}

/// Validates that a repo-relative registry source resolves within `repo_root`.
///
/// # Errors
///
/// Returns [`RegistrySourceError::PlainAbsolutePath`] or
/// [`RegistrySourceError::PathEscapesRepo`] when the source is unsafe, or
/// [`RegistrySourceError::Read`] when existing path components cannot be
/// canonicalized.
pub fn validate_repo_relative_registry_path_within_repo(
    repo_root: &Path,
    source: &str,
) -> Result<PathBuf, RegistrySourceError> {
    reject_repo_relative_registry_path_escape(source)?;
    resolve_repo_relative_path(repo_root, Path::new(source), source)
}

/// Returns true when a registry source is remote or uses an explicit URL scheme.
pub fn is_external_registry_source(source: &str) -> bool {
    if source.contains("://") {
        return true;
    }

    let lower = source.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
}

/// Inputs for resolving one language registry source.
#[derive(Debug, Clone, Copy)]
pub struct RegistrySourceInput<'a> {
    /// Repository root used for repo-relative sources and cache materialization.
    pub repo_root: &'a Path,
    /// Language id string used in cache filenames.
    pub language_id: &'a str,
    /// Optional raw source string from config.
    pub source: Option<&'a str>,
}

/// Resolves a configured language registry source, honoring lock pins unless upgrading.
///
/// # Errors
///
/// Returns a [`RegistrySourceError`] when the source cannot be fetched, validated,
/// materialized, or does not match a non-upgrade lock pin.
pub fn resolve_language_registry_source(
    repo_root: &Path,
    language_id: &str,
    registry: Option<&LanguageRegistrySource>,
    locked: Option<&LockedRegistry>,
    upgrade: bool,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    let source = match registry {
        Some(LanguageRegistrySource::PathOrUrl { source, .. }) => Some(source.as_str()),
        Some(LanguageRegistrySource::Git { .. }) => None,
        None => None,
    };
    resolve_registry_source_with_lock(
        RegistrySourceInput {
            repo_root,
            language_id,
            source,
        },
        registry,
        locked,
        upgrade,
    )
}

/// Rejects git registry configuration while git resolution is not yet available.
///
/// # Errors
///
/// Returns [`RegistrySourceError::GitRegistryResolutionNotWired`] for git mode.
pub fn ensure_language_registry_source_supported(
    registry: Option<&LanguageRegistrySource>,
) -> Result<(), RegistrySourceError> {
    if let Some(LanguageRegistrySource::Git { git, tag }) = registry {
        return Err(RegistrySourceError::GitRegistryResolutionNotWired {
            git: git.clone(),
            tag: tag.clone(),
        });
    }
    Ok(())
}

/// Resolved registry source ready for downstream config rewriting and locking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRegistrySource {
    /// Original source string, defaulted when config omitted it.
    pub source: String,
    /// Repo-relative path to the materialized registry JSON.
    pub repo_relative_path: String,
    /// Lowercase hexadecimal SHA-256 digest of the registry bytes.
    pub sha256: String,
    /// Full Git commit object id when resolved from Git.
    pub git_commit: Option<String>,
}

/// Typed failures while resolving registry sources.
#[derive(Debug, Error)]
pub enum RegistrySourceError {
    /// A git registry was configured before git registry resolution was implemented.
    #[error("git registry resolution is not wired yet for {git}@{tag}")]
    GitRegistryResolutionNotWired {
        /// Git repository configured in waxrc.
        git: String,
        /// Git tag or commit configured in waxrc.
        tag: String,
    },
    /// Unsupported source URL scheme.
    #[error(
        "unsupported registry source scheme in {input}; use repo-relative path, file://, http://, or https://"
    )]
    UnsupportedScheme {
        /// Source string.
        input: String,
    },
    /// Plain absolute paths are not allowed.
    #[error("registry source {input} is an absolute path; use file:// for outside-repo files")]
    PlainAbsolutePath {
        /// Source string.
        input: String,
    },
    /// Repo-relative path attempted to escape the repository root.
    #[error("registry source {input} must not escape the repository root")]
    PathEscapesRepo {
        /// Source string.
        input: String,
    },
    /// File URL could not be parsed.
    #[error("invalid file:// registry source {input}: {reason}")]
    InvalidFileUrl {
        /// Source string.
        input: String,
        /// Human-readable parse failure reason.
        reason: &'static str,
    },
    /// Registry bytes could not be read from disk.
    #[error("failed to read registry source {input}: {io}")]
    Read {
        /// Source string.
        input: String,
        /// Underlying I/O error.
        #[source]
        io: std::io::Error,
    },
    /// Registry bytes could not be fetched over HTTP.
    #[error("failed to fetch registry source {input}: {http}")]
    Fetch {
        /// Source string.
        input: String,
        /// Underlying HTTP error.
        #[source]
        http: reqwest::Error,
    },
    /// Registry HTTP source returned a non-success status.
    #[error("failed to fetch registry source {input}: HTTP {status}")]
    HttpStatus {
        /// Source string.
        input: String,
        /// HTTP status code.
        status: reqwest::StatusCode,
    },
    /// Registry JSON is syntactically malformed.
    #[error("malformed registry JSON from {input}: {json}")]
    MalformedJson {
        /// Source string.
        input: String,
        /// Underlying JSON error.
        #[source]
        json: serde_json::Error,
    },
    /// Registry JSON shape does not satisfy the contract.
    #[error("invalid registry JSON from {input}: {reason}")]
    InvalidShape {
        /// Source string.
        input: String,
        /// Human-readable shape error.
        reason: &'static str,
    },
    /// External registry materialization failed.
    #[error("failed to materialize registry source {input} to {path}: {io}")]
    CacheWrite {
        /// Source string.
        input: String,
        /// Target cache path.
        path: String,
        /// Underlying I/O error.
        #[source]
        io: std::io::Error,
    },
    /// External registry cache could not be atomically replaced.
    #[error("failed to atomically materialize registry source {input} to {path}: {source}")]
    CacheAtomicWrite {
        /// Source string.
        input: String,
        /// Target cache path.
        path: String,
        /// Underlying atomic-write failure.
        #[source]
        source: Box<AtomicWriteError>,
    },
    /// Git resolution failed.
    #[error("failed to resolve Git registry: {0}")]
    Git(#[from] RegistryGitError),
    /// A Git lock has incomplete metadata.
    #[error("invalid partial Git lock for {language_id}")]
    InvalidGitLock {
        /// Language whose lock is malformed.
        language_id: String,
    },
    /// A locked Git registry's bytes or commit changed unexpectedly.
    #[error(transparent)]
    LockedGitMismatch(Box<LockedGitMismatch>),
}

/// Details for a Git registry lock pin mismatch.
#[derive(Debug, Error)]
#[error(
    "locked Git registry mismatch for {language_id}: expected commit {expected_commit} and digest {expected_digest}, got commit {actual_commit} and digest {actual_digest}"
)]
pub struct LockedGitMismatch {
    /// Language whose pinned content changed.
    pub language_id: String,
    /// Commit recorded in the lock.
    pub expected_commit: String,
    /// Digest recorded in the lock.
    pub expected_digest: String,
    /// Commit read from Git.
    pub actual_commit: String,
    /// Digest read from Git.
    pub actual_digest: String,
}

/// Resolves a registry source and returns the local repo-relative materialized path.
///
/// # Errors
///
/// Returns [`RegistrySourceError::UnsupportedScheme`],
/// [`RegistrySourceError::PlainAbsolutePath`],
/// [`RegistrySourceError::PathEscapesRepo`], or
/// [`RegistrySourceError::InvalidFileUrl`] for invalid sources;
/// [`RegistrySourceError::Read`], [`RegistrySourceError::Fetch`], or
/// [`RegistrySourceError::HttpStatus`] for I/O and HTTP failures;
/// [`RegistrySourceError::MalformedJson`] or [`RegistrySourceError::InvalidShape`]
/// for invalid registry JSON; and [`RegistrySourceError::CacheWrite`] or
/// [`RegistrySourceError::CacheAtomicWrite`] when an external registry cannot
/// be materialized locally.
pub fn resolve_registry_source(
    input: RegistrySourceInput<'_>,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    resolve_registry_source_with_options(input, None, None, false, false)
}

/// Resolves a registry source for validate, allowing a missing `components` key to warn later.
pub(crate) fn resolve_registry_source_allowing_missing_components(
    input: RegistrySourceInput<'_>,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    resolve_registry_source_with_options(input, None, None, false, true)
}

fn resolve_registry_source_with_lock(
    input: RegistrySourceInput<'_>,
    registry: Option<&LanguageRegistrySource>,
    locked: Option<&LockedRegistry>,
    upgrade: bool,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    resolve_registry_source_with_options(input, registry, locked, upgrade, false)
}

fn resolve_registry_source_with_options(
    input: RegistrySourceInput<'_>,
    registry: Option<&LanguageRegistrySource>,
    locked: Option<&LockedRegistry>,
    upgrade: bool,
    allow_missing_components: bool,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    if let Some(LanguageRegistrySource::Git { git, tag }) = registry {
        let canonical_source = format!("git:{}#{}", git.trim(), tag.trim());
        let fetch = if !upgrade {
            match locked {
                Some(lock) if lock.git.is_some() || lock.tag.is_some() || lock.commit.is_some() => {
                    let (Some(locked_git), Some(locked_tag), Some(locked_commit)) =
                        (&lock.git, &lock.tag, &lock.commit)
                    else {
                        return Err(RegistrySourceError::InvalidGitLock {
                            language_id: input.language_id.to_owned(),
                        });
                    };
                    if locked_git == git.trim()
                        && locked_tag == tag.trim()
                        && lock.source == canonical_source
                    {
                        fetch_git_registry_at_commit(
                            git,
                            locked_commit,
                            &wax_contract::LanguageId::try_from(input.language_id).map_err(
                                |_| RegistrySourceError::InvalidGitLock {
                                    language_id: input.language_id.to_owned(),
                                },
                            )?,
                            &input
                                .repo_root
                                .join(crate::config::repo_files::REGISTRY_GIT_CACHE_RELATIVE_DIR),
                        )?
                    } else {
                        fetch_git_registry(
                            git,
                            tag,
                            &wax_contract::LanguageId::try_from(input.language_id).map_err(
                                |_| RegistrySourceError::InvalidGitLock {
                                    language_id: input.language_id.to_owned(),
                                },
                            )?,
                            &input
                                .repo_root
                                .join(crate::config::repo_files::REGISTRY_GIT_CACHE_RELATIVE_DIR),
                        )?
                    }
                }
                _ => fetch_git_registry(
                    git,
                    tag,
                    &wax_contract::LanguageId::try_from(input.language_id).map_err(|_| {
                        RegistrySourceError::InvalidGitLock {
                            language_id: input.language_id.to_owned(),
                        }
                    })?,
                    &input
                        .repo_root
                        .join(crate::config::repo_files::REGISTRY_GIT_CACHE_RELATIVE_DIR),
                )?,
            }
        } else {
            fetch_git_registry(
                git,
                tag,
                &wax_contract::LanguageId::try_from(input.language_id).map_err(|_| {
                    RegistrySourceError::InvalidGitLock {
                        language_id: input.language_id.to_owned(),
                    }
                })?,
                &input
                    .repo_root
                    .join(crate::config::repo_files::REGISTRY_GIT_CACHE_RELATIVE_DIR),
            )?
        };
        validate_registry_json(&canonical_source, &fetch.bytes, allow_missing_components)?;
        let sha256 = crate::digest::sha256_hex(&fetch.bytes);
        if let Some(lock) = locked.filter(|lock| lock.source == canonical_source && !upgrade)
            && (lock.commit.as_deref() != Some(fetch.commit.as_str()) || lock.sha256 != sha256)
        {
            return Err(RegistrySourceError::LockedGitMismatch(Box::new(
                LockedGitMismatch {
                    language_id: input.language_id.to_owned(),
                    expected_commit: lock.commit.clone().unwrap_or_default(),
                    expected_digest: lock.sha256.clone(),
                    actual_commit: fetch.commit,
                    actual_digest: sha256,
                },
            )));
        }
        let path = materialize_external_registry(
            input.repo_root,
            input.language_id,
            &canonical_source,
            &sha256,
            &fetch.bytes,
        )?;
        return Ok(ResolvedRegistrySource {
            source: canonical_source,
            repo_relative_path: path,
            sha256,
            git_commit: Some(fetch.commit),
        });
    }
    let source = input
        .source
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| default_registry_path_for_language_id(input.language_id));
    let (bytes, repo_relative_path, external) = read_source(input.repo_root, &source)?;
    validate_registry_json(&source, &bytes, allow_missing_components)?;
    let sha256 = crate::digest::sha256_hex(&bytes);

    let repo_relative_path = if external {
        materialize_external_registry(input.repo_root, input.language_id, &source, &sha256, &bytes)?
    } else {
        repo_relative_path
    };

    Ok(ResolvedRegistrySource {
        source,
        repo_relative_path,
        sha256,
        git_commit: None,
    })
}

fn read_source(
    repo_root: &Path,
    source: &str,
) -> Result<(Vec<u8>, String, bool), RegistrySourceError> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_FETCH_TIMEOUT)
            .build()
            .map_err(|http| RegistrySourceError::Fetch {
                input: source.to_owned(),
                http,
            })?;
        let response = client
            .get(source)
            .send()
            .map_err(|http| RegistrySourceError::Fetch {
                input: source.to_owned(),
                http,
            })?;
        if !response.status().is_success() {
            return Err(RegistrySourceError::HttpStatus {
                input: source.to_owned(),
                status: response.status(),
            });
        }

        return response
            .bytes()
            .map(|bytes| (bytes.to_vec(), String::new(), true))
            .map_err(|http| RegistrySourceError::Fetch {
                input: source.to_owned(),
                http,
            });
    }

    if source.starts_with("file://") {
        let path = file_url_to_path(source)?;
        let bytes = fs::read(path).map_err(|io| RegistrySourceError::Read {
            input: source.to_owned(),
            io,
        })?;
        return Ok((bytes, String::new(), true));
    }

    if source.contains("://") {
        return Err(RegistrySourceError::UnsupportedScheme {
            input: source.to_owned(),
        });
    }

    let path = Path::new(source);
    if path.is_absolute() {
        return Err(RegistrySourceError::PlainAbsolutePath {
            input: source.to_owned(),
        });
    }

    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(RegistrySourceError::PathEscapesRepo {
            input: source.to_owned(),
        });
    }

    let resolved_path = resolve_repo_relative_path(repo_root, path, source)?;
    let bytes = fs::read(&resolved_path).map_err(|io| RegistrySourceError::Read {
        input: source.to_owned(),
        io,
    })?;
    Ok((bytes, source.to_owned(), false))
}

fn validate_registry_json(
    source: &str,
    bytes: &[u8],
    allow_missing_components: bool,
) -> Result<(), RegistrySourceError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|json| RegistrySourceError::MalformedJson {
            input: source.to_owned(),
            json,
        })?;

    let Some(object) = value.as_object() else {
        return Err(RegistrySourceError::InvalidShape {
            input: source.to_owned(),
            reason: "expected top-level object",
        });
    };

    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(RegistrySourceError::InvalidShape {
            input: source.to_owned(),
            reason: "missing or unsupported schema_version",
        });
    }

    match object.get("components") {
        Some(Value::Array(_)) => Ok(()),
        Some(_) => Err(RegistrySourceError::InvalidShape {
            input: source.to_owned(),
            reason: "components must be an array",
        }),
        None if allow_missing_components => Ok(()),
        None => Err(RegistrySourceError::InvalidShape {
            input: source.to_owned(),
            reason: "missing components array",
        }),
    }
}

fn materialize_external_registry(
    repo_root: &Path,
    language_id: &str,
    source: &str,
    sha256: &str,
    bytes: &[u8],
) -> Result<String, RegistrySourceError> {
    validate_cache_language_id(language_id)?;
    let repo_relative_path = format!("{REGISTRY_CACHE_RELATIVE_DIR}/{language_id}-{sha256}.json");
    let path = repo_root.join(&repo_relative_path);
    let parent = path
        .parent()
        .ok_or_else(|| RegistrySourceError::PathEscapesRepo {
            input: source.to_owned(),
        })?;

    ensure_cache_directory_within_repo(repo_root, parent, source)?;
    reject_symlink_path(&path, source)?;

    write_atomically(&path, bytes, AtomicWriteOptions::default()).map_err(|atomic_error| {
        RegistrySourceError::CacheAtomicWrite {
            input: source.to_owned(),
            path: path.display().to_string(),
            source: Box::new(atomic_error),
        }
    })?;

    Ok(repo_relative_path)
}

fn resolve_repo_relative_path(
    repo_root: &Path,
    relative_path: &Path,
    source: &str,
) -> Result<PathBuf, RegistrySourceError> {
    let canonical_repo_root =
        fs::canonicalize(repo_root).map_err(|io| RegistrySourceError::Read {
            input: source.to_owned(),
            io,
        })?;
    let candidate = repo_root.join(relative_path);
    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|io| RegistrySourceError::Read {
            input: source.to_owned(),
            io,
        })?;

    if !canonical_candidate.starts_with(&canonical_repo_root) {
        return Err(RegistrySourceError::PathEscapesRepo {
            input: source.to_owned(),
        });
    }

    Ok(canonical_candidate)
}

fn validate_cache_language_id(language_id: &str) -> Result<(), RegistrySourceError> {
    let path = Path::new(language_id);
    let mut components = path.components();

    match components.next() {
        Some(Component::Normal(component))
            if component == std::ffi::OsStr::new(language_id) && components.next().is_none() =>
        {
            Ok(())
        }
        _ => Err(RegistrySourceError::PathEscapesRepo {
            input: language_id.to_owned(),
        }),
    }
}

fn ensure_cache_directory_within_repo(
    repo_root: &Path,
    directory: &Path,
    source: &str,
) -> Result<(), RegistrySourceError> {
    let canonical_repo_root =
        fs::canonicalize(repo_root).map_err(|io| RegistrySourceError::Read {
            input: source.to_owned(),
            io,
        })?;
    let relative =
        directory
            .strip_prefix(repo_root)
            .map_err(|_| RegistrySourceError::PathEscapesRepo {
                input: source.to_owned(),
            })?;
    let mut current = repo_root.to_path_buf();

    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(RegistrySourceError::PathEscapesRepo {
                        input: source.to_owned(),
                    });
                }
                if !metadata.is_dir() {
                    return Err(RegistrySourceError::CacheWrite {
                        input: source.to_owned(),
                        path: current.display().to_string(),
                        io: std::io::Error::other("cache parent is not a directory"),
                    });
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|io| RegistrySourceError::CacheWrite {
                    input: source.to_owned(),
                    path: current.display().to_string(),
                    io,
                })?;
            }
            Err(io) => {
                return Err(RegistrySourceError::CacheWrite {
                    input: source.to_owned(),
                    path: current.display().to_string(),
                    io,
                });
            }
        }
    }

    let canonical_directory =
        fs::canonicalize(directory).map_err(|io| RegistrySourceError::CacheWrite {
            input: source.to_owned(),
            path: directory.display().to_string(),
            io,
        })?;
    if !canonical_directory.starts_with(&canonical_repo_root) {
        return Err(RegistrySourceError::PathEscapesRepo {
            input: source.to_owned(),
        });
    }

    Ok(())
}

fn reject_symlink_path(path: &Path, source: &str) -> Result<(), RegistrySourceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(RegistrySourceError::CacheWrite {
            input: source.to_owned(),
            path: path.display().to_string(),
            io: std::io::Error::other("cache target must not be a symlink"),
        }),
        Ok(metadata) if metadata.is_dir() => Err(RegistrySourceError::CacheWrite {
            input: source.to_owned(),
            path: path.display().to_string(),
            io: std::io::Error::other("cache target is a directory"),
        }),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn file_url_to_path(url: &str) -> Result<PathBuf, RegistrySourceError> {
    let Some(rest) = url.strip_prefix("file://") else {
        return Err(RegistrySourceError::InvalidFileUrl {
            input: url.to_owned(),
            reason: "missing file:// prefix",
        });
    };

    let path_part = if rest.starts_with('/') {
        rest.to_owned()
    } else {
        let Some((host, path)) = rest.split_once('/') else {
            return Err(RegistrySourceError::InvalidFileUrl {
                input: url.to_owned(),
                reason: "missing absolute path",
            });
        };

        if host != "localhost" {
            return Err(RegistrySourceError::InvalidFileUrl {
                input: url.to_owned(),
                reason: "only empty host or localhost are supported",
            });
        }

        format!("/{path}")
    };

    if !path_part.starts_with('/') {
        return Err(RegistrySourceError::InvalidFileUrl {
            input: url.to_owned(),
            reason: "path must be absolute",
        });
    }

    let decoded =
        percent_decode(&path_part).map_err(|reason| RegistrySourceError::InvalidFileUrl {
            input: url.to_owned(),
            reason,
        })?;

    Ok(PathBuf::from(decoded))
}

fn percent_decode(input: &str) -> Result<String, &'static str> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("incomplete percent-encoding");
                }
                let high = from_hex(bytes[index + 1]).ok_or("invalid percent-encoding")?;
                let low = from_hex(bytes[index + 2]).ok_or("invalid percent-encoding")?;
                out.push((high << 4) | low);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(out).map_err(|_| "invalid UTF-8 in decoded path")
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        work: PathBuf,
        remote: PathBuf,
        first: String,
        second: String,
        first_sha256: String,
        second_sha256: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git_available() -> bool {
        match Command::new("git").arg("--version").output() {
            Ok(output) if output.status.success() => true,
            Ok(_) | Err(_) => false,
        }
    }

    fn command(args: &[&str], cwd: &Path) -> Output {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Wax Test",
                "-c",
                "user.email=wax-test@example.com",
            ])
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn git_output(args: &[&str], cwd: &Path) -> String {
        String::from_utf8(command(args, cwd).stdout)
            .expect("git output is UTF-8")
            .trim()
            .to_ascii_lowercase()
    }

    fn registry_json(symbol: &str) -> Vec<u8> {
        format!(r#"{{"schema_version":1,"components":[{{"id":"ds.button","symbol":"{symbol}"}}]}}"#)
            .into_bytes()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }

    fn fixture() -> Fixture {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "wax-registry-source-git-{}-{id}",
            std::process::id()
        ));
        let work = root.join("work");
        let remote = root.join("remote.git");
        fs::create_dir_all(&work).expect("fixture worktree");
        command(&["init", "-q"], &work);
        command(
            &["init", "--bare", "-q", "--", remote.to_str().unwrap()],
            &root,
        );
        fs::create_dir_all(work.join(".wax/registries")).expect("registry directory");
        let first_bytes = registry_json("ButtonV1");
        let first_sha256 = sha256_hex(&first_bytes);
        fs::write(work.join(".wax/registries/compose.json"), &first_bytes).expect("first registry");
        command(&["add", "."], &work);
        command(&["commit", "-m", "first", "-q"], &work);
        let first = git_output(&["rev-parse", "HEAD"], &work);
        command(&["tag", "v1"], &work);
        command(
            &["remote", "add", "origin", remote.to_str().unwrap()],
            &work,
        );
        command(&["push", "-q", "origin", "--tags", "HEAD"], &work);

        let second_bytes = registry_json("ButtonV2");
        let second_sha256 = sha256_hex(&second_bytes);
        fs::write(work.join(".wax/registries/compose.json"), &second_bytes)
            .expect("second registry");
        command(&["add", "."], &work);
        command(&["commit", "-m", "second", "-q"], &work);
        let second = git_output(&["rev-parse", "HEAD"], &work);
        command(&["push", "-q", "origin", "HEAD"], &work);

        Fixture {
            root,
            work,
            remote,
            first,
            second,
            first_sha256,
            second_sha256,
        }
    }

    impl Fixture {
        fn move_v1_tag(&self) {
            command(&["tag", "-f", "v1"], &self.work);
            command(&["push", "-q", "--force", "origin", "v1"], &self.work);
        }

        fn remote_url(&self) -> String {
            self.remote.to_str().unwrap().to_owned()
        }

        fn locked(&self, commit: &str, sha256: &str) -> LockedRegistry {
            let git = self.remote_url();
            LockedRegistry {
                source: format!("git:{git}#v1"),
                sha256: sha256.to_owned(),
                git: Some(git),
                tag: Some("v1".to_owned()),
                commit: Some(commit.to_owned()),
            }
        }
    }

    #[test]
    fn upgrade_resolves_moving_git_tag_instead_of_locked_commit() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let repo = fixture.root.join("app");
        fs::create_dir_all(&repo).expect("app repo");
        let registry = LanguageRegistrySource::Git {
            git: fixture.remote_url(),
            tag: "v1".to_owned(),
        };
        let locked = fixture.locked(&fixture.first, &fixture.first_sha256);

        let pinned = resolve_language_registry_source(
            &repo,
            "compose",
            Some(&registry),
            Some(&locked),
            false,
        )
        .unwrap();
        assert_eq!(pinned.git_commit.as_deref(), Some(fixture.first.as_str()));
        assert_eq!(pinned.sha256, fixture.first_sha256);

        fixture.move_v1_tag();

        let still_pinned = resolve_language_registry_source(
            &repo,
            "compose",
            Some(&registry),
            Some(&locked),
            false,
        )
        .unwrap();
        assert_eq!(
            still_pinned.git_commit.as_deref(),
            Some(fixture.first.as_str())
        );
        assert_eq!(still_pinned.sha256, fixture.first_sha256);

        let upgraded = resolve_language_registry_source(
            &repo,
            "compose",
            Some(&registry),
            Some(&locked),
            true,
        )
        .unwrap();
        assert_eq!(
            upgraded.git_commit.as_deref(),
            Some(fixture.second.as_str())
        );
        assert_eq!(upgraded.sha256, fixture.second_sha256);
        assert_eq!(
            upgraded.repo_relative_path,
            format!(
                "{REGISTRY_CACHE_RELATIVE_DIR}/compose-{}.json",
                fixture.second_sha256
            )
        );
    }
}
