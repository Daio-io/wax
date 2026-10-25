//! Versioned registry lookup envelopes, snapshot references, and error codes.

use crate::{Diagnostic, LanguageId, RegistryExtractionStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// JSON schema version for lookup envelopes (search, detail, and errors).
pub const REGISTRY_LOOKUP_SCHEMA_VERSION: u32 = 1;

/// Kind of registry entry addressed by search or detail lookup.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RegistryEntryKind {
    /// Component entry.
    Component,
    /// Token entry.
    Token,
}

/// Identity of an immutable registry snapshot used for lookup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySnapshotRef {
    /// Canonical source identity for the registry content.
    pub source: String,
    /// Language id for the snapshot.
    pub language: LanguageId,
    /// SHA-256 digest of the registry bytes.
    pub digest: String,
    /// Optional remembered or published design-system id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_id: Option<String>,
    /// Optional Git commit when the source is Git-backed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    /// Unknown snapshot fields preserved across serialization.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Filters applied to a registry search request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySearchFilters {
    /// Optional entry-kind filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<RegistryEntryKind>,
    /// Optional language filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    /// Maximum number of results to return.
    pub limit: u32,
    /// Unknown filter fields preserved across serialization.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One ranked search hit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySearchResult {
    /// Snapshot that produced this hit.
    pub snapshot: RegistrySnapshotRef,
    /// Entry kind.
    pub kind: RegistryEntryKind,
    /// Exact entry id within the snapshot registry.
    pub id: String,
    /// Short display label (commonly the symbol or key).
    pub label: String,
    /// Bounded summary text for the hit.
    pub summary: String,
    /// Alternative names that can be used to find this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Structured metadata tags that can be used to find this entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Extraction status when enrichment is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_status: Option<RegistryExtractionStatus>,
    /// Unknown result fields preserved across serialization.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Versioned successful search response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistrySearchResponse {
    /// Lookup envelope schema version.
    pub schema_version: u32,
    /// Original search query.
    pub query: String,
    /// Effective filters for the search.
    pub filters: RegistrySearchFilters,
    /// Snapshots selected for the search.
    pub snapshots: Vec<RegistrySnapshotRef>,
    /// Ranked results.
    pub results: Vec<RegistrySearchResult>,
    /// Non-fatal diagnostics produced while searching.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Unknown response fields preserved across serialization.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Versioned successful detail response for one registry entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryDetailResponse {
    /// Lookup envelope schema version.
    pub schema_version: u32,
    /// Snapshot that produced this entry.
    pub snapshot: RegistrySnapshotRef,
    /// Entry kind.
    pub kind: RegistryEntryKind,
    /// Full lossless entry object (component or token).
    pub entry: Value,
    /// Unknown response fields preserved across serialization.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Stable lookup failure codes shared by later phases.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RegistryLookupErrorCode {
    /// Caller arguments were invalid.
    InvalidArgument,
    /// The selected source could not be found.
    SourceNotFound,
    /// Registry bytes were present but invalid.
    InvalidRegistry,
    /// Offline mode could not satisfy the request from cache.
    OfflineCacheMiss,
    /// A locked snapshot could not be retrieved.
    LockedSnapshotUnavailable,
    /// Multiple entries matched an exact lookup.
    AmbiguousEntry,
    /// No entry matched an exact lookup.
    EntryNotFound,
    /// A requested snapshot digest or tag conflicted with selected context.
    SnapshotMismatch,
}

/// Typed registry lookup failure used by JSON output and core boundaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RegistryLookupError {
    /// Stable machine-readable failure code.
    pub code: RegistryLookupErrorCode,
    /// Human-readable failure message.
    pub message: String,
    /// Supporting diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Unknown error fields preserved across serialization.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl RegistryLookupError {
    /// Builds a lookup error with no diagnostics or extension fields.
    #[must_use]
    pub fn new(code: RegistryLookupErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            diagnostics: Vec::new(),
            extra: Map::new(),
        }
    }
}
