//! Stable data contract exchanged between wax language packs and the engine.
//!
//! Prefer [`scan_facts_from_json`] when ingesting pack output from the wire. Raw
//! serde deserialization only checks JSON shape and Rust integer widths; the
//! ingest helper also rejects contract-invalid values and stale derived counts.
//! In-process producers can call [`ScanFacts::validate`] before returning facts.

#![deny(missing_docs)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use time::OffsetDateTime;

/// Current JSON schema version for [`ScanFacts`].
pub const SCHEMA_VERSION: u32 = 1;

/// Maximum parser/extraction duration accepted by the frozen JSON contract.
///
/// Although [`Metrics::parse_extract_ms`] is represented as `u64`, the JSON
/// schema uses this practical bound so runtime validation and schema validation
/// agree without relying on large JSON number precision behavior.
pub const MAX_PARSE_EXTRACT_MS: u64 = u32::MAX as u64;

const NULLABLE_JSON_FIELDS: &[&[&str]] = &[&["metrics", "adoption_coverage_ratio"]];

/// Validated lowercase ASCII slug used to identify a language pack.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageId(String);

impl LanguageId {
    /// Returns the language id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the id and returns the validated string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for LanguageId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for LanguageId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for LanguageId {
    type Err = LanguageIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_language_id(value)?;
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<&str> for LanguageId {
    type Error = LanguageIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl TryFrom<String> for LanguageId {
    type Error = LanguageIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_language_id(&value)?;
        Ok(Self(value))
    }
}

impl Serialize for LanguageId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

/// Error returned when a language id is not a lowercase ASCII slug.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid language id {value:?}; expected lowercase ASCII slug [a-z][a-z0-9-]*")]
pub struct LanguageIdError {
    /// Invalid language id value.
    pub value: String,
}

fn validate_language_id(value: &str) -> Result<(), LanguageIdError> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(LanguageIdError {
            value: value.to_owned(),
        });
    };

    if !first.is_ascii_lowercase() {
        return Err(LanguageIdError {
            value: value.to_owned(),
        });
    }

    if chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
        Ok(())
    } else {
        Err(LanguageIdError {
            value: value.to_owned(),
        })
    }
}

/// Public errors returned by scan facts parsing and validation helpers.
#[derive(Debug, Error)]
pub enum ScanFactsError {
    /// JSON could not be deserialized into the scan facts contract.
    #[error("invalid ScanFacts JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// `schema_version` did not match [`SCHEMA_VERSION`].
    #[error("unsupported ScanFacts schema_version {found}; engine supports {supported}")]
    UnsupportedSchemaVersion {
        /// Schema version found in the input facts.
        found: u32,
        /// Schema version supported by this crate.
        supported: u32,
    },
    /// The facts deserialized but violated the frozen scan facts contract.
    #[error("invalid ScanFacts contract at {field}: {message}")]
    ContractViolation {
        /// Dotted field path for the violated contract field.
        field: String,
        /// Human-readable validation message.
        message: String,
    },
}

/// Language pack metadata embedded in scan facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LanguageMetadata {
    /// Stable language id used in configuration, manifests, and scan output.
    pub id: LanguageId,
    /// Language pack release version.
    pub version: String,
    /// Ecosystem or stack scanned by this language pack.
    pub ecosystem: String,
    /// Parser implementation name used by the language pack.
    pub parser_name: String,
    /// Parser implementation version used by the language pack.
    pub parser_version: String,
}

/// Source location shared by facts and diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceLocation {
    /// Repository-relative source file path.
    pub file: String,
    /// One-based source line number.
    pub line: u32,
    /// One-based source column number, when the pack can provide it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// Overall status for a language pack scan.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    /// The language pack completed the scan.
    Complete,
    /// The language pack returned usable facts with recoverable gaps.
    Partial,
    /// The language pack could not complete the scan.
    Failed,
}

/// Resolution status for a component usage site.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// The usage is resolved to a known design-system component.
    Resolved,
    /// The usage is a possible design-system component match.
    Candidate,
    /// The usage could not be resolved to the design-system registry.
    Unresolved,
}

/// Diagnostic severity emitted by a language pack.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    /// Scan error that may make facts incomplete or unusable.
    Error,
    /// Recoverable scan warning.
    Warning,
    /// Informational diagnostic.
    Info,
}

/// Normalized facts emitted by one language pack scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScanFacts {
    /// JSON schema version for this facts object.
    pub schema_version: u32,
    /// Metadata for the language pack that emitted these facts.
    pub language: LanguageMetadata,
    /// Engine-assigned snapshot id echoed by the language pack.
    pub snapshot_id: String,
    /// Time the scan facts were recorded.
    #[serde(with = "time::serde::rfc3339")]
    pub scanned_at: OffsetDateTime,
    /// Overall scan status.
    pub status: ScanStatus,
    /// Known design-system components loaded for this language.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Local components discovered in the repository.
    pub local_components: Vec<LocalComponent>,
    /// Component usage sites discovered in source.
    pub usage_sites: Vec<UsageSite>,
    /// Diagnostics emitted while scanning.
    pub diagnostics: Vec<Diagnostic>,
    /// Metrics derived or emitted for this scan.
    pub metrics: Metrics,
    /// Count summary derived from facts.
    pub counts: CountSummary,
}

/// Design-system component known to the language pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesignSystemComponent {
    /// Stable component id.
    pub id: String,
    /// Source symbol used by the language ecosystem.
    pub symbol: String,
    /// Canonical registry symbol for the design-system component.
    pub registry_symbol: String,
}

/// Repository-local component discovered by a language pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalComponent {
    /// Stable local component id.
    pub id: String,
    /// Source symbol used by the local component.
    pub symbol: String,
    /// Source location where the component is declared.
    pub location: SourceLocation,
}

/// Source usage site discovered by a language pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsageSite {
    /// Stable usage site id.
    pub id: String,
    /// Source location where the usage appears.
    pub location: SourceLocation,
    /// Source symbol used at this site.
    pub symbol: String,
    /// Resolution status against the design-system registry.
    pub match_status: MatchStatus,
    /// Registry symbol for resolved and candidate usage; absent for unresolved usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_symbol: Option<String>,
}

/// Diagnostic emitted by a language pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Source location related to the diagnostic, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
}

/// Metrics associated with one language scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    /// Resolved usage sites divided by all usage sites, or `None` when there are no usage sites.
    pub adoption_coverage_ratio: Option<f64>,
    /// Parser and extraction elapsed time in milliseconds.
    pub parse_extract_ms: u64,
    /// Number of files scanned by the language pack.
    pub files_scanned: u32,
}

/// Count summary derived from scan facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CountSummary {
    /// Number of design-system components.
    pub design_system_component_count: u32,
    /// Number of local components.
    pub local_component_count: u32,
    /// Number of discovered usage sites.
    pub usage_site_count: u32,
    /// Number of resolved usage sites.
    pub resolved_count: u32,
    /// Number of candidate usage sites.
    pub candidate_count: u32,
}

/// Merged scan facts keyed by language id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MergedScan {
    /// JSON schema version for the merged scan.
    pub schema_version: u32,
    /// Time the merged scan was recorded.
    #[serde(with = "time::serde::rfc3339")]
    pub recorded_at: OffsetDateTime,
    /// Per-language scan facts.
    pub languages: BTreeMap<LanguageId, ScanFacts>,
}

/// Validates a `ScanFacts.schema_version` value.
pub fn validate_schema_version(version: u32) -> Result<(), ScanFactsError> {
    if version != SCHEMA_VERSION {
        Err(ScanFactsError::UnsupportedSchemaVersion {
            found: version,
            supported: SCHEMA_VERSION,
        })
    } else {
        Ok(())
    }
}

/// Deserializes scan facts from JSON and validates the frozen contract.
///
/// Use this helper for pack output instead of raw serde deserialization. It
/// enforces schema version, unknown-field rejection, non-empty strings,
/// source-location bounds, usage-site registry-symbol invariants, and derived
/// count/metric consistency.
pub fn scan_facts_from_json(json: &str) -> Result<ScanFacts, ScanFactsError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    require_json_field(&value, &["metrics", "adoption_coverage_ratio"])?;
    reject_disallowed_nulls(&value, &[])?;
    let facts: ScanFacts = serde_json::from_value(value)?;
    validate_schema_version(facts.schema_version)?;
    facts.validate()?;
    Ok(facts)
}

impl ScanFacts {
    /// Validates non-derived fields and verifies counts/metrics match the facts.
    ///
    /// This is the in-memory equivalent of the validation performed by
    /// [`scan_facts_from_json`] after deserialization.
    pub fn validate(&self) -> Result<(), ScanFactsError> {
        validate_schema_version(self.schema_version)?;
        require_non_empty("language.version", &self.language.version)?;
        require_non_empty("language.ecosystem", &self.language.ecosystem)?;
        require_non_empty("language.parser_name", &self.language.parser_name)?;
        require_non_empty("language.parser_version", &self.language.parser_version)?;
        require_non_empty("snapshot_id", &self.snapshot_id)?;

        for (index, component) in self.design_system_components.iter().enumerate() {
            let field = format!("design_system_components[{index}]");
            require_non_empty(&format!("{field}.id"), &component.id)?;
            require_non_empty(&format!("{field}.symbol"), &component.symbol)?;
            require_non_empty(
                &format!("{field}.registry_symbol"),
                &component.registry_symbol,
            )?;
        }

        for (index, component) in self.local_components.iter().enumerate() {
            let field = format!("local_components[{index}]");
            require_non_empty(&format!("{field}.id"), &component.id)?;
            require_non_empty(&format!("{field}.symbol"), &component.symbol)?;
            validate_location(&format!("{field}.location"), &component.location)?;
        }

        for (index, site) in self.usage_sites.iter().enumerate() {
            let field = format!("usage_sites[{index}]");
            require_non_empty(&format!("{field}.id"), &site.id)?;
            validate_location(&format!("{field}.location"), &site.location)?;
            require_non_empty(&format!("{field}.symbol"), &site.symbol)?;
            validate_usage_site_registry_symbol(&field, site)?;
        }

        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            let field = format!("diagnostics[{index}]");
            require_non_empty(&format!("{field}.code"), &diagnostic.code)?;
            if let Some(location) = &diagnostic.location {
                validate_location(&format!("{field}.location"), location)?;
            }
        }

        validate_derived_values(self)
    }

    /// Recomputes counts and adoption coverage from fact collections.
    pub fn recompute_counts(&mut self) -> Result<(), ScanFactsError> {
        let (counts, ratio) = derive_counts_and_ratio(self)?;

        self.metrics.adoption_coverage_ratio = ratio;
        self.counts = counts;
        Ok(())
    }
}

fn require_json_field(value: &serde_json::Value, path: &[&str]) -> Result<(), ScanFactsError> {
    let mut current = value;
    for (index, segment) in path.iter().enumerate() {
        current = current
            .get(*segment)
            .ok_or_else(|| contract_violation(&path[..=index].join("."), "field is required"))?;
    }
    Ok(())
}

fn reject_disallowed_nulls(
    value: &serde_json::Value,
    path: &[String],
) -> Result<(), ScanFactsError> {
    match value {
        serde_json::Value::Null if is_nullable_json_field(path) => Ok(()),
        serde_json::Value::Null => Err(contract_violation(
            &json_path(path),
            "explicit null is not allowed by the scan facts schema",
        )),
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let mut child_path = path.to_vec();
                child_path.push(index.to_string());
                reject_disallowed_nulls(item, &child_path)?;
            }
            Ok(())
        }
        serde_json::Value::Object(entries) => {
            for (key, child) in entries {
                let mut child_path = path.to_vec();
                child_path.push(key.clone());
                reject_disallowed_nulls(child, &child_path)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn is_nullable_json_field(path: &[String]) -> bool {
    NULLABLE_JSON_FIELDS
        .iter()
        .any(|allowed| path.iter().map(String::as_str).eq(allowed.iter().copied()))
}

fn json_path(path: &[String]) -> String {
    if path.is_empty() {
        return "$".to_owned();
    }

    let mut out = String::from("$");
    for segment in path {
        if segment.parse::<usize>().is_ok() {
            out.push('[');
            out.push_str(segment);
            out.push(']');
        } else {
            out.push('.');
            out.push_str(segment);
        }
    }
    out
}

fn validate_location(field: &str, location: &SourceLocation) -> Result<(), ScanFactsError> {
    require_non_empty(&format!("{field}.file"), &location.file)?;

    if location.line == 0 {
        return Err(contract_violation(
            &format!("{field}.line"),
            "line must be one-based",
        ));
    }

    if location.column == Some(0) {
        return Err(contract_violation(
            &format!("{field}.column"),
            "column must be one-based when present",
        ));
    }

    Ok(())
}

fn validate_usage_site_registry_symbol(
    field: &str,
    site: &UsageSite,
) -> Result<(), ScanFactsError> {
    match (site.match_status, &site.registry_symbol) {
        (MatchStatus::Resolved | MatchStatus::Candidate, Some(registry_symbol)) => {
            require_non_empty(&format!("{field}.registry_symbol"), registry_symbol)
        }
        (MatchStatus::Resolved | MatchStatus::Candidate, None) => Err(contract_violation(
            &format!("{field}.registry_symbol"),
            "registry_symbol is required for resolved and candidate usage",
        )),
        (MatchStatus::Unresolved, Some(_)) => Err(contract_violation(
            &format!("{field}.registry_symbol"),
            "registry_symbol must be absent for unresolved usage",
        )),
        (MatchStatus::Unresolved, None) => Ok(()),
    }
}

fn validate_derived_values(facts: &ScanFacts) -> Result<(), ScanFactsError> {
    if facts.metrics.parse_extract_ms > MAX_PARSE_EXTRACT_MS {
        return Err(contract_violation(
            "metrics.parse_extract_ms",
            "parse_extract_ms exceeds the JSON contract maximum",
        ));
    }

    let (expected_counts, expected_ratio) = derive_counts_and_ratio(facts)?;

    if facts.counts != expected_counts {
        return Err(contract_violation(
            "counts",
            "count summary must match the emitted facts",
        ));
    }

    match (facts.metrics.adoption_coverage_ratio, expected_ratio) {
        (None, None) => Ok(()),
        (Some(actual), _) if !actual.is_finite() => Err(contract_violation(
            "metrics.adoption_coverage_ratio",
            "adoption coverage ratio must be finite",
        )),
        (Some(actual), _) if !(0.0..=1.0).contains(&actual) => Err(contract_violation(
            "metrics.adoption_coverage_ratio",
            "adoption coverage ratio must be between 0 and 1",
        )),
        (Some(actual), Some(expected)) if (actual - expected).abs() <= 1e-12 => Ok(()),
        (Some(_), None) => Err(contract_violation(
            "metrics.adoption_coverage_ratio",
            "adoption coverage ratio must be null when usage_site_count is zero",
        )),
        (None, Some(_)) => Err(contract_violation(
            "metrics.adoption_coverage_ratio",
            "adoption coverage ratio is required when usage sites are present",
        )),
        (Some(_), Some(_)) => Err(contract_violation(
            "metrics.adoption_coverage_ratio",
            "adoption coverage ratio must equal resolved_count / usage_site_count",
        )),
    }
}

fn derive_counts_and_ratio(
    facts: &ScanFacts,
) -> Result<(CountSummary, Option<f64>), ScanFactsError> {
    let mut resolved_count = 0_u32;
    let mut candidate_count = 0_u32;

    for site in &facts.usage_sites {
        match site.match_status {
            MatchStatus::Resolved => increment_count("counts.resolved_count", &mut resolved_count)?,
            MatchStatus::Candidate => {
                increment_count("counts.candidate_count", &mut candidate_count)?;
            }
            MatchStatus::Unresolved => {}
        }
    }

    let usage_site_count = checked_len("counts.usage_site_count", facts.usage_sites.len())?;
    let counts = CountSummary {
        design_system_component_count: checked_len(
            "counts.design_system_component_count",
            facts.design_system_components.len(),
        )?,
        local_component_count: checked_len(
            "counts.local_component_count",
            facts.local_components.len(),
        )?,
        usage_site_count,
        resolved_count,
        candidate_count,
    };
    let ratio = if usage_site_count == 0 {
        None
    } else {
        Some(f64::from(resolved_count) / f64::from(usage_site_count))
    };

    Ok((counts, ratio))
}

fn checked_len(field: &str, len: usize) -> Result<u32, ScanFactsError> {
    u32::try_from(len).map_err(|_| contract_violation(field, "count exceeds u32 maximum"))
}

fn increment_count(field: &str, count: &mut u32) -> Result<(), ScanFactsError> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| contract_violation(field, "count exceeds u32 maximum"))?;
    Ok(())
}

fn require_non_empty(field: &str, value: &str) -> Result<(), ScanFactsError> {
    if value.is_empty() {
        Err(contract_violation(field, "value must not be empty"))
    } else {
        Ok(())
    }
}

fn contract_violation(field: &str, message: &str) -> ScanFactsError {
    ScanFactsError::ContractViolation {
        field: field.to_owned(),
        message: message.to_owned(),
    }
}
