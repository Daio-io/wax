//! Stable JSON summary builder for `wax scan --format json-summary` and artifact outputs.

use super::scan::ScanCommandError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use wax_contract::{Diagnostic, DiagnosticSeverity, MergedScan, ScanStatus, SourceLocation};
use wax_core::{AtomicWriteOptions, write_atomically};
use wax_lang_api::normalize_repo_relative_path;

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

/// Raw invocation rollups included in the adoption summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonSummaryRawInvocations {
    /// Total raw UI invocations across match statuses.
    pub total: u32,
    /// Resolved design-system invocations.
    pub resolved: u32,
    /// Local-component invocations.
    pub local: u32,
    /// Candidate design-system invocations.
    pub candidate: u32,
    /// Unresolved invocations.
    pub unresolved: u32,
}

/// Repository adoption headline plus rollups available from current scan facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSummaryAdoption {
    /// Repository invocation adoption ratio when available.
    pub coverage_ratio: Option<f64>,
    /// Adoption-eligible invocation count.
    pub eligible_invocation_count: u32,
    /// Adopted invocation count.
    pub adopted_invocation_count: u32,
    /// Eligible invocations that are not adopted.
    pub non_adopted_invocation_count: u32,
    /// Invocations excluded from primary adoption.
    pub adoption_excluded_invocation_count: u32,
    /// Repository raw-invocation rollups.
    pub raw_invocations: JsonSummaryRawInvocations,
}

/// One diagnostic included in the JSON summary.
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
    /// Repository root as a portable path (`.` for the scanned root).
    pub repo_root: String,
    /// Repo-relative path to the merged scan artifact when possible.
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
        repo_root: ".".to_owned(),
        scan_path: contract_path(repo_root, scan_path),
        snapshot_ids,
        languages,
        adoption: JsonSummaryAdoption {
            coverage_ratio: merged.repo_summary.metrics.invocation_adoption_ratio,
            eligible_invocation_count: merged
                .repo_summary
                .counts
                .adoption
                .eligible_invocation_count,
            adopted_invocation_count: merged.repo_summary.counts.adoption.adopted_invocation_count,
            non_adopted_invocation_count: merged
                .repo_summary
                .counts
                .adoption
                .non_adopted_invocation_count,
            adoption_excluded_invocation_count: merged
                .repo_summary
                .counts
                .adoption
                .adoption_excluded_invocation_count,
            raw_invocations: JsonSummaryRawInvocations {
                total: merged.repo_summary.counts.raw_invocations.total,
                resolved: merged.repo_summary.counts.raw_invocations.resolved,
                local: merged.repo_summary.counts.raw_invocations.local,
                candidate: merged.repo_summary.counts.raw_invocations.candidate,
                unresolved: merged.repo_summary.counts.raw_invocations.unresolved,
            },
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
/// Returns [`ScanCommandError::OutputIo`] when serialization fails, or
/// [`ScanCommandError::AtomicWrite`] when the atomic replace fails.
pub fn write_json_summary(path: &Path, summary: &JsonSummary) -> Result<(), ScanCommandError> {
    let contents =
        serde_json::to_vec_pretty(summary).map_err(|source| ScanCommandError::OutputIo {
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        })?;
    let mut with_newline = contents;
    with_newline.push(b'\n');
    write_atomically(path, &with_newline, AtomicWriteOptions::default())?;
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

/// Whether a diagnostic counts as a scan failure for summaries and `--strict`.
#[must_use]
pub(crate) fn is_failure_diagnostic(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity == DiagnosticSeverity::Error || diagnostic.code == "parse_failed"
}

fn map_location(location: &SourceLocation) -> JsonSummaryLocation {
    JsonSummaryLocation {
        file: location.file.clone(),
        line: location.line,
        column: location.column,
    }
}

/// Portable path label for JSON summary contracts.
fn contract_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .map(normalize_repo_relative_path)
        .unwrap_or_else(|_| path.display().to_string())
}

/// Human/JSON status label shared by stdout summary and json-summary.
#[must_use]
pub fn status_label(status: ScanStatus) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::str::FromStr;
    use time::OffsetDateTime;
    use wax_contract::{
        AdoptionCounts, CountSummary, DefinitionCounts, Diagnostic, DiagnosticSeverity, LanguageId,
        LanguageMetadata, Metrics, ParentScopeCounts, RawInvocationCounts, RegistryCounts,
        RepoSummary, SCHEMA_VERSION, ScanFacts, ScanStatus, SourceLocation,
    };

    #[test]
    fn write_json_summary_preserves_typed_atomic_write_error() {
        let root = std::env::temp_dir().join(format!(
            "wax-cli-atomic-write-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let blocker = root.join("not-a-directory");
        std::fs::write(&blocker, b"file").unwrap();
        let destination = blocker.join("scan-summary.json");

        let summary = JsonSummary {
            schema_version: 1,
            generated_at: "1970-01-01T00:00:00Z".to_owned(),
            repo_root: ".".to_owned(),
            scan_path: ".wax/out/scan-merged.json".to_owned(),
            snapshot_ids: vec![],
            languages: vec![],
            adoption: JsonSummaryAdoption {
                coverage_ratio: None,
                eligible_invocation_count: 0,
                adopted_invocation_count: 0,
                non_adopted_invocation_count: 0,
                adoption_excluded_invocation_count: 0,
                raw_invocations: JsonSummaryRawInvocations {
                    total: 0,
                    resolved: 0,
                    local: 0,
                    candidate: 0,
                    unresolved: 0,
                },
            },
            diagnostics: vec![],
            artifacts: vec![],
            limits: vec![],
        };

        let error =
            write_json_summary(&destination, &summary).expect_err("parent file blocks write");
        let _ = std::fs::remove_dir_all(&root);

        assert!(
            matches!(error, ScanCommandError::AtomicWrite(_)),
            "atomic-write failures must stay typed, got: {error:?}"
        );
    }

    #[test]
    fn json_summary_includes_only_failure_diagnostics() {
        let merged = merged_with_diagnostics(vec![
            diagnostic(
                DiagnosticSeverity::Warning,
                "root_not_found",
                "missing root",
            ),
            diagnostic(
                DiagnosticSeverity::Info,
                "basic_text_scan",
                "heuristic scan",
            ),
            diagnostic(DiagnosticSeverity::Error, "PACK_TIMEOUT", "timed out"),
            diagnostic(DiagnosticSeverity::Warning, "parse_failed", "parse failed"),
        ]);

        let summary = build_json_summary(
            &merged,
            Path::new("/tmp/repo"),
            Path::new("/tmp/repo/.wax/out/scan-merged.json"),
            &[],
        );

        let codes: Vec<&str> = summary
            .diagnostics
            .iter()
            .map(|entry| entry.code.as_str())
            .collect();
        assert_eq!(codes, vec!["PACK_TIMEOUT", "parse_failed"]);
        assert_eq!(summary.repo_root, ".");
        assert_eq!(summary.scan_path, ".wax/out/scan-merged.json");
    }

    #[test]
    fn json_summary_includes_available_adoption_rollups() {
        let mut merged = merged_with_diagnostics(vec![]);
        merged.repo_summary.counts.adoption = AdoptionCounts {
            eligible_invocation_count: 8,
            adopted_invocation_count: 7,
            non_adopted_invocation_count: 1,
            adoption_excluded_invocation_count: 2,
        };
        merged.repo_summary.counts.raw_invocations = RawInvocationCounts {
            total: 10,
            resolved: 7,
            local: 1,
            candidate: 1,
            unresolved: 1,
        };
        merged.repo_summary.metrics.invocation_adoption_ratio = Some(0.875);

        let summary = build_json_summary(
            &merged,
            Path::new("/tmp/repo"),
            Path::new("/tmp/repo/.wax/out/scan-merged.json"),
            &[],
        );

        assert_eq!(summary.adoption.coverage_ratio, Some(0.875));
        assert_eq!(summary.adoption.eligible_invocation_count, 8);
        assert_eq!(summary.adoption.adopted_invocation_count, 7);
        assert_eq!(summary.adoption.non_adopted_invocation_count, 1);
        assert_eq!(summary.adoption.adoption_excluded_invocation_count, 2);
        assert_eq!(summary.adoption.raw_invocations.total, 10);
        assert_eq!(summary.adoption.raw_invocations.resolved, 7);
        assert_eq!(summary.adoption.raw_invocations.candidate, 1);
    }

    fn diagnostic(severity: DiagnosticSeverity, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            severity,
            code: code.to_owned(),
            message: message.to_owned(),
            location: Some(SourceLocation {
                file: "src/a.kt".to_owned(),
                line: 1,
                column: None,
                root_group: None,
            }),
        }
    }

    fn merged_with_diagnostics(diagnostics: Vec<Diagnostic>) -> MergedScan {
        let language_id = LanguageId::from_str("compose").unwrap();
        MergedScan {
            schema_version: SCHEMA_VERSION,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            repo_summary: RepoSummary {
                languages: vec![language_id.clone()],
                counts: CountSummary {
                    registry: RegistryCounts::default(),
                    definitions: DefinitionCounts::default(),
                    raw_invocations: RawInvocationCounts::default(),
                    adoption: AdoptionCounts::default(),
                    parent_scopes: ParentScopeCounts::default(),
                    invocation_origins: Default::default(),
                    tokens: Default::default(),
                },
                metrics: Metrics {
                    invocation_adoption_ratio: None,
                    registry_resolution_ratio: None,
                    parse_extract_ms: 0,
                    files_scanned: 1,
                },
            },
            symbol_usage_summary: vec![],
            token_usage_summary: vec![],
            scan_scope: Default::default(),
            token_inference: wax_contract::TokenInferenceReport::empty(2.0),
            root_groups: vec![],
            root_group_summary: vec![],
            languages: BTreeMap::from([(
                language_id.clone(),
                ScanFacts {
                    schema_version: SCHEMA_VERSION,
                    language: LanguageMetadata {
                        id: language_id,
                        version: "0.1.0".to_owned(),
                        ecosystem: "test".to_owned(),
                        parser_name: "fixture".to_owned(),
                        parser_version: "1.0.0".to_owned(),
                    },
                    snapshot_id: "snap".to_owned(),
                    scanned_at: OffsetDateTime::UNIX_EPOCH,
                    status: ScanStatus::Complete,
                    design_system_components: vec![],
                    local_components: vec![],
                    usage_sites: vec![],
                    diagnostics,
                    metrics: Metrics {
                        invocation_adoption_ratio: None,
                        registry_resolution_ratio: None,
                        parse_extract_ms: 0,
                        files_scanned: 1,
                    },
                    counts: CountSummary::default(),
                    symbol_usage_summary: vec![],
                    design_system_tokens: vec![],
                    token_sites: vec![],
                    hardcoded_style_sites: vec![],
                    token_usage_summary: vec![],
                },
            )]),
        }
    }
}
