//! Language-agnostic facts emitted by language packs and merged by the engine.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageMetadata {
    pub id: String,
    pub version: String,
    pub ecosystem: String,
    pub parser: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFacts {
    pub schema_version: u32,
    pub language: LanguageMetadata,
    pub snapshot_id: String,
    pub status: String,
    pub design_system_components: Vec<DesignSystemComponent>,
    pub local_components: Vec<LocalComponent>,
    pub usage_sites: Vec<UsageSite>,
    pub diagnostics: Vec<Diagnostic>,
    pub metrics: Metrics,
    pub counts: CountSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignSystemComponent {
    pub id: String,
    pub symbol: String,
    pub registry_symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalComponent {
    pub id: String,
    pub symbol: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSite {
    pub id: String,
    pub file: String,
    pub line: u32,
    pub symbol: String,
    pub match_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub adoption_coverage_ratio: f64,
    pub parse_extract_ms: f64,
    pub files_scanned: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountSummary {
    pub design_system_component_count: u32,
    pub local_component_count: u32,
    pub usage_site_count: u32,
    pub resolved_count: u32,
    pub candidate_count: u32,
    pub modifier_chain_count: u32,
    pub slot_lambda_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryRef {
    pub design_system_symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergedScan {
    pub schema_version: u32,
    pub recorded_at: String,
    pub languages: BTreeMap<String, ScanFacts>,
}

impl ScanFacts {
    pub fn recompute_counts(&mut self) {
        let resolved = self
            .usage_sites
            .iter()
            .filter(|u| u.match_status == "resolved")
            .count() as u32;
        let candidate = self
            .usage_sites
            .iter()
            .filter(|u| u.match_status == "candidate")
            .count() as u32;
        let classified = self.usage_sites.len().max(1) as f64;
        self.metrics.adoption_coverage_ratio = resolved as f64 / classified;
        self.counts = CountSummary {
            design_system_component_count: self.design_system_components.len() as u32,
            local_component_count: self.local_components.len() as u32,
            usage_site_count: self.usage_sites.len() as u32,
            resolved_count: resolved,
            candidate_count: candidate,
            modifier_chain_count: self.counts.modifier_chain_count,
            slot_lambda_count: self.counts.slot_lambda_count,
        };
    }
}
