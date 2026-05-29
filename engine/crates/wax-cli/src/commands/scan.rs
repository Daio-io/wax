//! `wax scan` command implementation.

use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;
use wax_contract::{DiagnosticSeverity, ScanStatus};
use wax_core::{Engine, EngineError, ScanOptions};

const MAX_ERROR_DIAGNOSTICS: usize = 5;

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
    #[error("failed to write scan output: {source}")]
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
    let merged = Engine::scan_repo_with_options(
        &options.repo_root,
        ScanOptions {
            scan_concurrency: options.scan_concurrency,
            allow_auto_install: options.allow_auto_install,
        },
    )?;

    let output_path = options.repo_root.join(".wax/out/scan-merged.json");
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
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .take(MAX_ERROR_DIAGNOSTICS)
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        writeln!(writer, "error diagnostics: none").map_err(write_error)?;
    } else {
        writeln!(writer, "error diagnostics (up to {MAX_ERROR_DIAGNOSTICS}):")
            .map_err(write_error)?;
        for diagnostic in diagnostics {
            writeln!(writer, "  {}: {}", diagnostic.code, diagnostic.message)
                .map_err(write_error)?;
        }
    }

    Ok(())
}

fn write_error(source: io::Error) -> ScanCommandError {
    ScanCommandError::Io { source }
}

fn status_label(status: ScanStatus) -> &'static str {
    match status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "partial",
        ScanStatus::Failed => "failed",
    }
}
