//! Stable data contract exchanged between wax language packs and the engine.
//!
//! Prefer [`scan_facts_from_json`] when ingesting pack output from the wire. Raw
//! serde deserialization only checks JSON shape and Rust integer widths; the
//! ingest helper also rejects contract-invalid values and stale derived counts.
//! In-process producers can call [`ScanFacts::validate`] before returning facts.
//!
//! # Examples
//!
//! ```
//! use wax_contract::LanguageId;
//!
//! let language = LanguageId::try_from("swift")?;
//! assert_eq!(language.as_str(), "swift");
//! assert!(LanguageId::try_from("Swift UI").is_err());
//! # Ok::<(), wax_contract::LanguageIdError>(())
//! ```

#![deny(missing_docs)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use time::OffsetDateTime;

/// Current JSON schema version for [`ScanFacts`] and [`MergedScan`].
pub const SCHEMA_VERSION: u32 = 3;

/// Maximum parser/extraction duration accepted by the frozen JSON contract.
///
/// Although [`Metrics::parse_extract_ms`] is represented as `u64`, the JSON
/// schema uses this practical bound so runtime validation and schema validation
/// agree without relying on large JSON number precision behavior.
pub const MAX_PARSE_EXTRACT_MS: u64 = u32::MAX as u64;

const NULLABLE_JSON_FIELDS: &[&[&str]] = &[
    &["metrics", "invocation_adoption_ratio"],
    &["metrics", "registry_resolution_ratio"],
];

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

/// Resolution status for a UI invocation usage site.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// The invocation resolved to a configured design-system registry component.
    Resolved,
    /// The invocation may refer to a design-system component but needs review.
    Candidate,
    /// The invocation resolved to a UI definition declared in the scanned repository.
    Local,
    /// The invocation has UI shape but could not match registry or local definitions.
    Unresolved,
}

/// Kind of symbol represented by a [`SymbolUsageSummary`] row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A grouped design-system registry symbol with resolved invocations.
    Registry,
    /// A grouped in-repository UI symbol with local invocations.
    Local,
    /// A grouped symbol with candidate design-system invocations.
    Candidate,
    /// A grouped UI-shaped symbol that remains unmatched.
    Unresolved,
}

/// Expected durability of an identity for trend consumers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStability {
    /// Based on package/module plus declaration; file moves usually preserve it.
    Semantic,
    /// Based partly on file/module path; moves may cause trend churn.
    PathSensitive,
    /// Stable only inside one scan result; not a long-term trend key.
    ScanLocal,
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

/// Parent scope attribution for a usage site.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParentScope {
    /// Grouping key for the parent scope; prefer semantic identity over file path.
    pub parent_id: String,
    /// Source-level parent symbol name.
    pub symbol: String,
    /// Best-effort semantic parent identity when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_symbol: Option<String>,
    /// Language-defined parent category such as `composable`, `view`, or `component`.
    pub scope_kind: String,
    /// Human-readable explanation of how the parent id was built.
    pub identity_basis: String,
    /// Expected durability of the parent identity for trend consumers.
    pub identity_stability: IdentityStability,
    /// Source location of the parent declaration when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
}

/// Per-parent invocation counts inside a [`SymbolUsageSummary`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolParentScopeSummary {
    /// Grouping key for the parent scope.
    pub parent_id: String,
    /// Source-level parent symbol name.
    pub symbol: String,
    /// Best-effort semantic parent identity when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_symbol: Option<String>,
    /// Language-defined parent category.
    pub scope_kind: String,
    /// Human-readable explanation of how the parent id was built.
    pub identity_basis: String,
    /// Expected durability of the parent identity.
    pub identity_stability: IdentityStability,
    /// Number of invocations attributed to this parent scope.
    pub invocation_count: u32,
    /// Source location of the parent declaration when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
}

/// Derived per-callee summary grouped from usage sites.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolUsageSummary {
    /// Normalized callee grouping key for this summary row.
    pub symbol_id: String,
    /// Source-level callee symbol name.
    pub symbol: String,
    /// Best-effort semantic callee identity when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_symbol: Option<String>,
    /// What this summary row represents after grouping usage sites.
    pub symbol_kind: SymbolKind,
    /// Match status shared by invocations in this summary row.
    pub match_status: MatchStatus,
    /// Canonical registry symbol for registry rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_symbol: Option<String>,
    /// Local definition id for local rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_definition_id: Option<String>,
    /// Human-readable explanation of how the symbol id was built.
    pub identity_basis: String,
    /// Expected durability of the symbol identity.
    pub identity_stability: IdentityStability,
    /// Number of usage sites represented by this row.
    pub raw_invocation_count: u32,
    /// Number of unique parent scopes, regardless of row limit.
    pub parent_scope_count: u32,
    /// Number of files containing invocations for this symbol.
    pub file_count: u32,
    /// Complete or limited parent-scope rows for this symbol.
    pub parent_scopes: Vec<SymbolParentScopeSummary>,
    /// Limit applied when emitting parent rows: `null` emits all, `0` emits none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_scope_limit: Option<u32>,
    /// Whether `parent_scopes` omits rows because a limit was applied.
    pub parent_scopes_truncated: bool,
}

/// Design token category used for token references and hard-coded styling candidates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TokenCategory {
    /// Color token or hard-coded color candidate.
    Color,
    /// Spacing, sizing, padding, margin, or gap token or candidate.
    Spacing,
    /// Typography token or hard-coded typography candidate.
    Typography,
    /// Radius or shape token or candidate.
    Radius,
    /// Elevation, shadow, or z-depth token or candidate.
    Elevation,
    /// Known token whose category is not classified.
    Unknown,
}

/// Design-system token known to a language pack from its registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesignSystemToken {
    /// Stable token id within the language registry.
    pub id: String,
    /// Exact source-facing token key.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
    /// Exact source-facing aliases for the same token.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Optional canonical source-facing value for deterministic inference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Source reference to a known design token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TokenSite {
    /// Stable token reference site id.
    pub id: String,
    /// Source location where the token reference appears.
    pub location: SourceLocation,
    /// Referenced token id from `design_system_tokens`.
    pub token_id: String,
    /// Exact key or alias matched in source.
    pub key: String,
    /// Token category copied from the matched token.
    pub category: TokenCategory,
    /// Parent scope attribution when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentScope>,
}

/// Typed usage context for a hard-coded styling observation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StyleContext {
    /// Padding / inset styling.
    Padding,
    /// Margin styling.
    Margin,
    /// Gap / spacing-between styling.
    Gap,
    /// Explicit width.
    Width,
    /// Explicit height.
    Height,
    /// Combined size (width and height together).
    Size,
    /// Corner radius.
    Radius,
    /// Color styling.
    Color,
    /// Typography styling.
    Typography,
    /// Elevation / shadow styling.
    Elevation,
    /// Recognized styling literal without a more precise context.
    Unknown,
}

/// Hard-coded styling literal detected in a styling context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HardcodedStyleSite {
    /// Stable hard-coded style site id.
    pub id: String,
    /// Source location where the literal appears.
    pub location: SourceLocation,
    /// Source text for the hard-coded styling value.
    pub value: String,
    /// Styling category inferred from context.
    pub category: TokenCategory,
    /// Precise usage context for the observation.
    pub context: StyleContext,
    /// Parent scope attribution when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentScope>,
}

/// Derived per-token usage summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TokenUsageSummary {
    /// Language pack that owns this token summary row.
    ///
    /// Token ids are stable only within a language registry, so merged rows must
    /// carry language identity to remain unambiguous across packs.
    pub language: String,
    /// Token id represented by this summary row.
    pub token_id: String,
    /// Exact registry key for the token.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
    /// Number of token references to this token.
    pub reference_count: u32,
    /// Number of files containing references to this token.
    pub file_count: u32,
    /// Number of parent scopes containing references to this token.
    pub parent_scope_count: u32,
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
    /// UI invocation usage sites discovered in source.
    pub usage_sites: Vec<UsageSite>,
    /// Diagnostics emitted while scanning.
    pub diagnostics: Vec<Diagnostic>,
    /// Metrics derived or emitted for this scan.
    pub metrics: Metrics,
    /// Count summary derived from facts.
    pub counts: CountSummary,
    /// Derived per-callee summaries when emitted by the engine or pack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_usage_summary: Vec<SymbolUsageSummary>,
    /// Known design-system tokens loaded for this language.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub design_system_tokens: Vec<DesignSystemToken>,
    /// Known token references discovered in source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_sites: Vec<TokenSite>,
    /// Hard-coded styling candidates discovered in source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hardcoded_style_sites: Vec<HardcodedStyleSite>,
    /// Derived per-token summaries when emitted by the engine or pack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_usage_summary: Vec<TokenUsageSummary>,
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
    /// Best-effort semantic identity when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_symbol: Option<String>,
    /// Human-readable explanation of how the local id was built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_basis: Option<String>,
    /// Expected durability of the local identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_stability: Option<IdentityStability>,
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
    /// Best-effort semantic callee identity when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_symbol: Option<String>,
    /// Resolution status against registry and local definitions.
    pub match_status: MatchStatus,
    /// Registry symbol for resolved and candidate usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_symbol: Option<String>,
    /// Local definition id for local usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_definition_id: Option<String>,
    /// Parent scope attribution when enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentScope>,
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
    /// Resolved invocations divided by adoption-eligible invocations, or `None` when eligible is zero.
    pub invocation_adoption_ratio: Option<f64>,
    /// Resolved invocations divided by all raw invocations, or `None` when total is zero.
    pub registry_resolution_ratio: Option<f64>,
    /// Parser and extraction elapsed time in milliseconds.
    pub parse_extract_ms: u64,
    /// Number of files scanned by the language pack.
    pub files_scanned: u32,
}

/// Registry-related raw counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RegistryCounts {
    /// Number of configured design-system registry components.
    pub component_count: u32,
    /// Number of distinct registry components with at least one resolved invocation.
    pub used_component_count: u32,
    /// Raw count of resolved design-system invocations.
    pub resolved_raw_invocation_count: u32,
    /// Raw count of candidate design-system invocations.
    pub candidate_raw_invocation_count: u32,
}

/// Local definition inventory counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct DefinitionCounts {
    /// Number of local UI definitions discovered in source.
    pub local_definition_count: u32,
    /// Number of local definitions with at least one local invocation.
    pub invoked_local_definition_count: u32,
    /// Number of local definitions with no local invocations.
    pub unused_local_definition_count: u32,
}

/// Raw invocation counters grouped by match status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawInvocationCounts {
    /// Count of all detected UI invocations across statuses.
    pub total: u32,
    /// Count of invocations with `match_status: "resolved"`.
    pub resolved: u32,
    /// Count of invocations with `match_status: "local"`.
    pub local: u32,
    /// Count of invocations with `match_status: "candidate"`.
    pub candidate: u32,
    /// Count of invocations with `match_status: "unresolved"`.
    pub unresolved: u32,
}

/// Adoption-eligibility counters after candidate policy is applied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct AdoptionCounts {
    /// Denominator for primary invocation adoption after candidate policy.
    pub eligible_invocation_count: u32,
    /// Numerator for primary invocation adoption; resolved invocations by default.
    pub adopted_invocation_count: u32,
    /// Adoption-eligible invocations that are not counted as adopted.
    pub non_adopted_invocation_count: u32,
}

/// Parent-scope aggregate counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ParentScopeCounts {
    /// Number of unique parent scopes found in attributed usage sites.
    pub total: u32,
    /// Number of parent scopes containing at least one resolved invocation.
    pub with_resolved_invocations: u32,
    /// Number of parent scopes containing at least one local invocation.
    pub with_local_invocations: u32,
    /// Number of parent scopes containing at least one unresolved invocation.
    pub with_unresolved_invocations: u32,
}

/// Token counts grouped by category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenCategoryCounts {
    /// Color token references or candidates.
    pub color: u32,
    /// Spacing token references or candidates.
    pub spacing: u32,
    /// Typography token references or candidates.
    pub typography: u32,
    /// Radius token references or candidates.
    pub radius: u32,
    /// Elevation token references or candidates.
    pub elevation: u32,
    /// Unknown-category token references or candidates.
    pub unknown: u32,
}

/// Design-token scan counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenCounts {
    /// Number of configured design-system tokens.
    pub configured_token_count: u32,
    /// Number of distinct configured tokens with at least one reference.
    pub used_token_count: u32,
    /// Number of known token reference sites.
    pub token_reference_site_count: u32,
    /// Number of hard-coded styling candidates.
    pub hardcoded_style_candidate_count: u32,
    /// Token reference counts grouped by category.
    pub token_references_by_category: TokenCategoryCounts,
    /// Hard-coded styling candidate counts grouped by category.
    pub hardcoded_by_category: TokenCategoryCounts,
    /// Number of parent scopes containing at least one token reference.
    pub parent_scopes_with_token_references: u32,
    /// Number of parent scopes containing at least one hard-coded styling candidate.
    pub parent_scopes_with_hardcoded_candidates: u32,
}

/// Count summary derived from scan facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CountSummary {
    /// Registry-related raw counters.
    pub registry: RegistryCounts,
    /// Local definition inventory counters.
    pub definitions: DefinitionCounts,
    /// Raw invocation counters grouped by match status.
    pub raw_invocations: RawInvocationCounts,
    /// Adoption-eligibility counters.
    pub adoption: AdoptionCounts,
    /// Parent-scope aggregate counters.
    pub parent_scopes: ParentScopeCounts,
    /// Design-token scan counters.
    #[serde(default)]
    pub tokens: TokenCounts,
}

/// Repo-level summary on a merged scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoSummary {
    /// Language ids included in the merged scan.
    pub languages: Vec<LanguageId>,
    /// Summed raw counters across languages.
    pub counts: CountSummary,
    /// Ratios recomputed from repo-level counters.
    pub metrics: Metrics,
}

/// Classification produced by deterministic token inference.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenInferenceClassification {
    /// Observed value exactly matches one or more canonical token values.
    Exact,
    /// Observed numeric value is within tolerance of one or more canonical values.
    Near,
    /// Observation was assessed and matched no token.
    Unmatched,
    /// Observation could not be assessed from available registry metadata.
    Unassessed,
}

/// Confidence attached to exact and near replacement suggestions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TokenInferenceConfidence {
    /// Weakest suggestion confidence.
    Low,
    /// Moderate suggestion confidence.
    Medium,
    /// Strong suggestion confidence.
    High,
    /// Strongest suggestion confidence.
    VeryHigh,
}

/// Whether a suggestion came from an exact or near value match.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenMatchKind {
    /// Exact normalized value match.
    Exact,
    /// Near numeric match within tolerance.
    Near,
}

/// Typed evidence explaining an inference row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TokenInferenceEvidence {
    /// Observed value exactly matched a canonical value.
    ExactValue,
    /// Observed numeric value fell within configured tolerance.
    WithinNumericTolerance,
    /// Usage context is a clear token-oriented property.
    ClearUsageContext,
    /// Usage context is a generic dimension such as width/height/size/unknown.
    GenericDimensionContext,
    /// Multiple equally good suggestions were retained.
    MultipleEqualMatches,
    /// Registry tokens lack canonical values needed for assessment.
    MissingCanonicalValues,
    /// Some same-category tokens lack usable canonical values.
    IncompleteCanonicalCoverage,
    /// A canonical value could not be normalized for comparison.
    UnsupportedCanonicalFormat,
    /// Observed and canonical units are incompatible.
    IncompatibleUnits,
    /// Numeric distance exceeded the configured tolerance.
    OutsideNumericTolerance,
}

/// One ranked token replacement suggestion for a hard-coded observation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TokenReplacementSuggestion {
    /// Suggested token id from the same language registry.
    pub token_id: String,
    /// Suggested token key from the same language registry.
    pub token_key: String,
    /// Canonical registry value used for the match.
    pub canonical_value: String,
    /// Whether the suggestion is an exact or near match.
    pub match_kind: TokenMatchKind,
    /// Absolute numeric distance when meaningful; exact nonnumeric matches omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Normalized unit for numeric suggestions when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_unit: Option<String>,
}

/// Core-owned inference conclusion for one raw hard-coded style site.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HardcodedStyleInference {
    /// Language that owns the raw hard-coded site.
    pub language: LanguageId,
    /// Stable raw-site id from `hardcoded_style_sites`.
    pub site_id: String,
    /// Exact, near, unmatched, or unassessed classification.
    pub classification: TokenInferenceClassification,
    /// Present only for exact and near rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<TokenInferenceConfidence>,
    /// Ranked replacement suggestions; empty for unmatched and unassessed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<TokenReplacementSuggestion>,
    /// Typed evidence explaining the classification and confidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<TokenInferenceEvidence>,
}

/// Exact/near candidate counts grouped by confidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenConfidenceCounts {
    /// Candidates with very high confidence.
    pub very_high: u32,
    /// Candidates with high confidence.
    pub high: u32,
    /// Candidates with medium confidence.
    pub medium: u32,
    /// Candidates with low confidence.
    pub low: u32,
}

/// Exact/near candidate counts grouped by style context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct StyleContextCounts {
    /// Padding candidates.
    pub padding: u32,
    /// Margin candidates.
    pub margin: u32,
    /// Gap candidates.
    pub gap: u32,
    /// Width candidates.
    pub width: u32,
    /// Height candidates.
    pub height: u32,
    /// Size candidates.
    pub size: u32,
    /// Radius candidates.
    pub radius: u32,
    /// Color candidates.
    pub color: u32,
    /// Typography candidates.
    pub typography: u32,
    /// Elevation candidates.
    pub elevation: u32,
    /// Unknown-context candidates.
    pub unknown: u32,
}

/// Summary counts for a token inference report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenInferenceCounts {
    /// Total raw hard-coded observations covered by inference rows.
    pub hardcoded_observation_count: u32,
    /// Exact + near + unmatched observations.
    pub assessed_observation_count: u32,
    /// Exact replacement candidates.
    pub exact_replacement_candidate_count: u32,
    /// Near replacement candidates.
    pub near_replacement_candidate_count: u32,
    /// Assessed observations with no replacement match.
    pub unmatched_observation_count: u32,
    /// Observations that could not be assessed.
    pub unassessed_observation_count: u32,
    /// Exact/near candidates grouped by confidence.
    pub candidates_by_confidence: TokenConfidenceCounts,
    /// Exact/near candidates grouped by token category.
    pub candidates_by_category: TokenCategoryCounts,
    /// Exact/near candidates grouped by style context.
    pub candidates_by_context: StyleContextCounts,
}

/// Core-owned token inference facts for a merged scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TokenInferenceReport {
    /// Applied numeric near-match tolerance.
    pub numeric_tolerance: f64,
    /// Counts reconciled from [`Self::sites`].
    pub counts: TokenInferenceCounts,
    /// One inference row per raw hard-coded site across languages.
    pub sites: Vec<HardcodedStyleInference>,
}

impl TokenInferenceReport {
    /// Empty report used when no raw hard-coded sites exist.
    #[must_use]
    pub fn empty(numeric_tolerance: f64) -> Self {
        Self {
            numeric_tolerance,
            counts: TokenInferenceCounts::default(),
            sites: Vec::new(),
        }
    }
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
    /// Repo-level counts and metrics summed across languages.
    pub repo_summary: RepoSummary,
    /// Root-level per-callee summaries grouped across languages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_usage_summary: Vec<SymbolUsageSummary>,
    /// Derived per-token summaries across merged languages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_usage_summary: Vec<TokenUsageSummary>,
    /// Core-owned hard-coded style inference report.
    pub token_inference: TokenInferenceReport,
    /// Per-language scan facts.
    pub languages: BTreeMap<LanguageId, ScanFacts>,
}

/// Validates a `ScanFacts.schema_version` value.
///
/// # Errors
///
/// Returns [`ScanFactsError::UnsupportedSchemaVersion`] when `version` differs
/// from [`SCHEMA_VERSION`].
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
/// source-location bounds, usage-site linkage invariants, and derived
/// count/metric consistency.
///
/// # Errors
///
/// Returns [`ScanFactsError::InvalidJson`] for malformed JSON or JSON that does
/// not match the serialized contract, [`ScanFactsError::UnsupportedSchemaVersion`]
/// for an incompatible schema, or [`ScanFactsError::ContractViolation`] when a
/// semantic invariant or derived value is invalid.
pub fn scan_facts_from_json(json: &str) -> Result<ScanFacts, ScanFactsError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    require_json_field(&value, &["metrics", "invocation_adoption_ratio"])?;
    require_json_field(&value, &["metrics", "registry_resolution_ratio"])?;
    reject_disallowed_nulls(&value, &[])?;
    let facts: ScanFacts = serde_json::from_value(value)?;
    validate_schema_version(facts.schema_version)?;
    facts.validate()?;
    Ok(facts)
}

impl ScanFacts {
    /// Validates non-derived fields and verifies counts/metrics match the facts.
    ///
    /// # Errors
    ///
    /// Returns [`ScanFactsError::UnsupportedSchemaVersion`] for an incompatible
    /// schema or [`ScanFactsError::ContractViolation`] for invalid fields,
    /// linkage, locations, summaries, counts, or metrics.
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
            validate_usage_site_linkage(&field, site)?;
            if let Some(parent) = &site.parent {
                validate_parent_scope(&format!("{field}.parent"), parent)?;
            }
        }

        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            let field = format!("diagnostics[{index}]");
            require_non_empty(&format!("{field}.code"), &diagnostic.code)?;
            if let Some(location) = &diagnostic.location {
                validate_location(&format!("{field}.location"), location)?;
            }
        }

        for (index, summary) in self.symbol_usage_summary.iter().enumerate() {
            validate_symbol_usage_summary(&format!("symbol_usage_summary[{index}]"), summary)?;
        }

        for (index, token) in self.design_system_tokens.iter().enumerate() {
            let field = format!("design_system_tokens[{index}]");
            require_non_empty(&format!("{field}.id"), &token.id)?;
            require_non_empty(&format!("{field}.key"), &token.key)?;
            if let Some(value) = &token.value
                && value.is_empty()
            {
                return Err(contract_violation(
                    &format!("{field}.value"),
                    "value must not be empty when present",
                ));
            }
            for (alias_index, alias) in token.aliases.iter().enumerate() {
                if alias.is_empty() {
                    return Err(contract_violation(
                        &format!("{field}.aliases[{alias_index}]"),
                        "token alias must not be empty",
                    ));
                }
            }
        }

        for (index, site) in self.token_sites.iter().enumerate() {
            let field = format!("token_sites[{index}]");
            require_non_empty(&format!("{field}.id"), &site.id)?;
            validate_location(&format!("{field}.location"), &site.location)?;
            require_non_empty(&format!("{field}.token_id"), &site.token_id)?;
            require_non_empty(&format!("{field}.key"), &site.key)?;
            if let Some(parent) = &site.parent {
                validate_parent_scope(&format!("{field}.parent"), parent)?;
            }
        }

        for (index, site) in self.hardcoded_style_sites.iter().enumerate() {
            let field = format!("hardcoded_style_sites[{index}]");
            require_non_empty(&format!("{field}.id"), &site.id)?;
            validate_location(&format!("{field}.location"), &site.location)?;
            if site.value.is_empty() {
                return Err(contract_violation(
                    &format!("{field}.value"),
                    "value must not be empty",
                ));
            }
            if let Some(parent) = &site.parent {
                validate_parent_scope(&format!("{field}.parent"), parent)?;
            }
        }

        for (index, summary) in self.token_usage_summary.iter().enumerate() {
            validate_token_usage_summary(&format!("token_usage_summary[{index}]"), summary)?;
        }

        validate_derived_values(self)
    }

    /// Recomputes counts and adoption metrics from fact collections.
    ///
    /// # Errors
    ///
    /// Returns [`ScanFactsError::ContractViolation`] when a derived counter
    /// would overflow its contract width.
    pub fn recompute_counts(&mut self) -> Result<(), ScanFactsError> {
        let (counts, metrics) = derive_counts_and_metrics(self)?;
        self.counts = counts;
        self.metrics.invocation_adoption_ratio = metrics.0;
        self.metrics.registry_resolution_ratio = metrics.1;
        Ok(())
    }
}

impl MergedScan {
    /// Validates merged scan shape and repo-level derived values.
    ///
    /// # Errors
    ///
    /// Returns [`ScanFactsError::UnsupportedSchemaVersion`] for an incompatible
    /// schema or [`ScanFactsError::ContractViolation`] when language keys,
    /// summaries, counts, or repo-level metrics are inconsistent.
    pub fn validate(&self) -> Result<(), ScanFactsError> {
        validate_schema_version(self.schema_version)?;

        for (language_id, facts) in &self.languages {
            if facts.language.id != *language_id {
                return Err(contract_violation(
                    &format!("languages.{language_id}"),
                    "language metadata id must match the map key",
                ));
            }
            facts.validate()?;
        }

        for (index, summary) in self.symbol_usage_summary.iter().enumerate() {
            validate_symbol_usage_summary(&format!("symbol_usage_summary[{index}]"), summary)?;
        }

        for (index, summary) in self.token_usage_summary.iter().enumerate() {
            validate_token_usage_summary(&format!("token_usage_summary[{index}]"), summary)?;
        }

        validate_repo_summary(self)?;
        validate_token_inference(self)?;

        Ok(())
    }
}

fn validate_repo_summary(merged: &MergedScan) -> Result<(), ScanFactsError> {
    let expected_languages = merged.languages.keys().cloned().collect::<Vec<_>>();
    if merged.repo_summary.languages != expected_languages {
        return Err(contract_violation(
            "repo_summary.languages",
            "languages must match merged scan language keys in deterministic order",
        ));
    }

    let mut counts = CountSummary::default();
    let mut parse_extract_ms = 0_u64;
    let mut files_scanned = 0_u32;
    for facts in merged.languages.values() {
        counts = checked_add_count_summaries("repo_summary.counts", &counts, &facts.counts)?;
        parse_extract_ms = parse_extract_ms
            .checked_add(facts.metrics.parse_extract_ms)
            .ok_or_else(|| {
                contract_violation(
                    "repo_summary.metrics.parse_extract_ms",
                    "parse_extract_ms exceeds u64 maximum",
                )
            })?;
        files_scanned = files_scanned
            .checked_add(facts.metrics.files_scanned)
            .ok_or_else(|| {
                contract_violation(
                    "repo_summary.metrics.files_scanned",
                    "files_scanned exceeds u32 maximum",
                )
            })?;
    }

    if merged.repo_summary.counts != counts {
        return Err(contract_violation(
            "repo_summary.counts",
            "repo counts must equal the sum of language counts",
        ));
    }

    if merged.repo_summary.metrics.parse_extract_ms != parse_extract_ms {
        return Err(contract_violation(
            "repo_summary.metrics.parse_extract_ms",
            "parse_extract_ms must equal the sum of language parse_extract_ms values",
        ));
    }
    if merged.repo_summary.metrics.files_scanned != files_scanned {
        return Err(contract_violation(
            "repo_summary.metrics.files_scanned",
            "files_scanned must equal the sum of language files_scanned values",
        ));
    }

    let (expected_adoption, expected_resolution) = ratios_from_counts(&counts);
    validate_ratio(
        "repo_summary.metrics.invocation_adoption_ratio",
        merged.repo_summary.metrics.invocation_adoption_ratio,
        expected_adoption,
    )?;
    validate_ratio(
        "repo_summary.metrics.registry_resolution_ratio",
        merged.repo_summary.metrics.registry_resolution_ratio,
        expected_resolution,
    )
}

fn validate_token_inference(merged: &MergedScan) -> Result<(), ScanFactsError> {
    if !merged.token_inference.numeric_tolerance.is_finite()
        || merged.token_inference.numeric_tolerance < 0.0
    {
        return Err(contract_violation(
            "token_inference.numeric_tolerance",
            "numeric_tolerance must be a finite non-negative number",
        ));
    }

    let mut raw_sites_by_language = BTreeMap::new();
    let mut tokens_by_language = BTreeMap::new();
    let mut raw_site_count = 0_u32;
    for (language_id, facts) in &merged.languages {
        let raw_sites = raw_sites_by_language
            .entry(language_id.clone())
            .or_insert_with(BTreeMap::new);
        for (site_index, site) in facts.hardcoded_style_sites.iter().enumerate() {
            if raw_sites.insert(site.id.clone(), site).is_some() {
                return Err(contract_violation(
                    &format!("languages.{language_id}.hardcoded_style_sites[{site_index}].id"),
                    "raw hard-coded site ids must be unique within a language",
                ));
            }
            raw_site_count = raw_site_count.checked_add(1).ok_or_else(|| {
                contract_violation(
                    "token_inference.counts.hardcoded_observation_count",
                    "hardcoded observation count exceeds u32 maximum",
                )
            })?;
        }
        tokens_by_language.insert(
            language_id.clone(),
            facts
                .design_system_tokens
                .iter()
                .map(|token| (token.id.as_str(), token))
                .collect::<BTreeMap<_, _>>(),
        );
    }

    let mut inference_keys = BTreeSet::new();
    let mut exact = 0_u32;
    let mut near = 0_u32;
    let mut unmatched = 0_u32;
    let mut unassessed = 0_u32;
    let mut by_confidence = TokenConfidenceCounts::default();
    let mut by_category = TokenCategoryCounts::default();
    let mut by_context = StyleContextCounts::default();

    for (index, row) in merged.token_inference.sites.iter().enumerate() {
        let field = format!("token_inference.sites[{index}]");
        require_non_empty(&format!("{field}.site_id"), &row.site_id)?;

        if !inference_keys.insert((row.language.clone(), row.site_id.clone())) {
            return Err(contract_violation(
                &format!("{field}.site_id"),
                "inference (language, site_id) pairs must be unique",
            ));
        }

        if !merged.languages.contains_key(&row.language) {
            return Err(contract_violation(
                &format!("{field}.language"),
                "inference language must exist in merged scan languages",
            ));
        }
        let Some(site) = raw_sites_by_language
            .get(&row.language)
            .and_then(|sites| sites.get(row.site_id.as_str()))
        else {
            return Err(contract_violation(
                &format!("{field}.site_id"),
                "inference site_id must resolve to a raw hard-coded site in that language",
            ));
        };

        match row.classification {
            TokenInferenceClassification::Exact | TokenInferenceClassification::Near => {
                if row.suggestions.is_empty() {
                    return Err(contract_violation(
                        &format!("{field}.suggestions"),
                        "exact and near rows must include at least one suggestion",
                    ));
                }
                if row.confidence.is_none() {
                    return Err(contract_violation(
                        &format!("{field}.confidence"),
                        "exact and near rows must include confidence",
                    ));
                }
            }
            TokenInferenceClassification::Unmatched | TokenInferenceClassification::Unassessed => {
                if !row.suggestions.is_empty() {
                    return Err(contract_violation(
                        &format!("{field}.suggestions"),
                        "unmatched and unassessed rows must not include suggestions",
                    ));
                }
                if row.confidence.is_some() {
                    return Err(contract_violation(
                        &format!("{field}.confidence"),
                        "unmatched and unassessed rows must not include confidence",
                    ));
                }
            }
        }

        for (suggestion_index, suggestion) in row.suggestions.iter().enumerate() {
            let suggestion_field = format!("{field}.suggestions[{suggestion_index}]");
            require_non_empty(
                &format!("{suggestion_field}.token_id"),
                &suggestion.token_id,
            )?;
            require_non_empty(
                &format!("{suggestion_field}.token_key"),
                &suggestion.token_key,
            )?;
            require_non_empty(
                &format!("{suggestion_field}.canonical_value"),
                &suggestion.canonical_value,
            )?;

            let expected_kind = match row.classification {
                TokenInferenceClassification::Exact => TokenMatchKind::Exact,
                TokenInferenceClassification::Near => TokenMatchKind::Near,
                TokenInferenceClassification::Unmatched
                | TokenInferenceClassification::Unassessed => {
                    return Err(contract_violation(
                        &format!("{field}.suggestions"),
                        "unmatched and unassessed rows must not include suggestions",
                    ));
                }
            };
            if suggestion.match_kind != expected_kind {
                return Err(contract_violation(
                    &format!("{suggestion_field}.match_kind"),
                    "suggestion match_kind must agree with row classification",
                ));
            }

            match suggestion.match_kind {
                TokenMatchKind::Exact => match suggestion.distance {
                    None | Some(0.0) => {}
                    Some(_) => {
                        return Err(contract_violation(
                            &format!("{suggestion_field}.distance"),
                            "exact suggestion distance must be absent or 0",
                        ));
                    }
                },
                TokenMatchKind::Near => match suggestion.distance {
                    Some(distance) if distance.is_finite() && distance > 0.0 => {}
                    _ => {
                        return Err(contract_violation(
                            &format!("{suggestion_field}.distance"),
                            "near suggestion distance must be finite and positive",
                        ));
                    }
                },
            }

            let Some(token) = tokens_by_language
                .get(&row.language)
                .and_then(|tokens| tokens.get(suggestion.token_id.as_str()))
            else {
                return Err(contract_violation(
                    &format!("{suggestion_field}.token_id"),
                    "suggested token must exist in the same language",
                ));
            };
            if token.key != suggestion.token_key {
                return Err(contract_violation(
                    &format!("{suggestion_field}.token_key"),
                    "suggested token_key must match the registry token key",
                ));
            }
            let Some(canonical_value) = &token.value else {
                return Err(contract_violation(
                    &format!("{suggestion_field}.canonical_value"),
                    "suggested token must have a canonical registry value",
                ));
            };
            if canonical_value != &suggestion.canonical_value {
                return Err(contract_violation(
                    &format!("{suggestion_field}.canonical_value"),
                    "suggestion canonical_value must match the registry token value",
                ));
            }
            if token.category != site.category {
                return Err(contract_violation(
                    &format!("{suggestion_field}.token_id"),
                    "suggested token must share the raw site category",
                ));
            }
        }

        match row.classification {
            TokenInferenceClassification::Exact => {
                exact = exact.checked_add(1).ok_or_else(|| {
                    contract_violation(
                        "token_inference.counts.exact_replacement_candidate_count",
                        "count exceeds u32 maximum",
                    )
                })?;
                increment_confidence_count(&mut by_confidence, row.confidence)?;
                increment_token_category(
                    "token_inference.counts.candidates_by_category",
                    &mut by_category,
                    site.category,
                )?;
                increment_style_context_count(&mut by_context, site.context);
            }
            TokenInferenceClassification::Near => {
                near = near.checked_add(1).ok_or_else(|| {
                    contract_violation(
                        "token_inference.counts.near_replacement_candidate_count",
                        "count exceeds u32 maximum",
                    )
                })?;
                increment_confidence_count(&mut by_confidence, row.confidence)?;
                increment_token_category(
                    "token_inference.counts.candidates_by_category",
                    &mut by_category,
                    site.category,
                )?;
                increment_style_context_count(&mut by_context, site.context);
            }
            TokenInferenceClassification::Unmatched => {
                unmatched = unmatched.checked_add(1).ok_or_else(|| {
                    contract_violation(
                        "token_inference.counts.unmatched_observation_count",
                        "count exceeds u32 maximum",
                    )
                })?;
            }
            TokenInferenceClassification::Unassessed => {
                unassessed = unassessed.checked_add(1).ok_or_else(|| {
                    contract_violation(
                        "token_inference.counts.unassessed_observation_count",
                        "count exceeds u32 maximum",
                    )
                })?;
            }
        }
    }

    for (language_id, sites) in &raw_sites_by_language {
        for site_id in sites.keys() {
            if !inference_keys.contains(&(language_id.clone(), site_id.clone())) {
                return Err(contract_violation(
                    "token_inference.sites",
                    "every raw hard-coded site must have exactly one inference row",
                ));
            }
        }
    }

    let row_count = checked_len(
        "token_inference.counts.hardcoded_observation_count",
        merged.token_inference.sites.len(),
    )?;
    if row_count != raw_site_count {
        return Err(contract_violation(
            "token_inference.counts.hardcoded_observation_count",
            "hardcoded_observation_count must equal inference row count and raw hard-coded site count",
        ));
    }

    let assessed = exact
        .checked_add(near)
        .and_then(|value| value.checked_add(unmatched))
        .ok_or_else(|| {
            contract_violation(
                "token_inference.counts.assessed_observation_count",
                "count exceeds u32 maximum",
            )
        })?;
    let observations = assessed.checked_add(unassessed).ok_or_else(|| {
        contract_violation(
            "token_inference.counts.hardcoded_observation_count",
            "count exceeds u32 maximum",
        )
    })?;

    let counts = &merged.token_inference.counts;
    if counts.hardcoded_observation_count != observations
        || counts.hardcoded_observation_count != row_count
    {
        return Err(contract_violation(
            "token_inference.counts.hardcoded_observation_count",
            "hardcoded_observation_count must equal assessed + unassessed and the inference row count",
        ));
    }
    if counts.assessed_observation_count != assessed {
        return Err(contract_violation(
            "token_inference.counts.assessed_observation_count",
            "assessed_observation_count must equal exact + near + unmatched",
        ));
    }
    if counts.exact_replacement_candidate_count != exact {
        return Err(contract_violation(
            "token_inference.counts.exact_replacement_candidate_count",
            "exact_replacement_candidate_count must match exact rows",
        ));
    }
    if counts.near_replacement_candidate_count != near {
        return Err(contract_violation(
            "token_inference.counts.near_replacement_candidate_count",
            "near_replacement_candidate_count must match near rows",
        ));
    }
    if counts.unmatched_observation_count != unmatched {
        return Err(contract_violation(
            "token_inference.counts.unmatched_observation_count",
            "unmatched_observation_count must match unmatched rows",
        ));
    }
    if counts.unassessed_observation_count != unassessed {
        return Err(contract_violation(
            "token_inference.counts.unassessed_observation_count",
            "unassessed_observation_count must match unassessed rows",
        ));
    }
    if counts.candidates_by_confidence != by_confidence {
        return Err(contract_violation(
            "token_inference.counts.candidates_by_confidence",
            "candidates_by_confidence must match exact and near rows",
        ));
    }
    if counts.candidates_by_category != by_category {
        return Err(contract_violation(
            "token_inference.counts.candidates_by_category",
            "candidates_by_category must match exact and near rows",
        ));
    }
    if counts.candidates_by_context != by_context {
        return Err(contract_violation(
            "token_inference.counts.candidates_by_context",
            "candidates_by_context must match exact and near rows",
        ));
    }

    Ok(())
}

fn increment_confidence_count(
    counts: &mut TokenConfidenceCounts,
    confidence: Option<TokenInferenceConfidence>,
) -> Result<(), ScanFactsError> {
    match confidence {
        Some(TokenInferenceConfidence::VeryHigh) => {
            counts.very_high = counts.very_high.checked_add(1).ok_or_else(|| {
                contract_violation(
                    "token_inference.counts.candidates_by_confidence.very_high",
                    "count exceeds u32 maximum",
                )
            })?;
        }
        Some(TokenInferenceConfidence::High) => {
            counts.high = counts.high.checked_add(1).ok_or_else(|| {
                contract_violation(
                    "token_inference.counts.candidates_by_confidence.high",
                    "count exceeds u32 maximum",
                )
            })?;
        }
        Some(TokenInferenceConfidence::Medium) => {
            counts.medium = counts.medium.checked_add(1).ok_or_else(|| {
                contract_violation(
                    "token_inference.counts.candidates_by_confidence.medium",
                    "count exceeds u32 maximum",
                )
            })?;
        }
        Some(TokenInferenceConfidence::Low) => {
            counts.low = counts.low.checked_add(1).ok_or_else(|| {
                contract_violation(
                    "token_inference.counts.candidates_by_confidence.low",
                    "count exceeds u32 maximum",
                )
            })?;
        }
        None => {
            return Err(contract_violation(
                "token_inference.counts.candidates_by_confidence",
                "exact and near rows must include confidence",
            ));
        }
    }
    Ok(())
}

fn increment_style_context_count(counts: &mut StyleContextCounts, context: StyleContext) {
    match context {
        StyleContext::Padding => counts.padding = counts.padding.saturating_add(1),
        StyleContext::Margin => counts.margin = counts.margin.saturating_add(1),
        StyleContext::Gap => counts.gap = counts.gap.saturating_add(1),
        StyleContext::Width => counts.width = counts.width.saturating_add(1),
        StyleContext::Height => counts.height = counts.height.saturating_add(1),
        StyleContext::Size => counts.size = counts.size.saturating_add(1),
        StyleContext::Radius => counts.radius = counts.radius.saturating_add(1),
        StyleContext::Color => counts.color = counts.color.saturating_add(1),
        StyleContext::Typography => counts.typography = counts.typography.saturating_add(1),
        StyleContext::Elevation => counts.elevation = counts.elevation.saturating_add(1),
        StyleContext::Unknown => counts.unknown = counts.unknown.saturating_add(1),
    }
}

fn validate_parent_scope(field: &str, parent: &ParentScope) -> Result<(), ScanFactsError> {
    require_non_empty(&format!("{field}.parent_id"), &parent.parent_id)?;
    require_non_empty(&format!("{field}.symbol"), &parent.symbol)?;
    require_non_empty(&format!("{field}.scope_kind"), &parent.scope_kind)?;
    require_non_empty(&format!("{field}.identity_basis"), &parent.identity_basis)?;
    if let Some(location) = &parent.location {
        validate_location(&format!("{field}.location"), location)?;
    }
    Ok(())
}

fn validate_symbol_usage_summary(
    field: &str,
    summary: &SymbolUsageSummary,
) -> Result<(), ScanFactsError> {
    require_non_empty(&format!("{field}.symbol_id"), &summary.symbol_id)?;
    require_non_empty(&format!("{field}.symbol"), &summary.symbol)?;
    require_non_empty(&format!("{field}.identity_basis"), &summary.identity_basis)?;

    let expected_status = match summary.symbol_kind {
        SymbolKind::Registry => MatchStatus::Resolved,
        SymbolKind::Local => MatchStatus::Local,
        SymbolKind::Candidate => MatchStatus::Candidate,
        SymbolKind::Unresolved => MatchStatus::Unresolved,
    };
    if summary.match_status != expected_status {
        return Err(contract_violation(
            &format!("{field}.match_status"),
            "match_status must align with symbol_kind",
        ));
    }

    match summary.symbol_kind {
        SymbolKind::Registry => {
            require_non_empty(
                &format!("{field}.registry_symbol"),
                summary.registry_symbol.as_deref().unwrap_or(""),
            )?;
            if summary.local_definition_id.is_some() {
                return Err(contract_violation(
                    &format!("{field}.local_definition_id"),
                    "registry symbol summaries must not carry local_definition_id",
                ));
            }
        }
        SymbolKind::Local => {
            require_non_empty(
                &format!("{field}.local_definition_id"),
                summary.local_definition_id.as_deref().unwrap_or(""),
            )?;
            if summary.registry_symbol.is_some() {
                return Err(contract_violation(
                    &format!("{field}.registry_symbol"),
                    "local symbol summaries must not carry registry_symbol",
                ));
            }
        }
        SymbolKind::Candidate => {
            require_non_empty(
                &format!("{field}.registry_symbol"),
                summary.registry_symbol.as_deref().unwrap_or(""),
            )?;
            if summary.local_definition_id.is_some() {
                return Err(contract_violation(
                    &format!("{field}.local_definition_id"),
                    "candidate symbol summaries must not carry local_definition_id",
                ));
            }
        }
        SymbolKind::Unresolved => {
            if summary.registry_symbol.is_some() || summary.local_definition_id.is_some() {
                return Err(contract_violation(
                    field,
                    "unresolved symbol summaries must not carry registry or local linkage",
                ));
            }
        }
    }

    if summary.parent_scope_limit == Some(0) && !summary.parent_scopes.is_empty() {
        return Err(contract_violation(
            &format!("{field}.parent_scopes"),
            "parent_scope_limit 0 requires empty parent_scopes",
        ));
    }

    let emitted = summary.parent_scopes.len() as u64;
    if summary.parent_scopes_truncated && emitted >= u64::from(summary.parent_scope_count) {
        return Err(contract_violation(
            &format!("{field}.parent_scopes_truncated"),
            "parent_scopes_truncated must be true only when emitted rows are fewer than parent_scope_count",
        ));
    }

    if !summary.parent_scopes_truncated
        && summary.parent_scope_limit != Some(0)
        && emitted != u64::from(summary.parent_scope_count)
        && summary.parent_scope_limit.is_some()
    {
        let limit = summary
            .parent_scope_limit
            .unwrap_or(summary.parent_scope_count);
        if emitted != u64::from(limit.min(summary.parent_scope_count)) {
            return Err(contract_violation(
                &format!("{field}.parent_scopes"),
                "parent_scopes length must match limit semantics",
            ));
        }
    }

    for (index, parent) in summary.parent_scopes.iter().enumerate() {
        let field = format!("{field}.parent_scopes[{index}]");
        require_non_empty(&format!("{field}.parent_id"), &parent.parent_id)?;
        require_non_empty(&format!("{field}.symbol"), &parent.symbol)?;
        require_non_empty(&format!("{field}.scope_kind"), &parent.scope_kind)?;
        require_non_empty(&format!("{field}.identity_basis"), &parent.identity_basis)?;
        if let Some(location) = &parent.location {
            validate_location(&format!("{field}.location"), location)?;
        }
    }

    Ok(())
}

fn validate_token_usage_summary(
    field: &str,
    summary: &TokenUsageSummary,
) -> Result<(), ScanFactsError> {
    require_non_empty(&format!("{field}.language"), &summary.language)?;
    require_non_empty(&format!("{field}.token_id"), &summary.token_id)?;
    require_non_empty(&format!("{field}.key"), &summary.key)?;
    Ok(())
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
        || matches!(
            path,
            [section, index, field]
                if section == "symbol_usage_summary"
                    && index.parse::<usize>().is_ok()
                    && field == "parent_scope_limit"
        )
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

fn validate_usage_site_linkage(field: &str, site: &UsageSite) -> Result<(), ScanFactsError> {
    match site.match_status {
        MatchStatus::Resolved | MatchStatus::Candidate => {
            require_non_empty(
                &format!("{field}.registry_symbol"),
                site.registry_symbol.as_deref().unwrap_or(""),
            )?;
            if site.local_definition_id.is_some() {
                return Err(contract_violation(
                    &format!("{field}.local_definition_id"),
                    "local_definition_id must be absent for resolved and candidate usage",
                ));
            }
        }
        MatchStatus::Local => {
            require_non_empty(
                &format!("{field}.local_definition_id"),
                site.local_definition_id.as_deref().unwrap_or(""),
            )?;
            if site.registry_symbol.is_some() {
                return Err(contract_violation(
                    &format!("{field}.registry_symbol"),
                    "registry_symbol must be absent for local usage",
                ));
            }
        }
        MatchStatus::Unresolved => {
            if site.registry_symbol.is_some() {
                return Err(contract_violation(
                    &format!("{field}.registry_symbol"),
                    "registry_symbol must be absent for unresolved usage",
                ));
            }
            if site.local_definition_id.is_some() {
                return Err(contract_violation(
                    &format!("{field}.local_definition_id"),
                    "local_definition_id must be absent for unresolved usage",
                ));
            }
        }
    }
    Ok(())
}

fn validate_derived_values(facts: &ScanFacts) -> Result<(), ScanFactsError> {
    if facts.metrics.parse_extract_ms > MAX_PARSE_EXTRACT_MS {
        return Err(contract_violation(
            "metrics.parse_extract_ms",
            "parse_extract_ms exceeds the JSON contract maximum",
        ));
    }

    let (expected_counts, (expected_adoption, expected_resolution)) =
        derive_counts_and_metrics(facts)?;

    if facts.counts != expected_counts {
        return Err(contract_violation(
            "counts",
            "count summary must match the emitted facts",
        ));
    }

    validate_ratio(
        "metrics.invocation_adoption_ratio",
        facts.metrics.invocation_adoption_ratio,
        expected_adoption,
    )?;
    validate_ratio(
        "metrics.registry_resolution_ratio",
        facts.metrics.registry_resolution_ratio,
        expected_resolution,
    )
}

fn checked_add_count_summaries(
    field: &str,
    left: &CountSummary,
    right: &CountSummary,
) -> Result<CountSummary, ScanFactsError> {
    Ok(CountSummary {
        registry: RegistryCounts {
            component_count: checked_add_count(
                &format!("{field}.registry.component_count"),
                left.registry.component_count,
                right.registry.component_count,
            )?,
            used_component_count: checked_add_count(
                &format!("{field}.registry.used_component_count"),
                left.registry.used_component_count,
                right.registry.used_component_count,
            )?,
            resolved_raw_invocation_count: checked_add_count(
                &format!("{field}.registry.resolved_raw_invocation_count"),
                left.registry.resolved_raw_invocation_count,
                right.registry.resolved_raw_invocation_count,
            )?,
            candidate_raw_invocation_count: checked_add_count(
                &format!("{field}.registry.candidate_raw_invocation_count"),
                left.registry.candidate_raw_invocation_count,
                right.registry.candidate_raw_invocation_count,
            )?,
        },
        definitions: DefinitionCounts {
            local_definition_count: checked_add_count(
                &format!("{field}.definitions.local_definition_count"),
                left.definitions.local_definition_count,
                right.definitions.local_definition_count,
            )?,
            invoked_local_definition_count: checked_add_count(
                &format!("{field}.definitions.invoked_local_definition_count"),
                left.definitions.invoked_local_definition_count,
                right.definitions.invoked_local_definition_count,
            )?,
            unused_local_definition_count: checked_add_count(
                &format!("{field}.definitions.unused_local_definition_count"),
                left.definitions.unused_local_definition_count,
                right.definitions.unused_local_definition_count,
            )?,
        },
        raw_invocations: RawInvocationCounts {
            total: checked_add_count(
                &format!("{field}.raw_invocations.total"),
                left.raw_invocations.total,
                right.raw_invocations.total,
            )?,
            resolved: checked_add_count(
                &format!("{field}.raw_invocations.resolved"),
                left.raw_invocations.resolved,
                right.raw_invocations.resolved,
            )?,
            local: checked_add_count(
                &format!("{field}.raw_invocations.local"),
                left.raw_invocations.local,
                right.raw_invocations.local,
            )?,
            candidate: checked_add_count(
                &format!("{field}.raw_invocations.candidate"),
                left.raw_invocations.candidate,
                right.raw_invocations.candidate,
            )?,
            unresolved: checked_add_count(
                &format!("{field}.raw_invocations.unresolved"),
                left.raw_invocations.unresolved,
                right.raw_invocations.unresolved,
            )?,
        },
        adoption: AdoptionCounts {
            eligible_invocation_count: checked_add_count(
                &format!("{field}.adoption.eligible_invocation_count"),
                left.adoption.eligible_invocation_count,
                right.adoption.eligible_invocation_count,
            )?,
            adopted_invocation_count: checked_add_count(
                &format!("{field}.adoption.adopted_invocation_count"),
                left.adoption.adopted_invocation_count,
                right.adoption.adopted_invocation_count,
            )?,
            non_adopted_invocation_count: checked_add_count(
                &format!("{field}.adoption.non_adopted_invocation_count"),
                left.adoption.non_adopted_invocation_count,
                right.adoption.non_adopted_invocation_count,
            )?,
        },
        parent_scopes: ParentScopeCounts {
            total: checked_add_count(
                &format!("{field}.parent_scopes.total"),
                left.parent_scopes.total,
                right.parent_scopes.total,
            )?,
            with_resolved_invocations: checked_add_count(
                &format!("{field}.parent_scopes.with_resolved_invocations"),
                left.parent_scopes.with_resolved_invocations,
                right.parent_scopes.with_resolved_invocations,
            )?,
            with_local_invocations: checked_add_count(
                &format!("{field}.parent_scopes.with_local_invocations"),
                left.parent_scopes.with_local_invocations,
                right.parent_scopes.with_local_invocations,
            )?,
            with_unresolved_invocations: checked_add_count(
                &format!("{field}.parent_scopes.with_unresolved_invocations"),
                left.parent_scopes.with_unresolved_invocations,
                right.parent_scopes.with_unresolved_invocations,
            )?,
        },
        tokens: TokenCounts {
            configured_token_count: checked_add_count(
                &format!("{field}.tokens.configured_token_count"),
                left.tokens.configured_token_count,
                right.tokens.configured_token_count,
            )?,
            used_token_count: checked_add_count(
                &format!("{field}.tokens.used_token_count"),
                left.tokens.used_token_count,
                right.tokens.used_token_count,
            )?,
            token_reference_site_count: checked_add_count(
                &format!("{field}.tokens.token_reference_site_count"),
                left.tokens.token_reference_site_count,
                right.tokens.token_reference_site_count,
            )?,
            hardcoded_style_candidate_count: checked_add_count(
                &format!("{field}.tokens.hardcoded_style_candidate_count"),
                left.tokens.hardcoded_style_candidate_count,
                right.tokens.hardcoded_style_candidate_count,
            )?,
            token_references_by_category: add_token_category_counts(
                &left.tokens.token_references_by_category,
                &right.tokens.token_references_by_category,
            ),
            hardcoded_by_category: add_token_category_counts(
                &left.tokens.hardcoded_by_category,
                &right.tokens.hardcoded_by_category,
            ),
            parent_scopes_with_token_references: checked_add_count(
                &format!("{field}.tokens.parent_scopes_with_token_references"),
                left.tokens.parent_scopes_with_token_references,
                right.tokens.parent_scopes_with_token_references,
            )?,
            parent_scopes_with_hardcoded_candidates: checked_add_count(
                &format!("{field}.tokens.parent_scopes_with_hardcoded_candidates"),
                left.tokens.parent_scopes_with_hardcoded_candidates,
                right.tokens.parent_scopes_with_hardcoded_candidates,
            )?,
        },
    })
}

fn add_token_category_counts(
    left: &TokenCategoryCounts,
    right: &TokenCategoryCounts,
) -> TokenCategoryCounts {
    TokenCategoryCounts {
        color: left.color.saturating_add(right.color),
        spacing: left.spacing.saturating_add(right.spacing),
        typography: left.typography.saturating_add(right.typography),
        radius: left.radius.saturating_add(right.radius),
        elevation: left.elevation.saturating_add(right.elevation),
        unknown: left.unknown.saturating_add(right.unknown),
    }
}

fn checked_add_count(field: &str, left: u32, right: u32) -> Result<u32, ScanFactsError> {
    left.checked_add(right)
        .ok_or_else(|| contract_violation(field, "count exceeds u32 maximum"))
}

fn ratios_from_counts(counts: &CountSummary) -> DerivedMetrics {
    let invocation_adoption_ratio = if counts.adoption.eligible_invocation_count == 0 {
        None
    } else {
        Some(
            f64::from(counts.adoption.adopted_invocation_count)
                / f64::from(counts.adoption.eligible_invocation_count),
        )
    };
    let registry_resolution_ratio = if counts.raw_invocations.total == 0 {
        None
    } else {
        Some(f64::from(counts.raw_invocations.resolved) / f64::from(counts.raw_invocations.total))
    };
    (invocation_adoption_ratio, registry_resolution_ratio)
}

fn validate_ratio(
    field: &str,
    actual: Option<f64>,
    expected: Option<f64>,
) -> Result<(), ScanFactsError> {
    match (actual, expected) {
        (None, None) => Ok(()),
        (Some(actual), _) if !actual.is_finite() => {
            Err(contract_violation(field, "ratio must be finite"))
        }
        (Some(actual), _) if !(0.0..=1.0).contains(&actual) => {
            Err(contract_violation(field, "ratio must be between 0 and 1"))
        }
        (Some(actual), Some(expected)) if (actual - expected).abs() <= 1e-12 => Ok(()),
        (Some(_), None) => Err(contract_violation(
            field,
            "ratio must be null when the denominator is zero",
        )),
        (None, Some(_)) => Err(contract_violation(
            field,
            "ratio is required when the denominator is non-zero",
        )),
        (Some(_), Some(_)) => Err(contract_violation(
            field,
            "ratio must match derived counters",
        )),
    }
}

type DerivedMetrics = (Option<f64>, Option<f64>);

fn increment_token_category(
    field: &str,
    counts: &mut TokenCategoryCounts,
    category: TokenCategory,
) -> Result<(), ScanFactsError> {
    let slot = match category {
        TokenCategory::Color => &mut counts.color,
        TokenCategory::Spacing => &mut counts.spacing,
        TokenCategory::Typography => &mut counts.typography,
        TokenCategory::Radius => &mut counts.radius,
        TokenCategory::Elevation => &mut counts.elevation,
        TokenCategory::Unknown => &mut counts.unknown,
    };
    increment_count(field, slot)
}

fn derive_counts_and_metrics(
    facts: &ScanFacts,
) -> Result<(CountSummary, DerivedMetrics), ScanFactsError> {
    let mut resolved = 0_u32;
    let mut local = 0_u32;
    let mut candidate = 0_u32;
    let mut unresolved = 0_u32;
    let mut used_registry_symbols = BTreeSet::new();
    let mut invoked_local_ids = BTreeSet::new();
    let mut parent_ids = BTreeSet::new();
    let mut parents_with_resolved = BTreeSet::new();
    let mut parents_with_local = BTreeSet::new();
    let mut parents_with_unresolved = BTreeSet::new();

    let mut token_by_id = BTreeMap::new();
    for (index, token) in facts.design_system_tokens.iter().enumerate() {
        let field = format!("design_system_tokens[{index}]");
        if token.id.is_empty() {
            return Err(contract_violation(
                &format!("{field}.id"),
                "token id must not be empty",
            ));
        }
        if token.key.is_empty() {
            return Err(contract_violation(
                &format!("{field}.key"),
                "token key must not be empty",
            ));
        }
        for (alias_index, alias) in token.aliases.iter().enumerate() {
            if alias.is_empty() {
                return Err(contract_violation(
                    &format!("{field}.aliases[{alias_index}]"),
                    "token alias must not be empty",
                ));
            }
        }
        if token_by_id
            .insert(token.id.clone(), token.clone())
            .is_some()
        {
            return Err(contract_violation(
                &format!("{field}.id"),
                "duplicate token id",
            ));
        }
    }

    let mut used_token_ids = BTreeSet::new();
    let mut token_references_by_category = TokenCategoryCounts::default();
    let mut hardcoded_by_category = TokenCategoryCounts::default();
    let mut token_parent_ids = BTreeSet::new();
    let mut hardcoded_parent_ids = BTreeSet::new();
    let mut token_reference_site_count = 0_u32;
    let mut hardcoded_style_candidate_count = 0_u32;

    for (index, site) in facts.token_sites.iter().enumerate() {
        let field = format!("token_sites[{index}]");
        let Some(token) = token_by_id.get(&site.token_id) else {
            return Err(contract_violation(
                &format!("{field}.token_id"),
                "token_id must reference design_system_tokens",
            ));
        };
        if site.key != token.key && !token.aliases.contains(&site.key) {
            return Err(contract_violation(
                &format!("{field}.key"),
                "key must match token key or alias",
            ));
        }
        if site.category != token.category {
            return Err(contract_violation(
                &format!("{field}.category"),
                "category must match referenced token category",
            ));
        }
        used_token_ids.insert(site.token_id.clone());
        increment_count(
            "counts.tokens.token_reference_site_count",
            &mut token_reference_site_count,
        )?;
        increment_token_category(
            "counts.tokens.token_references_by_category",
            &mut token_references_by_category,
            site.category,
        )?;
        if let Some(parent) = &site.parent {
            token_parent_ids.insert(parent.parent_id.clone());
        }
    }

    for (index, site) in facts.hardcoded_style_sites.iter().enumerate() {
        let field = format!("hardcoded_style_sites[{index}]");
        if site.value.is_empty() {
            return Err(contract_violation(
                &format!("{field}.value"),
                "value must not be empty",
            ));
        }
        increment_count(
            "counts.tokens.hardcoded_style_candidate_count",
            &mut hardcoded_style_candidate_count,
        )?;
        increment_token_category(
            "counts.tokens.hardcoded_by_category",
            &mut hardcoded_by_category,
            site.category,
        )?;
        if let Some(parent) = &site.parent {
            hardcoded_parent_ids.insert(parent.parent_id.clone());
        }
    }

    for site in &facts.usage_sites {
        match site.match_status {
            MatchStatus::Resolved => {
                increment_count("counts.raw_invocations.resolved", &mut resolved)?;
                if let Some(registry_symbol) = &site.registry_symbol {
                    used_registry_symbols.insert(registry_symbol.clone());
                }
            }
            MatchStatus::Local => {
                increment_count("counts.raw_invocations.local", &mut local)?;
                if let Some(local_id) = &site.local_definition_id {
                    invoked_local_ids.insert(local_id.clone());
                }
            }
            MatchStatus::Candidate => {
                increment_count("counts.raw_invocations.candidate", &mut candidate)?;
            }
            MatchStatus::Unresolved => {
                increment_count("counts.raw_invocations.unresolved", &mut unresolved)?;
            }
        }

        if let Some(parent) = &site.parent {
            parent_ids.insert(parent.parent_id.clone());
            match site.match_status {
                MatchStatus::Resolved => {
                    parents_with_resolved.insert(parent.parent_id.clone());
                }
                MatchStatus::Local => {
                    parents_with_local.insert(parent.parent_id.clone());
                }
                MatchStatus::Unresolved => {
                    parents_with_unresolved.insert(parent.parent_id.clone());
                }
                MatchStatus::Candidate => {}
            }
        }
    }

    let total = checked_add_many(
        "counts.raw_invocations.total",
        &[resolved, local, candidate, unresolved],
    )?;
    let eligible = checked_add_many(
        "counts.adoption.eligible_invocation_count",
        &[resolved, local, unresolved],
    )?;
    let adopted = resolved;
    let non_adopted = eligible.checked_sub(adopted).ok_or_else(|| {
        contract_violation(
            "counts.adoption.non_adopted_invocation_count",
            "non_adopted count underflow",
        )
    })?;

    let local_definition_count = checked_len(
        "counts.definitions.local_definition_count",
        facts.local_components.len(),
    )?;
    let invoked_local_definition_count = checked_len(
        "counts.definitions.invoked_local_definition_count",
        invoked_local_ids.len(),
    )?;
    let unused_local_definition_count =
        local_definition_count.saturating_sub(invoked_local_definition_count);

    let counts = CountSummary {
        registry: RegistryCounts {
            component_count: checked_len(
                "counts.registry.component_count",
                facts.design_system_components.len(),
            )?,
            used_component_count: checked_len(
                "counts.registry.used_component_count",
                used_registry_symbols.len(),
            )?,
            resolved_raw_invocation_count: resolved,
            candidate_raw_invocation_count: candidate,
        },
        definitions: DefinitionCounts {
            local_definition_count,
            invoked_local_definition_count,
            unused_local_definition_count,
        },
        raw_invocations: RawInvocationCounts {
            total,
            resolved,
            local,
            candidate,
            unresolved,
        },
        adoption: AdoptionCounts {
            eligible_invocation_count: eligible,
            adopted_invocation_count: adopted,
            non_adopted_invocation_count: non_adopted,
        },
        parent_scopes: ParentScopeCounts {
            total: checked_len("counts.parent_scopes.total", parent_ids.len())?,
            with_resolved_invocations: checked_len(
                "counts.parent_scopes.with_resolved_invocations",
                parents_with_resolved.len(),
            )?,
            with_local_invocations: checked_len(
                "counts.parent_scopes.with_local_invocations",
                parents_with_local.len(),
            )?,
            with_unresolved_invocations: checked_len(
                "counts.parent_scopes.with_unresolved_invocations",
                parents_with_unresolved.len(),
            )?,
        },
        tokens: TokenCounts {
            configured_token_count: checked_len(
                "counts.tokens.configured_token_count",
                facts.design_system_tokens.len(),
            )?,
            used_token_count: checked_len("counts.tokens.used_token_count", used_token_ids.len())?,
            token_reference_site_count,
            hardcoded_style_candidate_count,
            token_references_by_category,
            hardcoded_by_category,
            parent_scopes_with_token_references: checked_len(
                "counts.tokens.parent_scopes_with_token_references",
                token_parent_ids.len(),
            )?,
            parent_scopes_with_hardcoded_candidates: checked_len(
                "counts.tokens.parent_scopes_with_hardcoded_candidates",
                hardcoded_parent_ids.len(),
            )?,
        },
    };

    let invocation_adoption_ratio = if eligible == 0 {
        None
    } else {
        Some(f64::from(adopted) / f64::from(eligible))
    };
    let registry_resolution_ratio = if total == 0 {
        None
    } else {
        Some(f64::from(resolved) / f64::from(total))
    };

    Ok((
        counts,
        (invocation_adoption_ratio, registry_resolution_ratio),
    ))
}

fn checked_add_many(field: &str, values: &[u32]) -> Result<u32, ScanFactsError> {
    values.iter().try_fold(0_u32, |acc, value| {
        acc.checked_add(*value)
            .ok_or_else(|| contract_violation(field, "count exceeds u32 maximum"))
    })
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
