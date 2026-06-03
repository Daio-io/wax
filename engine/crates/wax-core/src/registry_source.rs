//! Design-system registry source resolution.

use crate::config::repo_files::{DEFAULT_REGISTRY_RELATIVE_PATH, REGISTRY_CACHE_RELATIVE_DIR};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const HTTP_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Resolved registry source ready for downstream config rewriting and locking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRegistrySource {
    /// Original source string, defaulted when config omitted it.
    pub source: String,
    /// Repo-relative path to the materialized registry JSON.
    pub repo_relative_path: String,
    /// Lowercase hexadecimal SHA-256 digest of the registry bytes.
    pub sha256: String,
    /// Whether this source came from a deprecated config field.
    pub deprecated: bool,
}

/// Typed failures while resolving registry sources.
#[derive(Debug, Error)]
pub enum RegistrySourceError {
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
}

/// Resolves a registry source and returns the local repo-relative materialized path.
pub fn resolve_registry_source(
    input: RegistrySourceInput<'_>,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    resolve_registry_source_with_deprecation(input, false)
}

/// Resolves a registry source and preserves whether it came from a deprecated field.
pub fn resolve_registry_source_with_deprecation(
    input: RegistrySourceInput<'_>,
    deprecated: bool,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    let source = input
        .source
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .unwrap_or(DEFAULT_REGISTRY_RELATIVE_PATH);

    let source = source.to_owned();
    let (bytes, repo_relative_path, external) = read_source(input.repo_root, &source)?;
    validate_registry_json(&source, &bytes)?;
    let sha256 = hex_lower_sha256(&bytes);

    let repo_relative_path = if external {
        materialize_external_registry(input.repo_root, input.language_id, &source, &sha256, &bytes)?
    } else {
        repo_relative_path
    };

    Ok(ResolvedRegistrySource {
        source,
        repo_relative_path,
        sha256,
        deprecated,
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

fn validate_registry_json(source: &str, bytes: &[u8]) -> Result<(), RegistrySourceError> {
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

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|io| RegistrySourceError::CacheWrite {
            input: source.to_owned(),
            path: path.display().to_string(),
            io,
        })?;
    }

    fs::write(&path, bytes).map_err(|io| RegistrySourceError::CacheWrite {
        input: source.to_owned(),
        path: path.display().to_string(),
        io,
    })?;

    Ok(repo_relative_path)
}

fn hex_lower_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
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
