//! `wax.lock.json` repository lockfile parsing and consistency checks.

use crate::config::waxrc::WaxRc;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;
use time::OffsetDateTime;
use wax_contract::LanguageId;

/// Current `wax.lock.json` schema version supported by this engine.
pub const WAX_LOCK_SCHEMA_VERSION: u32 = 1;

/// Repository lockfile pinning the language pack artifacts selected for a repo.
#[derive(Debug, Clone, Deserialize, PartialEq)]
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
    /// Locked language pack artifacts by validated language id.
    pub languages: BTreeMap<LanguageId, LockedLanguage>,
}

/// Lockfile entry for one resolved language pack.
#[derive(Debug, Clone, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SignatureRef {
    /// Signature metadata format, for example `sigstore-cosign-bundle`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Format-owned bundle metadata.
    pub bundle: serde_json::Value,
}

/// Mismatch report comparing enabled `.waxrc` languages with lockfile entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaxLockLanguageReport {
    /// Enabled `.waxrc` language ids missing from `wax.lock.json`.
    pub missing_enabled_languages: BTreeSet<LanguageId>,
    /// Lockfile language ids that are disabled or absent in `.waxrc`.
    pub stale_locked_languages: BTreeSet<LanguageId>,
}

#[derive(Debug, Deserialize)]
struct WaxLockVersion {
    schema_version: u32,
}

/// Errors returned while loading `wax.lock.json`.
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
    /// The JSON is valid but does not match the supported lockfile shape.
    #[error("invalid wax.lock.json config in {path}: {source}")]
    InvalidConfig {
        /// Path passed to [`load_lockfile`].
        path: String,
        /// Underlying lockfile decoding error.
        #[source]
        source: serde_json::Error,
    },
    /// The file uses a schema version this engine does not understand.
    #[error(
        "unsupported wax.lock.json schema_version {found} in {path}; this engine supports {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Path passed to [`load_lockfile`].
        path: String,
        /// Schema version found in the file.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
}

/// Loads and validates a `wax.lock.json` file from disk.
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
    if version.schema_version != WAX_LOCK_SCHEMA_VERSION {
        return Err(LockfileError::UnsupportedSchemaVersion {
            path: path_display,
            found: version.schema_version,
            supported: WAX_LOCK_SCHEMA_VERSION,
        });
    }

    let lock: WaxLock =
        serde_json::from_value(value).map_err(|source| LockfileError::InvalidConfig {
            path: path_display,
            source,
        })?;

    Ok(lock)
}

/// Compares enabled `.waxrc` language ids against the ids pinned in a lockfile.
pub fn check_waxrc_lockfile_languages(waxrc: &WaxRc, lockfile: &WaxLock) -> WaxLockLanguageReport {
    let enabled: BTreeSet<LanguageId> = waxrc
        .languages
        .iter()
        .filter(|language| language.enabled)
        .map(|language| language.id.clone())
        .collect();
    let locked: BTreeSet<LanguageId> = lockfile.languages.keys().cloned().collect();

    WaxLockLanguageReport {
        missing_enabled_languages: enabled.difference(&locked).cloned().collect(),
        stale_locked_languages: locked.difference(&enabled).cloned().collect(),
    }
}
