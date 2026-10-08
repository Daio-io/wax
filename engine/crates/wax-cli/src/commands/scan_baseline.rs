//! Baseline loading and scan-summary delta computation.

use super::scan::ScanCommandError;
use super::scan_summary::{
    JsonSummary, JsonSummaryDiagnostic, JsonSummaryLocation, is_failure_diagnostic,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use wax_contract::MergedScan;

/// Stable diagnostic identity included in baseline changes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DiagnosticRef {
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Language that emitted the diagnostic, when known.
    pub language: Option<String>,
    /// Source location when present; part of the comparison fingerprint.
    ///
    /// Kept beyond the brief's `{ code, message, language }` sketch so the same
    /// code/message at different files are distinct new vs resolved diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<JsonSummaryLocation>,
}

/// Changes between the current scan summary and a baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryDeltas {
    /// Change in repository adoption coverage, or `None` when either side lacks coverage.
    pub adoption_coverage_delta: Option<f64>,
    /// Change in resolved raw invocations.
    pub resolved_delta: i64,
    /// Change in candidate raw invocations.
    pub candidate_delta: i64,
    /// Failure diagnostics present only in the current scan.
    pub new_error_diagnostics: Vec<DiagnosticRef>,
    /// Failure diagnostics present only in the baseline.
    pub resolved_diagnostics: Vec<DiagnosticRef>,
}

/// Normalized values needed to compare either supported baseline format.
#[derive(Debug)]
pub struct BaselineSummary {
    adoption_ratio: Option<f64>,
    resolved: u32,
    candidate: u32,
    diagnostics: BTreeSet<DiagnosticRef>,
}

/// Loads a prior JSON summary or merged scan as a normalized baseline.
///
/// # Errors
///
/// Returns [`ScanCommandError::BaselineIo`] when the file cannot be read and
/// [`ScanCommandError::BaselineUnrecognized`] when its JSON shape is unsupported.
pub fn load_baseline(path: &Path) -> Result<BaselineSummary, ScanCommandError> {
    let contents = fs::read_to_string(path).map_err(|source| ScanCommandError::BaselineIo {
        path: path.to_path_buf(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|_| ScanCommandError::BaselineUnrecognized {
            path: path.to_path_buf(),
        })?;

    if value.get("schema_version").is_some()
        && value.get("scan_path").is_some()
        && value
            .get("languages")
            .is_some_and(serde_json::Value::is_array)
    {
        let summary =
            serde_json::from_value(value).map_err(|_| ScanCommandError::BaselineUnrecognized {
                path: path.to_path_buf(),
            })?;
        Ok(BaselineSummary::from_json_summary(summary))
    } else if value.get("repo_summary").is_some()
        && value
            .get("languages")
            .is_some_and(serde_json::Value::is_object)
    {
        let merged =
            serde_json::from_value(value).map_err(|_| ScanCommandError::BaselineUnrecognized {
                path: path.to_path_buf(),
            })?;
        Ok(BaselineSummary::from_merged(&merged))
    } else {
        Err(ScanCommandError::BaselineUnrecognized {
            path: path.to_path_buf(),
        })
    }
}

/// Computes deterministic metric and diagnostic changes from `baseline` to `current`.
#[must_use]
pub fn compute_deltas(current: &JsonSummary, baseline: &BaselineSummary) -> SummaryDeltas {
    let current_diagnostics =
        diagnostic_set(current.diagnostics.iter().map(diagnostic_ref_from_summary));

    SummaryDeltas {
        adoption_coverage_delta: current
            .adoption
            .coverage_ratio
            .zip(baseline.adoption_ratio)
            .map(|(current, prior)| current - prior),
        resolved_delta: i64::from(current.adoption.raw_invocations.resolved)
            - i64::from(baseline.resolved),
        candidate_delta: i64::from(current.adoption.raw_invocations.candidate)
            - i64::from(baseline.candidate),
        new_error_diagnostics: current_diagnostics
            .difference(&baseline.diagnostics)
            .cloned()
            .collect(),
        resolved_diagnostics: baseline
            .diagnostics
            .difference(&current_diagnostics)
            .cloned()
            .collect(),
    }
}

impl BaselineSummary {
    fn from_json_summary(summary: JsonSummary) -> Self {
        let diagnostics =
            diagnostic_set(summary.diagnostics.iter().map(diagnostic_ref_from_summary));
        Self {
            adoption_ratio: summary.adoption.coverage_ratio,
            resolved: summary.adoption.raw_invocations.resolved,
            candidate: summary.adoption.raw_invocations.candidate,
            diagnostics,
        }
    }

    fn from_merged(merged: &MergedScan) -> Self {
        let diagnostics = diagnostic_set(merged.languages.iter().flat_map(|(language, facts)| {
            facts
                .diagnostics
                .iter()
                .filter(|diagnostic| is_failure_diagnostic(diagnostic))
                .map(|diagnostic| DiagnosticRef {
                    code: diagnostic.code.clone(),
                    message: diagnostic.message.clone(),
                    language: Some(language.as_str().to_owned()),
                    location: diagnostic
                        .location
                        .as_ref()
                        .map(|location| JsonSummaryLocation {
                            file: location.file.clone(),
                            line: location.line,
                            column: location.column,
                        }),
                })
        }));
        Self {
            adoption_ratio: merged.repo_summary.metrics.invocation_adoption_ratio,
            resolved: merged.repo_summary.counts.raw_invocations.resolved,
            candidate: merged.repo_summary.counts.raw_invocations.candidate,
            diagnostics,
        }
    }
}

fn diagnostic_ref_from_summary(diagnostic: &JsonSummaryDiagnostic) -> DiagnosticRef {
    DiagnosticRef {
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        language: Some(diagnostic.language.clone()),
        location: diagnostic.location.clone(),
    }
}

fn diagnostic_set(diagnostics: impl IntoIterator<Item = DiagnosticRef>) -> BTreeSet<DiagnosticRef> {
    diagnostics.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::scan_summary::{
        JsonSummary, JsonSummaryAdoption, JsonSummaryDiagnostic, JsonSummaryLanguage,
        JsonSummaryRawInvocations,
    };
    use std::path::{Path, PathBuf};

    fn sample_summary(
        coverage: Option<f64>,
        resolved: u32,
        candidate: u32,
        diagnostics: Vec<JsonSummaryDiagnostic>,
    ) -> JsonSummary {
        JsonSummary {
            schema_version: crate::commands::scan_summary::JSON_SUMMARY_SCHEMA_VERSION,
            generated_at: "1970-01-01T00:00:00Z".to_owned(),
            repo_root: ".".to_owned(),
            scan_path: ".wax/out/scan-merged.json".to_owned(),
            snapshot_ids: vec![],
            languages: vec![JsonSummaryLanguage {
                id: "compose".to_owned(),
                version: "1.0.0".to_owned(),
                status: "complete".to_owned(),
                parser: "fixture".to_owned(),
                files_scanned: 1,
                coverage_ratio: coverage,
                resolved,
                candidate,
            }],
            adoption: JsonSummaryAdoption {
                coverage_ratio: coverage,
                eligible_invocation_count: resolved + candidate,
                adopted_invocation_count: resolved,
                non_adopted_invocation_count: candidate,
                adoption_excluded_invocation_count: 0,
                raw_invocations: JsonSummaryRawInvocations {
                    total: resolved + candidate,
                    resolved,
                    local: 0,
                    candidate,
                    unresolved: 0,
                },
            },
            diagnostics,
            artifacts: vec![],
            limits: vec![],
            deltas: None,
        }
    }

    fn write_temp(name: &str, body: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wax-cli-baseline-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("temp root");
        let path = root.join(name);
        fs::write(&path, body).expect("write baseline");
        path
    }

    fn cleanup_temp(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn scan_baseline_compute_deltas_reports_metric_and_diagnostic_changes() {
        let baseline = BaselineSummary::from_json_summary(sample_summary(
            Some(0.5),
            0,
            2,
            vec![JsonSummaryDiagnostic {
                severity: "error".to_owned(),
                code: "old_error".to_owned(),
                message: "old failure".to_owned(),
                language: "compose".to_owned(),
                location: None,
            }],
        ));
        let current = sample_summary(
            Some(1.0),
            1,
            1,
            vec![JsonSummaryDiagnostic {
                severity: "error".to_owned(),
                code: "current_error".to_owned(),
                message: "current failure".to_owned(),
                language: "compose".to_owned(),
                location: None,
            }],
        );

        let deltas = compute_deltas(&current, &baseline);

        assert_eq!(deltas.adoption_coverage_delta, Some(0.5));
        assert_eq!(deltas.resolved_delta, 1);
        assert_eq!(deltas.candidate_delta, -1);
        assert_eq!(
            deltas.new_error_diagnostics,
            vec![DiagnosticRef {
                code: "current_error".to_owned(),
                message: "current failure".to_owned(),
                language: Some("compose".to_owned()),
                location: None,
            }]
        );
        assert_eq!(
            deltas.resolved_diagnostics,
            vec![DiagnosticRef {
                code: "old_error".to_owned(),
                message: "old failure".to_owned(),
                language: Some("compose".to_owned()),
                location: None,
            }]
        );
    }

    #[test]
    fn scan_baseline_compute_deltas_distinguishes_same_code_at_different_locations() {
        use crate::commands::scan_summary::JsonSummaryLocation;

        let shared = |file: &str| JsonSummaryDiagnostic {
            severity: "error".to_owned(),
            code: "PARSE_ERROR".to_owned(),
            message: "unexpected token".to_owned(),
            language: "react".to_owned(),
            location: Some(JsonSummaryLocation {
                file: file.to_owned(),
                line: 4,
                column: Some(1),
            }),
        };
        let baseline = BaselineSummary::from_json_summary(sample_summary(
            Some(0.5),
            0,
            1,
            vec![shared("src/A.tsx"), shared("src/B.tsx")],
        ));
        let current = sample_summary(Some(0.5), 0, 1, vec![shared("src/B.tsx")]);

        let deltas = compute_deltas(&current, &baseline);

        assert_eq!(
            deltas.resolved_diagnostics,
            vec![DiagnosticRef {
                code: "PARSE_ERROR".to_owned(),
                message: "unexpected token".to_owned(),
                language: Some("react".to_owned()),
                location: Some(JsonSummaryLocation {
                    file: "src/A.tsx".to_owned(),
                    line: 4,
                    column: Some(1),
                }),
            }]
        );
        assert!(deltas.new_error_diagnostics.is_empty());
    }

    #[test]
    fn scan_baseline_load_baseline_accepts_json_summary() {
        let summary = sample_summary(Some(0.75), 3, 1, vec![]);
        let body = serde_json::to_string_pretty(&summary).expect("serialize summary");
        let path = write_temp("prior-summary.json", &body);

        let baseline = load_baseline(&path).expect("json-summary baseline");
        let current = sample_summary(Some(0.75), 3, 1, vec![]);
        let deltas = compute_deltas(&current, &baseline);
        cleanup_temp(&path);

        assert_eq!(deltas.adoption_coverage_delta, Some(0.0));
        assert_eq!(deltas.resolved_delta, 0);
        assert_eq!(deltas.candidate_delta, 0);
        assert!(deltas.new_error_diagnostics.is_empty());
        assert!(deltas.resolved_diagnostics.is_empty());
    }

    #[test]
    fn scan_baseline_load_baseline_accepts_schema_version_1_summary() {
        let mut summary = sample_summary(Some(0.5), 1, 1, vec![]);
        summary.schema_version = 1;
        let body = serde_json::to_string_pretty(&summary).expect("serialize v1 summary");
        let path = write_temp("prior-v1-summary.json", &body);

        let baseline = load_baseline(&path).expect("v1 json-summary baseline");
        let current = sample_summary(Some(0.75), 2, 0, vec![]);
        let deltas = compute_deltas(&current, &baseline);
        cleanup_temp(&path);

        assert_eq!(deltas.adoption_coverage_delta, Some(0.25));
        assert_eq!(deltas.resolved_delta, 1);
        assert_eq!(deltas.candidate_delta, -1);
    }

    #[test]
    fn scan_baseline_load_baseline_rejects_unrecognized() {
        let path = write_temp("unknown.json", r#"{"not":"a scan"}"#);

        let error = load_baseline(&path).expect_err("unrecognized baseline");
        let matches_path = matches!(
            &error,
            ScanCommandError::BaselineUnrecognized { path: p } if p == &path
        );
        cleanup_temp(&path);
        assert!(matches_path, "got: {error:?}");
    }
}
