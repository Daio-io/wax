//! Stable JSON summary builder for `wax scan --format json-summary` and artifact outputs.

use super::scan::ScanCommandError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use wax_contract::{Diagnostic, DiagnosticSeverity, MergedScan, ScanStatus, SourceLocation};
use wax_core::{AtomicWriteOptions, write_atomically};

/// Known gaps when module/category/ownership rollups are unavailable.
pub const SUMMARY_LIMIT_MODULE: &str = "module rollups are not available in current scan facts";
/// Category rollup gap message.
pub const SUMMARY_LIMIT_CATEGORY: &str = "category rollups are not available in current scan facts";
/// Ownership rollup gap message.
pub const SUMMARY_LIMIT_OWNERSHIP: &str =
    "ownership rollups are not available in current scan facts";

/// One written scan artifact recorded in the JSON summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrittenArtifact {
    /// Artifact format id.
    pub format: String,
    /// Destination path as requested or normalized.
    pub path: String,
    /// Byte size when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// Per-language row in the JSON summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSummaryLanguage {
    /// Language pack id.
    pub id: String,
    /// Language pack version.
    pub version: String,
    /// Scan status label.
    pub status: String,
    /// Parser implementation name.
    pub parser: String,
    /// Files scanned for this language.
    pub files_scanned: u32,
    /// Per-language invocation adoption ratio when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_ratio: Option<f64>,
    /// Resolved raw design-system invocations.
    pub resolved: u32,
    /// Candidate raw design-system invocations.
    pub candidate: u32,
}

/// Repository adoption headline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSummaryAdoption {
    /// Repository invocation adoption ratio when available.
    pub coverage_ratio: Option<f64>,
}

/// One failure diagnostic included in the JSON summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonSummaryDiagnostic {
    /// Diagnostic severity.
    pub severity: String,
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Language that emitted the diagnostic.
    pub language: String,
    /// Optional source location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<JsonSummaryLocation>,
}

/// Source location in the JSON summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonSummaryLocation {
    /// Repository-relative file path.
    pub file: String,
    /// One-based line number.
    pub line: u32,
    /// One-based column when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// Stable JSON summary contract for CI and dashboards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSummary {
    /// Summary schema version.
    pub schema_version: u32,
    /// RFC3339 generation timestamp.
    pub generated_at: String,
    /// Absolute or display path of the scanned repository root.
    pub repo_root: String,
    /// Path to the merged scan artifact.
    pub scan_path: String,
    /// Snapshot ids from per-language facts, in language-id order.
    pub snapshot_ids: Vec<String>,
    /// Per-language summary rows in language-id order.
    pub languages: Vec<JsonSummaryLanguage>,
    /// Repository adoption headline.
    pub adoption: JsonSummaryAdoption,
    /// Failure diagnostics mapped from the merged scan.
    pub diagnostics: Vec<JsonSummaryDiagnostic>,
    /// Artifacts written for this scan invocation.
    pub artifacts: Vec<WrittenArtifact>,
    /// Known data-gap warnings.
    pub limits: Vec<String>,
}

/// Builds a schema-version-1 JSON summary from merged scan facts.
#[must_use]
pub fn build_json_summary(
    merged: &MergedScan,
    repo_root: &Path,
    scan_path: &Path,
    artifacts: &[WrittenArtifact],
) -> JsonSummary {
    let languages: Vec<JsonSummaryLanguage> = merged
        .languages
        .iter()
        .map(|(language_id, facts)| JsonSummaryLanguage {
            id: language_id.as_str().to_owned(),
            version: facts.language.version.clone(),
            status: status_label(facts.status).to_owned(),
            parser: facts.language.parser_name.clone(),
            files_scanned: facts.metrics.files_scanned,
            coverage_ratio: facts.metrics.invocation_adoption_ratio,
            resolved: facts.counts.raw_invocations.resolved,
            candidate: facts.counts.raw_invocations.candidate,
        })
        .collect();

    let snapshot_ids: Vec<String> = merged
        .languages
        .values()
        .map(|facts| facts.snapshot_id.clone())
        .collect();

    JsonSummary {
        schema_version: 1,
        generated_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        repo_root: repo_root.display().to_string(),
        scan_path: scan_path.display().to_string(),
        snapshot_ids,
        languages,
        adoption: JsonSummaryAdoption {
            coverage_ratio: merged.repo_summary.metrics.invocation_adoption_ratio,
        },
        diagnostics: failure_diagnostics_with_language(merged),
        artifacts: artifacts.to_vec(),
        limits: vec![
            SUMMARY_LIMIT_MODULE.to_owned(),
            SUMMARY_LIMIT_CATEGORY.to_owned(),
            SUMMARY_LIMIT_OWNERSHIP.to_owned(),
        ],
    }
}

/// Writes a JSON summary atomically to `path`.
///
/// # Errors
///
/// Returns [`ScanCommandError::OutputIo`] when serialization or writing fails.
pub fn write_json_summary(path: &Path, summary: &JsonSummary) -> Result<(), ScanCommandError> {
    let contents =
        serde_json::to_vec_pretty(summary).map_err(|source| ScanCommandError::OutputIo {
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        })?;
    let mut with_newline = contents;
    with_newline.push(b'\n');
    write_atomically(path, &with_newline, AtomicWriteOptions::default()).map_err(|source| {
        ScanCommandError::OutputIo {
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        }
    })?;
    Ok(())
}

fn failure_diagnostics_with_language(merged: &MergedScan) -> Vec<JsonSummaryDiagnostic> {
    merged
        .languages
        .iter()
        .flat_map(|(language_id, facts)| {
            facts
                .diagnostics
                .iter()
                .filter(|diagnostic| is_failure_diagnostic(diagnostic))
                .map(|diagnostic| JsonSummaryDiagnostic {
                    severity: severity_label(diagnostic.severity).to_owned(),
                    code: diagnostic.code.clone(),
                    message: diagnostic.message.clone(),
                    language: language_id.as_str().to_owned(),
                    location: diagnostic.location.as_ref().map(map_location),
                })
        })
        .collect()
}

fn is_failure_diagnostic(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity == DiagnosticSeverity::Error || diagnostic.code == "parse_failed"
}

fn map_location(location: &SourceLocation) -> JsonSummaryLocation {
    JsonSummaryLocation {
        file: location.file.clone(),
        line: location.line,
        column: location.column,
    }
}

fn status_label(status: ScanStatus) -> &'static str {
    match status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "partial",
        ScanStatus::Failed => "failed",
    }
}

fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
    }
}
