//! Baseline loading and scan-summary delta computation.

use super::scan::ScanCommandError;
use super::scan_summary::{JsonSummary, is_failure_diagnostic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
pub struct BaselineSummary {
    adoption_ratio: Option<f64>,
    resolved: u32,
    candidate: u32,
    diagnostics: BTreeMap<(String, String, Option<String>), DiagnosticRef>,
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
        diagnostic_map(current.diagnostics.iter().map(|diagnostic| DiagnosticRef {
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            language: Some(diagnostic.language.clone()),
        }));

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
            .iter()
            .filter(|(fingerprint, _)| !baseline.diagnostics.contains_key(*fingerprint))
            .map(|(_, diagnostic)| diagnostic.clone())
            .collect(),
        resolved_diagnostics: baseline
            .diagnostics
            .iter()
            .filter(|(fingerprint, _)| !current_diagnostics.contains_key(*fingerprint))
            .map(|(_, diagnostic)| diagnostic.clone())
            .collect(),
    }
}

impl BaselineSummary {
    fn from_json_summary(summary: JsonSummary) -> Self {
        let diagnostics =
            diagnostic_map(
                summary
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| DiagnosticRef {
                        code: diagnostic.code,
                        message: diagnostic.message,
                        language: Some(diagnostic.language),
                    }),
            );
        Self {
            adoption_ratio: summary.adoption.coverage_ratio,
            resolved: summary.adoption.raw_invocations.resolved,
            candidate: summary.adoption.raw_invocations.candidate,
            diagnostics,
        }
    }

    fn from_merged(merged: &MergedScan) -> Self {
        let diagnostics = diagnostic_map(merged.languages.iter().flat_map(|(language, facts)| {
            facts
                .diagnostics
                .iter()
                .filter(|diagnostic| is_failure_diagnostic(diagnostic))
                .map(|diagnostic| DiagnosticRef {
                    code: diagnostic.code.clone(),
                    message: diagnostic.message.clone(),
                    language: Some(language.as_str().to_owned()),
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

fn diagnostic_map(
    diagnostics: impl IntoIterator<Item = DiagnosticRef>,
) -> BTreeMap<(String, String, Option<String>), DiagnosticRef> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let fingerprint = (
                diagnostic.code.clone(),
                diagnostic.message.clone(),
                diagnostic.language.clone(),
            );
            (fingerprint, diagnostic)
        })
        .collect()
}
