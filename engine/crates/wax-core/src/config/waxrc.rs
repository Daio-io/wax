//! `.waxrc` repository configuration parsing.

use serde::Deserialize;
use std::fs;
use std::path::Path;
use thiserror::Error;
use wax_contract::LanguageId;

/// Current `.waxrc` schema version supported by this engine.
pub const WAXRC_SCHEMA_VERSION: u32 = 1;

/// Repository-level wax configuration loaded from `.waxrc`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaxRc {
    /// Version of the `.waxrc` JSON schema.
    pub schema_version: u32,
    /// Engine-owned configuration.
    #[serde(default)]
    pub engine: EngineConfig,
    /// Language pack entries configured for this repository.
    pub languages: Vec<LanguageEntry>,
}

/// Engine-owned `.waxrc` settings.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    /// Maximum number of concurrent language scans.
    #[serde(default = "default_scan_concurrency")]
    pub scan_concurrency: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            scan_concurrency: default_scan_concurrency(),
        }
    }
}

/// Per-language `.waxrc` entry.
#[derive(Debug, Deserialize)]
pub struct LanguageEntry {
    /// Validated language pack identifier.
    pub id: LanguageId,
    /// Whether this language pack should run.
    pub enabled: bool,
    /// Pack-specific configuration kept opaque to the engine.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct WaxRcVersion {
    schema_version: u32,
}

/// Errors returned while loading `.waxrc`.
#[derive(Debug, Error)]
pub enum WaxRcError {
    /// The file could not be read from disk.
    #[error("failed to read .waxrc from {path}: {source}")]
    Read {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not syntactically valid JSON.
    #[error("malformed .waxrc JSON in {path}: {source}")]
    MalformedJson {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// The JSON is valid but does not match the supported `.waxrc` config shape.
    #[error("invalid .waxrc config in {path}: {source}")]
    InvalidConfig {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Underlying config decoding error.
        #[source]
        source: serde_json::Error,
    },
    /// The file uses a schema version this engine does not understand.
    #[error(
        "unsupported .waxrc schema_version {found} in {path}; this engine supports {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Schema version found in the file.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
}

/// Loads and validates a `.waxrc` JSON file from disk.
pub fn load_waxrc(path: impl AsRef<Path>) -> Result<WaxRc, WaxRcError> {
    let path = path.as_ref();
    let path_display = path.display().to_string();
    let contents = fs::read_to_string(path).map_err(|source| WaxRcError::Read {
        path: path_display.clone(),
        source,
    })?;

    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|source| WaxRcError::MalformedJson {
            path: path_display.clone(),
            source,
        })?;

    let version: WaxRcVersion =
        serde_json::from_value(value.clone()).map_err(|source| WaxRcError::InvalidConfig {
            path: path_display.clone(),
            source,
        })?;
    if version.schema_version != WAXRC_SCHEMA_VERSION {
        return Err(WaxRcError::UnsupportedSchemaVersion {
            path: path_display,
            found: version.schema_version,
            supported: WAXRC_SCHEMA_VERSION,
        });
    }

    let rc: WaxRc = serde_json::from_value(value).map_err(|source| WaxRcError::InvalidConfig {
        path: path_display,
        source,
    })?;

    Ok(rc)
}

fn default_scan_concurrency() -> u32 {
    2
}
