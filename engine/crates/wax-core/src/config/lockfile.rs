//! `wax.lock.json` repository lockfile loading, saving, and consistency checks.

use crate::atomic_file::{AtomicWriteError, AtomicWriteOptions, write_atomically};
use crate::config::waxrc::WaxRc;
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;
use time::OffsetDateTime;
use wax_contract::LanguageId;

/// Current `wax.lock.json` schema version written by this engine.
pub const WAX_LOCK_SCHEMA_VERSION: u32 = 3;
const MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION: u32 = 1;

/// Repository lockfile pinning the language pack artifacts selected for a repo.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WaxLock {
    /// Version of the `wax.lock.json` JSON schema.
    pub schema_version: u32,
    /// Engine orchestration API version expected by the locked language packs.
    pub engine_api_version: u32,
    /// Version of the wax engine that wrote this lockfile.
    pub wax_version: String,
    /// Time this lockfile was produced, when recorded by the writer.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub locked_at: Option<OffsetDateTime>,
    /// Locked design-system registry sources by validated language id.
    pub registries: BTreeMap<LanguageId, LockedRegistry>,
    /// Locked language pack artifacts by validated language id.
    pub languages: BTreeMap<LanguageId, LockedLanguage>,
}

/// Lockfile entry for one resolved design-system registry.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedRegistry {
    /// Registry source string from config or default resolution.
    pub source: String,
    /// SHA-256 digest of the exact registry JSON content.
    pub sha256: String,
    /// Exact Git URL when this registry was resolved from Git.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    /// Configured Git tag or ref that produced this pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Full Git commit object id that produced this pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Optional repository-relative directory containing the registry bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdir: Option<String>,
    /// Exact publisher definition selected with this registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<wax_contract::RegistryDefinitionPin>,
}

/// Lockfile entry for one resolved language pack.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedLanguage {
    /// Locked language pack version.
    pub version: String,
    /// Language pack API version verified by the engine before spawning.
    pub api_version: u32,
    /// Pack index URL or mirror id that produced this resolution.
    pub source: String,
    /// Machine-specific artifact selected when this lockfile was produced.
    pub resolved: ResolvedLanguage,
}

/// Resolved artifact metadata for the machine that produced the lockfile.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedLanguage {
    /// Target triple for the selected language pack artifact.
    pub target: String,
    /// Download URL for the selected artifact.
    pub url: String,
    /// Expected SHA-256 digest of the selected artifact.
    pub sha256: String,
    /// Reserved signature bundle reference for future Sigstore/cosign metadata.
    ///
    /// Schema v1 writes `null`. `SignatureRef` still rejects unknown fields;
    /// only `bundle` is arbitrary JSON owned by the signature format, so a
    /// future v1.1 writer can carry upstream bundle metadata without changing
    /// the surrounding lockfile shape.
    pub signature: Option<SignatureRef>,
}

/// Reserved reference to signature metadata for a resolved artifact.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureRef {
    /// Signature metadata format, for example `sigstore-cosign-bundle`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Format-owned bundle metadata.
    pub bundle: serde_json::Value,
}

/// Mismatch report comparing configured languages with lockfile entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaxLockLanguageReport {
    /// Configured language ids missing from `wax.lock.json`.
    pub missing_enabled_languages: BTreeSet<LanguageId>,
    /// Lockfile language ids that are absent from config.
    pub stale_locked_languages: BTreeSet<LanguageId>,
}

#[derive(Debug, Deserialize)]
struct WaxLockVersion {
    schema_version: u32,
}

/// Errors returned while loading or saving `wax.lock.json`.
#[derive(Debug, Error)]
pub enum LockfileError {
    /// The file could not be read from disk.
    #[error("failed to read wax.lock.json from {path}: {source}")]
    Read {
        /// Path passed to [`load_lockfile`].
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not syntactically valid JSON.
    #[error("malformed wax.lock.json in {path}: {source}")]
    MalformedJson {
        /// Path passed to [`load_lockfile`].
        path: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// The lockfile could not be decoded or serialized.
    #[error("invalid wax.lock.json config in {path}: {source}")]
    InvalidConfig {
        /// Lockfile path.
        path: String,
        /// Underlying lockfile decoding or serialization error.
        #[source]
        source: serde_json::Error,
    },
    /// The lockfile contains invalid bundle registry pins.
    #[error("invalid wax.lock.json bundle lock in {path}: {source}")]
    InvalidBundleLock {
        /// Path passed to [`save_lockfile`].
        path: String,
        /// Underlying bundle validation error.
        #[source]
        source: wax_contract::CatalogError,
    },
    /// The lockfile could not be written atomically.
    #[error("failed to write wax.lock.json to {path}: {source}")]
    Write {
        /// Path passed to [`save_lockfile`].
        path: String,
        /// Underlying atomic-write error.
        #[source]
        source: AtomicWriteError,
    },
    /// The file uses a schema version this engine does not understand.
    #[error(
        "unsupported wax.lock.json schema_version {found} in {path}; this engine supports {min_supported} through {max_supported}"
    )]
    UnsupportedSchemaVersion {
        /// Path passed to [`load_lockfile`].
        path: String,
        /// Schema version found in the file.
        found: u32,
        /// Minimum schema version supported by this crate.
        min_supported: u32,
        /// Maximum schema version written by this crate.
        max_supported: u32,
    },
}

/// Loads and validates a `wax.lock.json` file from disk.
///
/// # Errors
///
/// Returns [`LockfileError::Read`] when the file cannot be read,
/// [`LockfileError::MalformedJson`] for invalid JSON,
/// [`LockfileError::InvalidConfig`] for an incompatible shape, or
/// [`LockfileError::UnsupportedSchemaVersion`] for an unsupported version.
pub fn load_lockfile(path: impl AsRef<Path>) -> Result<WaxLock, LockfileError> {
    let path = path.as_ref();
    let path_display = path.display().to_string();
    let contents = fs::read_to_string(path).map_err(|source| LockfileError::Read {
        path: path_display.clone(),
        source,
    })?;

    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|source| LockfileError::MalformedJson {
            path: path_display.clone(),
            source,
        })?;

    let version: WaxLockVersion =
        serde_json::from_value(value.clone()).map_err(|source| LockfileError::InvalidConfig {
            path: path_display.clone(),
            source,
        })?;
    if version.schema_version < MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION
        || version.schema_version > WAX_LOCK_SCHEMA_VERSION
    {
        return Err(LockfileError::UnsupportedSchemaVersion {
            path: path_display,
            found: version.schema_version,
            min_supported: MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION,
            max_supported: WAX_LOCK_SCHEMA_VERSION,
        });
    }

    let mut value = value;
    if version.schema_version == MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION
        && let Some(object) = value.as_object_mut()
    {
        object
            .entry("registries")
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
    }

    let lock: WaxLock =
        serde_json::from_value(value).map_err(|source| LockfileError::InvalidConfig {
            path: path_display.clone(),
            source,
        })?;

    validate_bundle_registry_locks(&lock.registries).map_err(|source| {
        LockfileError::InvalidConfig {
            path: path_display.clone(),
            source: serde_json::Error::custom(source),
        }
    })?;
    for (language_id, registry) in &lock.registries {
        let metadata = [
            registry.git.is_some(),
            registry.tag.is_some(),
            registry.commit.is_some(),
        ];
        if metadata.iter().any(|present| *present) && !metadata.iter().all(|present| *present) {
            return Err(LockfileError::InvalidConfig {
                path: path_display,
                source: serde_json::Error::custom(format!(
                    "registry lock for {language_id} has partial git metadata"
                )),
            });
        }
    }

    Ok(lock)
}

/// Stamps [`WAX_LOCK_SCHEMA_VERSION`], validates bundle registry locks, and
/// atomically writes pretty JSON with a trailing newline.
///
/// # Errors
///
/// Returns [`LockfileError::InvalidBundleLock`] for invalid bundle pins,
/// [`LockfileError::InvalidConfig`] if serialization fails, or
/// [`LockfileError::Write`] if the atomic write fails.
pub fn save_lockfile(path: impl AsRef<Path>, lockfile: &WaxLock) -> Result<(), LockfileError> {
    let path = path.as_ref();
    validate_bundle_registry_locks(&lockfile.registries).map_err(|source| {
        LockfileError::InvalidBundleLock {
            path: path.display().to_string(),
            source,
        }
    })?;
    let mut lockfile = lockfile.clone();
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    let mut contents =
        serde_json::to_string_pretty(&lockfile).map_err(|source| LockfileError::InvalidConfig {
            path: path.display().to_string(),
            source,
        })?;
    contents.push('\n');
    write_atomically(path, contents.as_bytes(), AtomicWriteOptions::default()).map_err(|source| {
        LockfileError::Write {
            path: path.display().to_string(),
            source,
        }
    })
}

/// Compares configured language ids against the ids pinned in a lockfile.
pub fn check_waxrc_lockfile_languages(waxrc: &WaxRc, lockfile: &WaxLock) -> WaxLockLanguageReport {
    let enabled: BTreeSet<LanguageId> = waxrc
        .languages
        .iter()
        .map(|language| language.id.clone())
        .collect();
    let locked: BTreeSet<LanguageId> = lockfile.languages.keys().cloned().collect();

    WaxLockLanguageReport {
        missing_enabled_languages: enabled.difference(&locked).cloned().collect(),
        stale_locked_languages: locked.difference(&enabled).cloned().collect(),
    }
}

/// Checks nested bundle metadata and cross-language agreement without source I/O.
///
/// # Errors
/// Rejects partial pins, unsafe paths, and definitions with mixed commit/hash pins.
pub fn validate_bundle_registry_locks(
    registries: &BTreeMap<LanguageId, LockedRegistry>,
) -> Result<(), wax_contract::CatalogError> {
    let mut bundles = BTreeMap::new();
    for registry in registries.values() {
        if registry.source.starts_with("git-system:") && registry.subdir.is_none() {
            return Err(wax_contract::CatalogError(
                "nested Git source requires subdir".into(),
            ));
        }
        if registry.definition.is_some() && registry.subdir.is_none() {
            return Err(wax_contract::CatalogError(
                "definition requires subdir".into(),
            ));
        }
        if let Some(subdir) = &registry.subdir {
            wax_contract::validate_artifact_path(subdir)?;
            if registry.git.is_none()
                || registry.tag.is_none()
                || registry.commit.as_ref().is_none_or(|commit| {
                    !matches!(commit.len(), 40 | 64)
                        || !commit
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                || !wax_contract::valid_sha256(&registry.sha256)
            {
                return Err(wax_contract::CatalogError(
                    "partial bundle Git metadata".into(),
                ));
            }
            if registry.source
                != crate::registry_git::git_source_identity(
                    registry.git.as_deref().unwrap_or_default(),
                    registry.tag.as_deref().unwrap_or_default(),
                    Some(subdir),
                    registry
                        .definition
                        .as_ref()
                        .map(|pin| pin.system_id.as_str()),
                )
            {
                return Err(wax_contract::CatalogError(
                    "bundle source identity does not match pins".into(),
                ));
            }
            if let Some(definition) = &registry.definition {
                definition.validate()?;
                if definition.path != format!("{subdir}/.wax/wax.config.json") {
                    return Err(wax_contract::CatalogError(
                        "definition path does not match subdir".into(),
                    ));
                }
                let key = (&registry.git, subdir, &definition.system_id);
                let value = (&registry.commit, &definition.sha256);
                if bundles
                    .insert(key, value)
                    .is_some_and(|previous| previous != value)
                {
                    return Err(wax_contract::CatalogError(
                        "bundle languages have mixed definition pins".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}
