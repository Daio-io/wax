//! `wax validate` command implementation.

use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;
use wax_core::validate::{ValidateError, ValidateWarning, validate_repo};

/// Options for `wax validate`.
#[derive(Debug, Clone)]
pub struct ValidateCommandOptions {
    /// Repository root containing `.waxrc` and repo-local inputs.
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
    let report = validate_repo(&options.repo_root)?;

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
        }
    }

    writeln!(writer, "validation passed").map_err(|source| ValidateCommandError::Io { source })?;
    Ok(())
}
