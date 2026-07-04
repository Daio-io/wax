//! Repository wax config parsing.

use serde::Deserialize;
use serde::de::Error as _;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use thiserror::Error;
use wax_contract::LanguageId;

/// Current wax config schema version supported by this engine.
pub const WAXRC_SCHEMA_VERSION: u32 = 2;

/// Repository-level wax configuration loaded from a repo config file.
#[derive(Debug)]
pub struct WaxRc {
    /// Version of the wax config JSON schema.
    pub schema_version: u32,
    /// Engine-owned configuration.
    pub engine: EngineConfig,
    /// Adoption Metrics v2 scan behavior.
    pub adoption: AdoptionConfig,
    /// Language pack entries configured for this repository.
    ///
    /// Presence of a language key means the language is enabled.
    pub languages: Vec<LanguageEntry>,
    /// Design-system publication configuration keyed by design-system id.
    pub design_systems: BTreeMap<String, DesignSystemConfig>,
}

/// Engine-owned wax config settings.
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

/// Adoption Metrics v2 repository settings.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AdoptionConfig {
    /// Whether parser-backed packs emit local invocation facts.
    #[serde(default = "default_true")]
    pub track_local_invocations: bool,
    /// Whether parser-backed packs emit unresolved UI invocation facts.
    #[serde(default = "default_true")]
    pub track_unresolved_invocations: bool,
    /// Parent-scope attribution settings.
    #[serde(default)]
    pub parent_attribution: ParentAttributionConfig,
    /// Candidate counting policy for primary adoption metrics.
    #[serde(default)]
    pub candidate_policy: CandidatePolicy,
    /// Derived symbol summary settings.
    #[serde(default)]
    pub symbol_usage_summary: SymbolUsageSummaryConfig,
}

impl Default for AdoptionConfig {
    fn default() -> Self {
        Self {
            track_local_invocations: true,
            track_unresolved_invocations: true,
            parent_attribution: ParentAttributionConfig::default(),
            candidate_policy: CandidatePolicy::ReportSeparately,
            symbol_usage_summary: SymbolUsageSummaryConfig::default(),
        }
    }
}

impl AdoptionConfig {
    fn validate_supported(&self) -> Result<(), serde_json::Error> {
        if !self.track_local_invocations {
            return Err(serde_json::Error::custom(
                "adoption.track_local_invocations=false is not supported yet",
            ));
        }
        if !self.track_unresolved_invocations {
            return Err(serde_json::Error::custom(
                "adoption.track_unresolved_invocations=false is not supported yet",
            ));
        }
        if !self.parent_attribution.enabled {
            return Err(serde_json::Error::custom(
                "adoption.parent_attribution.enabled=false is not supported yet",
            ));
        }
        if self.candidate_policy != CandidatePolicy::ReportSeparately {
            return Err(serde_json::Error::custom(
                "adoption.candidate_policy values other than report_separately are not supported yet",
            ));
        }
        if !self.symbol_usage_summary.enabled {
            return Err(serde_json::Error::custom(
                "adoption.symbol_usage_summary.enabled=false is not supported yet",
            ));
        }
        Ok(())
    }
}

/// Parent-scope attribution settings.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentAttributionConfig {
    /// Whether usage sites include parent attribution.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Parent visibility classes included by attribution.
    #[serde(default = "default_scope_visibility")]
    pub scope_visibility: Vec<String>,
}

impl Default for ParentAttributionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scope_visibility: default_scope_visibility(),
        }
    }
}

/// Candidate counting policy for primary adoption metrics.
#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidatePolicy {
    /// Exclude candidates from the primary numerator and denominator.
    #[default]
    ReportSeparately,
    /// Include candidates in the denominator but not the numerator.
    CountAsNonAdopted,
    /// Include candidates in both numerator and denominator.
    CountAsAdopted,
}

/// Derived symbol summary settings.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolUsageSummaryConfig {
    /// Whether the engine emits derived symbol usage summaries.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional per-symbol parent row limit.
    #[serde(default)]
    pub parent_scope_limit: Option<u32>,
}

impl Default for SymbolUsageSummaryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            parent_scope_limit: None,
        }
    }
}

/// Per-language wax config entry.
#[derive(Debug)]
pub struct LanguageEntry {
    /// Validated language pack identifier, derived from the languages map key.
    pub id: LanguageId,
    /// Repo-relative scan roots for this language.
    pub roots: Vec<String>,
    /// Optional registry source configuration.
    pub registry_source: Option<LanguageRegistrySource>,
    /// Pack-specific configuration kept opaque to the engine.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Parsed registry source setting from a language entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRegistrySource {
    /// Registry source string used for scanning.
    pub source: String,
    /// Optional upstream design-system reference (`<design-system-id>/<language-id>`).
    pub upstream: Option<String>,
}

/// Design-system publication configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignSystemConfig {
    /// Display name for prompts and lists.
    pub name: String,
    /// Published registries keyed by language id.
    pub registries: BTreeMap<LanguageId, DesignSystemRegistry>,
}

/// One published registry for a design system.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignSystemRegistry {
    /// Design-system-authored registry artifact path or URL.
    pub source: String,
    /// Optional hosted source preferred by app sync.
    #[serde(default)]
    pub published_source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaxRcRaw {
    schema_version: u32,
    #[serde(default)]
    engine: EngineConfig,
    #[serde(default)]
    adoption: AdoptionConfig,
    #[serde(default)]
    languages: BTreeMap<LanguageId, LanguageEntryRaw>,
    #[serde(default)]
    design_systems: BTreeMap<String, DesignSystemConfig>,
}

#[derive(Debug, Deserialize)]
struct LanguageEntryRaw {
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    registry: Option<RegistryRaw>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RegistryRaw {
    Source(String),
    Object {
        source: String,
        #[serde(default)]
        upstream: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct WaxRcVersion {
    schema_version: u32,
}

/// Errors returned while loading wax config.
#[derive(Debug, Error)]
pub enum WaxRcError {
    /// The file could not be read from disk.
    #[error("failed to read wax config from {path}: {source}")]
    Read {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not syntactically valid JSON.
    #[error("malformed wax config JSON in {path}: {source}")]
    MalformedJson {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Underlying JSON syntax error.
        #[source]
        source: serde_json::Error,
    },
    /// The JSON is valid but does not match the supported wax config shape.
    #[error("invalid wax config in {path}: {source}")]
    InvalidConfig {
        /// Path passed to [`load_waxrc`].
        path: String,
        /// Underlying config decoding error.
        #[source]
        source: serde_json::Error,
    },
    /// The file uses a schema version this engine does not understand.
    #[error(
        "unsupported wax config schema_version {found} in {path}; this engine supports {supported}"
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

/// Loads and validates a wax config JSON file from disk.
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

    let raw: WaxRcRaw =
        serde_json::from_value(value).map_err(|source| WaxRcError::InvalidConfig {
            path: path_display.clone(),
            source,
        })?;

    let languages = raw
        .languages
        .into_iter()
        .map(|(id, entry)| entry.into_language_entry(id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| WaxRcError::InvalidConfig {
            path: path_display.clone(),
            source,
        })?;

    let rc = WaxRc {
        schema_version: raw.schema_version,
        engine: raw.engine,
        adoption: raw.adoption,
        languages,
        design_systems: raw.design_systems,
    };

    for language in &rc.languages {
        language
            .validate_registry_source_config()
            .map_err(|source| WaxRcError::InvalidConfig {
                path: path_display.clone(),
                source,
            })?;
    }
    rc.adoption
        .validate_supported()
        .map_err(|source| WaxRcError::InvalidConfig {
            path: path_display.clone(),
            source,
        })?;

    Ok(rc)
}

impl LanguageEntryRaw {
    fn into_language_entry(self, id: LanguageId) -> Result<LanguageEntry, serde_json::Error> {
        if self.extra.contains_key("design_system_registry") {
            return Err(serde_json::Error::custom(
                "languages.*.design_system_registry is not supported; use registry instead",
            ));
        }
        if self.extra.contains_key("enabled") {
            return Err(serde_json::Error::custom(
                "languages.*.enabled is not supported; omit disabled languages instead",
            ));
        }
        if self.extra.contains_key("id") {
            return Err(serde_json::Error::custom(
                "languages.*.id is not supported; language id is the languages map key",
            ));
        }

        let registry_source = self.registry.map(|registry| match registry {
            RegistryRaw::Source(source) => LanguageRegistrySource {
                source,
                upstream: None,
            },
            RegistryRaw::Object { source, upstream } => LanguageRegistrySource { source, upstream },
        });

        Ok(LanguageEntry {
            id,
            roots: self.roots,
            registry_source,
            extra: self.extra,
        })
    }
}

impl LanguageEntry {
    fn validate_registry_source_config(&self) -> Result<(), serde_json::Error> {
        let Some(registry) = &self.registry_source else {
            return Ok(());
        };

        if registry.source.trim().is_empty() {
            return Err(serde_json::Error::custom(
                "languages.*.registry.source must be a non-empty string",
            ));
        }

        if let Some(upstream) = &registry.upstream
            && upstream.trim().is_empty()
        {
            return Err(serde_json::Error::custom(
                "languages.*.registry.upstream must be a non-empty string when set",
            ));
        }

        Ok(())
    }
}

fn default_scan_concurrency() -> u32 {
    2
}

fn default_true() -> bool {
    true
}

fn default_scope_visibility() -> Vec<String> {
    vec![
        "public".to_owned(),
        "internal".to_owned(),
        "private".to_owned(),
    ]
}
