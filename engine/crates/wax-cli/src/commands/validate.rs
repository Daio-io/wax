//! `wax validate` command implementation.

use crate::progress::{CliProgress, validate_progress_sink};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use wax_core::validate::{ValidateError, ValidateWarning, validate_repo_with_progress};

/// Options for `wax validate`.
#[derive(Debug, Clone)]
pub struct ValidateCommandOptions {
    /// Repository root containing `.wax/wax.config.json` (or legacy `.waxrc`) and lockfile inputs.
    pub repo_root: PathBuf,
}

/// Errors returned by `wax validate`.
#[derive(Debug, Error)]
pub enum ValidateCommandError {
    /// Repo validation failed.
    #[error(transparent)]
    Validate(#[from] ValidateError),
    /// Output writing failed.
    #[error("failed to write validate output: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax validate`.
pub fn run_validate(
    options: ValidateCommandOptions,
    writer: &mut impl Write,
) -> Result<(), ValidateCommandError> {
    let progress = Arc::new(CliProgress::new());
    let report = validate_repo_with_progress(
        &options.repo_root,
        validate_progress_sink(Arc::clone(&progress)),
    )?;
    progress.finish();

    for warning in report.warnings {
        match warning {
            ValidateWarning::EmptyRegistryComponents {
                language_id,
                registry_path,
            } => {
                eprintln!(
                    "warning: `{language_id}` registry `{registry_path}` has no components; adoption metrics stay empty until the registry is populated"
                );
            }
            ValidateWarning::DeprecatedDesignSystemRegistry { language_id, field } => {
                eprintln!(
                    "warning: language {language_id} uses deprecated {field}; use registry instead"
                );
            }
            ValidateWarning::IgnoredLegacyConfig { path } => {
                eprintln!("warning: ignored legacy config {path}");
            }
            ValidateWarning::IgnoredLegacyLockfile { path } => {
                eprintln!("warning: ignored legacy lockfile {path}");
            }
            ValidateWarning::PreferredConfigWithLegacyLockfile {
                config_path,
                lockfile_path,
            } => {
                eprintln!(
                    "warning: using legacy lockfile {lockfile_path} with centralized config {config_path}; migrate lockfile to .wax/wax.lock.json"
                );
            }
        }
    }

    writeln!(writer, "validation passed").map_err(|source| ValidateCommandError::Io { source })?;
    Ok(())
}
