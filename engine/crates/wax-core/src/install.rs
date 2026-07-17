//! Secure download, verification, and atomic installation of language packs.

use flate2::read::GzDecoder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Write};
use std::path::{Component, Path, PathBuf};
use tar::Archive;
use thiserror::Error;
use wax_contract::LanguageId;

use crate::paths::{PathsError, lang_install_dir, validate_version_segment, wax_home};

/// Manifest fields written to `manifest.json` next to installed pack binaries.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LanguagePackManifestSpec {
    /// Stable language id (must match [`install_language`] `id`).
    pub id: LanguageId,
    /// Pack version string (must match [`install_language`] `version`).
    pub version: String,
    /// Wire API version supported by this pack.
    pub api_version: u32,
    /// Launch command argv for the language server subprocess.
    pub command: Vec<String>,
    /// Ecosystem identifier string for UX or telemetry.
    pub ecosystem: String,
    /// Parser identifier attached to this pack.
    pub parser_name: String,
    /// Parser semver released alongside this pack.
    pub parser_version: String,
}

/// Errors produced while downloading or installing a language pack.
#[derive(Debug, Error)]
pub enum InstallError {
    /// Expected artifact digest disagrees with the pack index before download.
    #[error(
        "digest drift between lockfile or caller digest ({expected}) and pack index ({pack_index}); refusing install"
    )]
    DigestDrift {
        /// Digest supplied by caller (typically lockfile-pinned).
        expected: String,
        /// Digest declared by the registry pack index for this artifact.
        pack_index: String,
    },
    /// Paths helper failure resolving wax home or install directories.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Artifact URL scheme is not supported.
    #[error(
        "unsupported artifact URL scheme for {url}; supported schemes are file://, http://, https://"
    )]
    UnsupportedScheme {
        /// Original artifact URL string.
        url: String,
    },
    /// Malformed `file://` URL for local artifact reads.
    #[error("invalid file:// artifact URL {url}: {reason}")]
    InvalidFileUrl {
        /// Original artifact URL string.
        url: String,
        /// Parse failure detail.
        reason: &'static str,
    },
    /// Artifact bytes could not be fetched from disk or network.
    #[error("failed to fetch artifact from {url}: {source}")]
    Fetch {
        /// Artifact URL string.
        url: String,
        /// Underlying I/O or HTTP failure.
        #[source]
        source: FetchError,
    },
    /// Caller-supplied SHA-256 hex string is invalid.
    #[error("invalid SHA-256 hex digest {digest:?}: {reason}")]
    InvalidDigestHex {
        /// Provided digest string.
        digest: String,
        /// Reason detail.
        reason: &'static str,
    },
    /// Downloaded artifact digest does not match the expected digest.
    #[error("artifact SHA-256 mismatch for {url}: expected {expected}, computed {computed}")]
    ShaMismatch {
        /// Artifact URL string.
        url: String,
        /// Expected lowercase hex digest.
        expected: String,
        /// Computed lowercase hex digest.
        computed: String,
    },
    /// Language pack manifest disagrees with install parameters.
    #[error(
        "manifest fields disagree with install parameters (manifest id={manifest_id}, version={manifest_version}; install id={install_id}, version={install_version})"
    )]
    ManifestMismatch {
        /// Id encoded in manifest JSON.
        manifest_id: String,
        /// Version encoded in manifest JSON.
        manifest_version: String,
        /// Id requested for install.
        install_id: String,
        /// Version requested for install.
        install_version: String,
    },
    /// First argv element for launch command is missing or invalid.
    #[error("manifest.command must include at least one binary path")]
    MissingPrimaryBinary,
    /// Primary binary path uses unsupported syntax for resolving install-relative paths.
    #[error("manifest.command[0] must start with ./ when specifying a bundled binary")]
    InvalidPrimaryBinaryPath,
    /// Primary binary path does not reference a regular file inside the install directory.
    #[error(
        "manifest.command[0] must reference a regular file inside the install directory (got {path})"
    )]
    InvalidPrimaryBinary {
        /// Resolved primary binary path.
        path: String,
    },
    /// Archive contained an unsupported entry type (symlinks, hard links, etc.).
    #[error("unsupported archive entry type for path {path:?}")]
    UnsupportedArchiveEntry {
        /// Archive-relative path component text.
        path: String,
    },
    /// Archive entry attempted path traversal outside the staging directory.
    #[error("unsafe archive path {path:?}; dot-dot segments are not allowed")]
    PathTraversal {
        /// Raw archive entry path text.
        path: String,
    },
    /// Destination install directory already exists; reinstall/update is deferred to CLI update semantics.
    #[error(
        "language pack already installed at {path}; remove it with `wax language uninstall` before reinstalling"
    )]
    AlreadyInstalled {
        /// Existing install directory path.
        path: String,
    },
    /// Generic filesystem failure during staging or promotion.
    #[error("{context}: {source}")]
    Io {
        /// Operation description for debugging.
        context: String,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
}

/// Nested fetch failures surfaced through [`InstallError::Fetch`].
#[derive(Debug, Error)]
pub enum FetchError {
    /// Local artifact read failure.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// HTTP client failure for remote artifact URLs.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

/// Downloads an artifact, verifies digest, extracts contents to a staging dir,
/// writes `manifest.json`, optionally compares registry digest against lockfile
/// digest before download, then atomically promotes into `~/.wax/langs/<id>/<version>`.
///
/// Returns [`InstallError::AlreadyInstalled`] when that destination path already
/// exists. Same-version reinstall and in-place update are intentionally out of
/// scope here and belong to later CLI update/replace semantics.
///
/// When `pack_index_digest_hex` is `Some`, it must equal `expected_digest_hex`
/// (after normalization); otherwise [`InstallError::DigestDrift`] is returned
/// before any fetch so lockfile pins cannot silently track registry drift.
///
/// # Errors
///
/// Returns [`InstallError::Paths`] for unsafe versions or unresolved wax paths;
/// [`InstallError::ManifestMismatch`], [`InstallError::MissingPrimaryBinary`],
/// [`InstallError::InvalidPrimaryBinaryPath`], or
/// [`InstallError::InvalidPrimaryBinary`] for invalid manifest contents;
/// [`InstallError::InvalidDigestHex`], [`InstallError::DigestDrift`], or
/// [`InstallError::ShaMismatch`] for integrity failures; and
/// [`InstallError::UnsupportedScheme`], [`InstallError::InvalidFileUrl`], or
/// [`InstallError::Fetch`] when the artifact cannot be fetched. Unsafe archive
/// entries return [`InstallError::UnsupportedArchiveEntry`] or
/// [`InstallError::PathTraversal`]; destination conflicts and filesystem
/// failures return [`InstallError::AlreadyInstalled`] or [`InstallError::Io`].
pub fn install_language(
    id: &LanguageId,
    version: &str,
    target_triple: &str,
    artifact_url: &str,
    expected_digest_hex: &str,
    pack_index_digest_hex: Option<&str>,
    manifest: &LanguagePackManifestSpec,
) -> Result<PathBuf, InstallError> {
    let _ = target_triple;
    validate_version_segment(version)?;

    if manifest.id.as_str() != id.as_str() || manifest.version != version {
        return Err(InstallError::ManifestMismatch {
            manifest_id: manifest.id.to_string(),
            manifest_version: manifest.version.clone(),
            install_id: id.to_string(),
            install_version: version.to_owned(),
        });
    }

    let expected_digest = normalize_sha256_hex(expected_digest_hex)?;
    if let Some(pack_digest_raw) = pack_index_digest_hex {
        let pack_digest = normalize_sha256_hex(pack_digest_raw)?;
        if expected_digest != pack_digest {
            return Err(InstallError::DigestDrift {
                expected: expected_digest,
                pack_index: pack_digest,
            });
        }
    }

    let destination = lang_install_dir(id, version)?;
    if destination.exists() {
        return Err(InstallError::AlreadyInstalled {
            path: destination.display().to_string(),
        });
    }

    let bytes = fetch_artifact_bytes(artifact_url)?;

    let computed_digest = hex_lower_sha256(&bytes);
    if computed_digest != expected_digest {
        return Err(InstallError::ShaMismatch {
            url: artifact_url.to_owned(),
            expected: expected_digest,
            computed: computed_digest,
        });
    }

    let langs_parent = wax_home()?.join("langs").join(id.as_str());
    fs::create_dir_all(&langs_parent).map_err(|source| InstallError::Io {
        context: format!("create langs staging parent {}", langs_parent.display()),
        source,
    })?;

    let staging_dir = allocate_staging_dir(&langs_parent)?;

    let staging_result: Result<(), InstallError> = (|| {
        unpack_tar_gz_secure(&bytes, &staging_dir)?;
        write_manifest_json(&staging_dir, manifest, target_triple, &expected_digest).map_err(
            |source| InstallError::Io {
                context: format!("write manifest.json under {}", staging_dir.display()),
                source,
            },
        )?;
        let bin_path = validated_manifest_binary(&staging_dir, manifest)?;
        apply_unix_executable_bit(&bin_path).map_err(|source| InstallError::Io {
            context: format!("set executable bit for {}", bin_path.display()),
            source,
        })?;
        Ok(())
    })();

    if let Err(err) = staging_result {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(err);
    }

    let destination = lang_install_dir(id, version)?;
    if let Err(err) = promote_staging_dir(&staging_dir, &destination) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(err);
    }

    Ok(destination)
}

fn normalize_sha256_hex(hex: &str) -> Result<String, InstallError> {
    let trimmed = hex.trim();
    if trimmed.len() != 64 {
        return Err(InstallError::InvalidDigestHex {
            digest: hex.to_owned(),
            reason: "expected 64 hexadecimal characters",
        });
    }
    if !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(InstallError::InvalidDigestHex {
            digest: hex.to_owned(),
            reason: "digest contains non-hex characters",
        });
    }
    Ok(trimmed.to_ascii_lowercase())
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

fn fetch_artifact_bytes(url: &str) -> Result<Vec<u8>, InstallError> {
    if url.starts_with("file://") {
        let path = file_url_to_path(url)?;
        return fs::read(&path).map_err(|source| InstallError::Fetch {
            url: url.to_owned(),
            source: FetchError::Io(source),
        });
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|source| InstallError::Fetch {
                url: url.to_owned(),
                source: FetchError::Http(source),
            })?;
        let response = client
            .get(url)
            .send()
            .map_err(|source| InstallError::Fetch {
                url: url.to_owned(),
                source: FetchError::Http(source),
            })?;
        let response = response
            .error_for_status()
            .map_err(|source| InstallError::Fetch {
                url: url.to_owned(),
                source: FetchError::Http(source),
            })?;
        let bytes = response.bytes().map_err(|source| InstallError::Fetch {
            url: url.to_owned(),
            source: FetchError::Http(source),
        })?;
        return Ok(bytes.to_vec());
    }

    Err(InstallError::UnsupportedScheme {
        url: url.to_owned(),
    })
}

fn file_url_to_path(url: &str) -> Result<PathBuf, InstallError> {
    let Some(rest) = url.strip_prefix("file://") else {
        return Err(InstallError::InvalidFileUrl {
            url: url.to_owned(),
            reason: "missing file:// prefix",
        });
    };

    let path_part = if rest.starts_with('/') {
        rest.to_owned()
    } else {
        let Some((host, path)) = rest.split_once('/') else {
            return Err(InstallError::InvalidFileUrl {
                url: url.to_owned(),
                reason: "missing absolute path",
            });
        };

        if host != "localhost" {
            return Err(InstallError::InvalidFileUrl {
                url: url.to_owned(),
                reason: "only empty host or localhost are supported",
            });
        }

        format!("/{path}")
    };

    if !path_part.starts_with('/') {
        return Err(InstallError::InvalidFileUrl {
            url: url.to_owned(),
            reason: "path must be absolute",
        });
    }

    let decoded = percent_decode(&path_part).ok_or_else(|| InstallError::InvalidFileUrl {
        url: url.to_owned(),
        reason: "percent decoding failed",
    })?;

    Ok(PathBuf::from(decoded))
}

fn percent_decode(input: &str) -> Option<String> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = from_hex(bytes[i + 1])?;
                let lo = from_hex(bytes[i + 2])?;
                out.push(hi << 4 | lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(out).ok()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn allocate_staging_dir(parent: &Path) -> Result<PathBuf, InstallError> {
    for attempt in 0u32..1000 {
        let staging = parent.join(format!(".install-{}-{attempt}.tmp", std::process::id(),));

        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(InstallError::Io {
                    context: format!("create staging dir {}", staging.display()),
                    source,
                });
            }
        }
    }

    Err(InstallError::Io {
        context: "allocate unique staging directory".to_owned(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate unique staging directory",
        ),
    })
}

fn unpack_tar_gz_secure(bytes: &[u8], dst: &Path) -> Result<(), InstallError> {
    let cursor = Cursor::new(bytes);
    let gz = GzDecoder::new(cursor);
    let mut archive = Archive::new(gz);

    for entry_result in archive.entries().map_err(|source| InstallError::Io {
        context: "read tar archive entries".to_owned(),
        source,
    })? {
        let mut entry = entry_result.map_err(|source| InstallError::Io {
            context: "read tar archive entry".to_owned(),
            source,
        })?;

        let entry_type = entry.header().entry_type();
        match entry_type {
            tar::EntryType::Regular | tar::EntryType::Continuous => {}
            tar::EntryType::Directory => {}
            _ => {
                let path = entry
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return Err(InstallError::UnsupportedArchiveEntry { path });
            }
        }

        let raw_path = entry.path().map_err(|source| InstallError::Io {
            context: "decode tar entry path".to_owned(),
            source,
        })?;
        let safe_relative = validated_archive_relative_path(raw_path.as_ref())?;

        let out_path = dst.join(&safe_relative);

        if entry_type == tar::EntryType::Directory {
            fs::create_dir_all(&out_path).map_err(|source| InstallError::Io {
                context: format!("mkdir {}", out_path.display()),
                source,
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|source| InstallError::Io {
                context: format!("mkdir {}", parent.display()),
                source,
            })?;
        }

        let mut outfile = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out_path)
            .map_err(|source| InstallError::Io {
                context: format!("create {}", out_path.display()),
                source,
            })?;

        std::io::copy(&mut entry, &mut outfile).map_err(|source| InstallError::Io {
            context: format!("unpack {}", out_path.display()),
            source,
        })?;
    }

    Ok(())
}

fn validated_archive_relative_path(path: &Path) -> Result<PathBuf, InstallError> {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => cleaned.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(InstallError::PathTraversal {
                    path: path.display().to_string(),
                });
            }
        }
    }

    if cleaned.as_os_str().is_empty() {
        return Err(InstallError::PathTraversal {
            path: path.display().to_string(),
        });
    }

    Ok(cleaned)
}

#[derive(Serialize)]
struct PersistedLanguagePackManifest<'a> {
    #[serde(flatten)]
    manifest: &'a LanguagePackManifestSpec,
    target: &'a str,
    sha256: &'a str,
}

fn write_manifest_json(
    dir: &Path,
    manifest: &LanguagePackManifestSpec,
    target: &str,
    sha256: &str,
) -> io::Result<()> {
    let persisted = PersistedLanguagePackManifest {
        manifest,
        target,
        sha256,
    };
    let serialized = serde_json::to_string_pretty(&persisted)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let manifest_path = dir.join("manifest.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)?;
    file.write_all(serialized.as_bytes())?;
    file.write_all(b"\n")?;

    Ok(())
}

fn validated_manifest_binary(
    staging_dir: &Path,
    manifest: &LanguagePackManifestSpec,
) -> Result<PathBuf, InstallError> {
    let primary = manifest
        .command
        .first()
        .ok_or(InstallError::MissingPrimaryBinary)?;

    let rel = primary
        .strip_prefix("./")
        .ok_or(InstallError::InvalidPrimaryBinaryPath)?;

    let safe_relative = validated_archive_relative_path(Path::new(rel))?;
    let bin_path = staging_dir.join(&safe_relative);

    if !bin_path.starts_with(staging_dir) {
        return Err(InstallError::PathTraversal {
            path: rel.to_owned(),
        });
    }

    let meta = fs::metadata(&bin_path).map_err(|source| InstallError::Io {
        context: format!("stat manifest primary binary {}", bin_path.display()),
        source,
    })?;

    if !meta.is_file() {
        return Err(InstallError::InvalidPrimaryBinary {
            path: bin_path.display().to_string(),
        });
    }

    Ok(bin_path)
}

fn apply_unix_executable_bit(bin_path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let meta = fs::metadata(bin_path)?;
        let mut permissions = meta.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        fs::set_permissions(bin_path, permissions)?;
    }

    #[cfg(not(unix))]
    {
        let _ = bin_path;
    }

    Ok(())
}

fn promote_staging_dir(staging_dir: &Path, destination: &Path) -> Result<(), InstallError> {
    if destination.exists() {
        return Err(InstallError::AlreadyInstalled {
            path: destination.display().to_string(),
        });
    }

    match fs::rename(staging_dir, destination) {
        Ok(()) => Ok(()),
        Err(_source) if destination.exists() => Err(InstallError::AlreadyInstalled {
            path: destination.display().to_string(),
        }),
        Err(source) => Err(InstallError::Io {
            context: format!(
                "promote staged dir {} -> {}",
                staging_dir.display(),
                destination.display()
            ),
            source,
        }),
    }
}
