//! Language-agnostic facts emitted by language packs and merged by the engine.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageMetadata {
    pub id: String,
    pub version: String,
    pub ecosystem: String,
    pub parser: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Resolved,
    Candidate,
    Unresolved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScanFacts {
    pub schema_version: u32,
    pub language: LanguageMetadata,
    /// Assigned by the engine; echoed verbatim by the pack.
    pub snapshot_id: String,
    pub status: ScanStatus,
    pub design_system_components: Vec<DesignSystemComponent>,
    pub local_components: Vec<LocalComponent>,
    pub usage_sites: Vec<UsageSite>,
    pub diagnostics: Vec<Diagnostic>,
    pub metrics: Metrics,
    pub counts: CountSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignSystemComponent {
    pub id: String,
    pub symbol: String,
    pub registry_symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalComponent {
    pub id: String,
    pub symbol: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageSite {
    pub id: String,
    pub file: String,
    pub line: u32,
    pub symbol: String,
    pub match_status: MatchStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    /// Ratio in \[0.0, 1.0\]. `None` when there are zero usage sites.
    pub adoption_coverage_ratio: Option<f64>,
    pub parse_extract_ms: u64,
    pub files_scanned: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CountSummary {
    pub design_system_component_count: u32,
    pub local_component_count: u32,
    pub usage_site_count: u32,
    pub resolved_count: u32,
    pub candidate_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MergedScan {
    pub schema_version: u32,
    pub recorded_at: String,
    pub languages: BTreeMap<String, ScanFacts>,
}

pub fn validate_schema_version(version: u32) -> Result<(), String> {
    if version != SCHEMA_VERSION {
        Err(format!(
            "unsupported ScanFacts schema_version {version} (engine supports {SCHEMA_VERSION})"
        ))
    } else {
        Ok(())
    }
}

/// Deserialize and enforce `schema_version`.
pub fn scan_facts_from_json(json: &str) -> Result<ScanFacts, String> {
    let facts: ScanFacts =
        serde_json::from_str(json).map_err(|e| format!("invalid ScanFacts json: {e}"))?;
    validate_schema_version(facts.schema_version)?;
    Ok(facts)
}

impl ScanFacts {
    /// Recompute counts derivable from `usage_sites` and component lists.
    pub fn recompute_counts(&mut self) {
        let resolved = self
            .usage_sites
            .iter()
            .filter(|u| u.match_status == MatchStatus::Resolved)
            .count() as u32;
        let candidate = self
            .usage_sites
            .iter()
            .filter(|u| u.match_status == MatchStatus::Candidate)
            .count() as u32;

        self.metrics.adoption_coverage_ratio = if self.usage_sites.is_empty() {
            None
        } else {
            Some(resolved as f64 / self.usage_sites.len() as f64)
        };

        self.counts = CountSummary {
            design_system_component_count: self.design_system_components.len() as u32,
            local_component_count: self.local_components.len() as u32,
            usage_site_count: self.usage_sites.len() as u32,
            resolved_count: resolved,
            candidate_count: candidate,
        };
    }
}
