//! `wax scan` command implementation.

use super::diagnostic_output::format_diagnostic_line;
use crate::progress::{CliProgress, optional_scan_progress_sink};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use wax_contract::{Diagnostic, DiagnosticSeverity, MergedScan, ScanStatus};
use wax_core::{Engine, EngineError, ScanOptions};

const MAX_FAILURE_DIAGNOSTICS: usize = 5;
const SCAN_OUTPUT_RELATIVE_PATH: &str = ".wax/out/scan-merged.json";

/// Options for `wax scan`.
#[derive(Debug, Clone)]
pub struct ScanCommandOptions {
    /// Repository root containing `.waxrc` and `wax.lock.json`.
    pub repo_root: PathBuf,
    /// Whether missing packs may be auto-installed.
    pub allow_auto_install: bool,
    /// Optional scan concurrency override.
    pub scan_concurrency: Option<u32>,
}

/// Errors returned by `wax scan`.
#[derive(Debug, Error)]
pub enum ScanCommandError {
    /// Engine scan failed.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Summary writing failed.
    #[error("failed to write scan summary: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax scan`.
pub fn run_scan(
    options: ScanCommandOptions,
    writer: &mut impl Write,
) -> Result<(), ScanCommandError> {
    let progress = Arc::new(CliProgress::new());
    let merged = Engine::scan_repo_with_options(
        &options.repo_root,
        ScanOptions {
            scan_concurrency: options.scan_concurrency,
            allow_auto_install: options.allow_auto_install,
            progress: optional_scan_progress_sink(&progress),
        },
    )?;
    progress.finish();

    let output_path = options.repo_root.join(SCAN_OUTPUT_RELATIVE_PATH);
    write_scan_summary(writer, &merged, &output_path)
}

fn write_error(source: io::Error) -> ScanCommandError {
    ScanCommandError::Io { source }
}

fn write_scan_summary(
    writer: &mut impl Write,
    merged: &MergedScan,
    output_path: &Path,
) -> Result<(), ScanCommandError> {
    writeln!(writer, "scan output: {}", output_path.display()).map_err(write_error)?;
    writeln!(writer, "language status:").map_err(write_error)?;
    for (language_id, facts) in &merged.languages {
        write!(writer, "  {language_id}: {}", status_label(facts.status)).map_err(write_error)?;
        if let Some(ratio) = facts.metrics.adoption_coverage_ratio {
            write!(writer, " ({:.1}%)", ratio * 100.0).map_err(write_error)?;
        }
        writeln!(writer).map_err(write_error)?;
    }

    let diagnostics = merged
        .languages
        .values()
        .flat_map(|facts| facts.diagnostics.iter())
        .filter(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error || diagnostic.code == "parse_failed"
        })
        .take(MAX_FAILURE_DIAGNOSTICS)
        .collect::<Vec<_>>();
    write_failure_diagnostics(writer, &diagnostics)
}

fn write_failure_diagnostics(
    writer: &mut impl Write,
    diagnostics: &[&Diagnostic],
) -> Result<(), ScanCommandError> {
    if diagnostics.is_empty() {
        writeln!(writer, "failure diagnostics: none").map_err(write_error)?;
    } else {
        writeln!(
            writer,
            "failure diagnostics (up to {MAX_FAILURE_DIAGNOSTICS}):"
        )
        .map_err(write_error)?;
        for diagnostic in diagnostics {
            writeln!(writer, "  {}", format_diagnostic_line(diagnostic)).map_err(write_error)?;
        }
    }
    Ok(())
}

fn status_label(status: ScanStatus) -> &'static str {
    match status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "partial",
        ScanStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::write_scan_summary;
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use time::OffsetDateTime;
    use wax_contract::{
        CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, MergedScan,
        Metrics, SCHEMA_VERSION, ScanFacts, ScanStatus, SourceLocation,
    };

    #[test]
    fn summary_renders_status_adoption_and_failure_diagnostics() {
        let mut output = Vec::new();
        let merged = MergedScan {
            schema_version: SCHEMA_VERSION,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            languages: BTreeMap::from([
                (
                    LanguageId::from_str("compose").unwrap(),
                    facts_with_status(ScanStatus::Complete, Some(0.875), vec![]),
                ),
                (
                    LanguageId::from_str("react").unwrap(),
                    facts_with_status(
                        ScanStatus::Partial,
                        None,
                        vec![
                            diagnostic(DiagnosticSeverity::Error, "PACK_TIMEOUT", "timed out"),
                            diagnostic(DiagnosticSeverity::Warning, "PACK_WARN", "warn"),
                            Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                code: "parse_failed".to_owned(),
                                message: "failed to parse source file; file skipped".to_owned(),
                                location: Some(SourceLocation {
                                    file: "src/Broken.tsx".to_owned(),
                                    line: 4,
                                    column: Some(12),
                                }),
                            },
                        ],
                    ),
                ),
                (
                    LanguageId::from_str("swift").unwrap(),
                    facts_with_status(
                        ScanStatus::Failed,
                        None,
                        vec![diagnostic(
                            DiagnosticSeverity::Error,
                            "PACK_CRASH",
                            "process exited",
                        )],
                    ),
                ),
            ]),
        };

        write_scan_summary(
            &mut output,
            &merged,
            std::path::Path::new("/tmp/repo/.wax/out/scan-merged.json"),
        )
        .unwrap();

        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains("scan output: /tmp/repo/.wax/out/scan-merged.json"));
        assert!(stdout.contains("compose: complete (87.5%)"));
        assert!(stdout.contains("react: partial"));
        assert!(stdout.contains("swift: failed"));
        assert!(stdout.contains("PACK_TIMEOUT: timed out"));
        assert!(stdout.contains(
            "parse_failed (src/Broken.tsx:4:12): failed to parse source file; file skipped"
        ));
        assert!(stdout.contains("PACK_CRASH: process exited"));
        assert!(!stdout.contains("PACK_WARN: warn"));
    }

    fn facts_with_status(
        status: ScanStatus,
        adoption_coverage_ratio: Option<f64>,
        diagnostics: Vec<Diagnostic>,
    ) -> ScanFacts {
        ScanFacts {
            schema_version: SCHEMA_VERSION,
            language: LanguageMetadata {
                id: LanguageId::from_str("compose").unwrap(),
                version: "0.1.0".to_owned(),
                ecosystem: "test".to_owned(),
                parser_name: "fixture".to_owned(),
                parser_version: "1.0.0".to_owned(),
            },
            snapshot_id: "snap-1".to_owned(),
            scanned_at: OffsetDateTime::UNIX_EPOCH,
            status,
            design_system_components: Vec::new(),
            local_components: Vec::new(),
            usage_sites: Vec::new(),
            diagnostics,
            metrics: Metrics {
                adoption_coverage_ratio,
                parse_extract_ms: 1,
                files_scanned: 1,
            },
            counts: CountSummary {
                design_system_component_count: 0,
                local_component_count: 0,
                usage_site_count: 0,
                resolved_count: 0,
                candidate_count: 0,
            },
        }
    }

    fn diagnostic(severity: DiagnosticSeverity, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            severity,
            code: code.to_owned(),
            message: message.to_owned(),
            location: None,
        }
    }
}
