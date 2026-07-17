//! Repository validation rules for `wax validate`.

use crate::config::lockfile::{LockfileError, load_lockfile};
use crate::config::repo_files::discover_repo_files;
use crate::config::waxrc::{WaxRcError, load_waxrc};
use crate::progress::{ValidateProgress, ValidateProgressEvent};
use crate::registry_lock::{self, RegistryLockMismatch};
use crate::registry_source::{
    RegistrySourceInput, resolve_registry_source_allowing_missing_components,
};
use serde_json::Value;
use std::fs;
use std::path::Path;
use thiserror::Error;
use wax_contract::LanguageId;

const REGISTRY_SCHEMA_VERSION: u64 = 1;

/// Validation output for `validate_repo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateReport {
    /// Non-fatal warnings discovered while validating.
    pub warnings: Vec<ValidateWarning>,
}

/// Non-fatal warnings that should be shown to users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateWarning {
    /// Registry parsed but did not declare components.
    EmptyRegistryComponents {
        /// Language this registry belongs to.
        language_id: LanguageId,
        /// Registry path relative to repo root.
        registry_path: String,
    },
}

/// Typed validation failures with machine-friendly field paths.
#[derive(Debug, Error)]
pub enum ValidateError {
    /// Wax config could not be loaded.
    #[error(transparent)]
    WaxRc(#[from] WaxRcError),
    /// `wax.lock.json` could not be loaded.
    #[error(transparent)]
    Lockfile(#[from] LockfileError),
    /// Registry file could not be read.
    #[error("failed to read design-system registry for {field} from {path}: {source}")]
    RegistryRead {
        /// Config field path.
        field: String,
        /// Resolved filesystem path.
        path: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Registry JSON was malformed.
    #[error("malformed design-system registry JSON for {field} in {path}: {source}")]
    RegistryMalformedJson {
        /// Config field path.
        field: String,
        /// Resolved filesystem path.
        path: String,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// Registry schema version was unsupported.
    #[error(
        "unsupported design-system registry schema_version {found} for {field} in {path}; engine supports {supported}"
    )]
    RegistryUnsupportedSchemaVersion {
        /// Config field path.
        field: String,
        /// Resolved filesystem path.
        path: String,
        /// Found schema version.
        found: u64,
        /// Supported schema version.
        supported: u64,
    },
    /// Registry JSON shape was invalid.
    #[error("invalid design-system registry for {field} in {path}: {reason}")]
    RegistryInvalidShape {
        /// Config field path.
        field: String,
        /// Resolved filesystem path.
        path: String,
        /// Validation reason.
        reason: &'static str,
    },
    /// Registry source resolution failed.
    #[error("invalid .wax config field {field}: {source}")]
    RegistrySource {
        /// Config field path.
        field: String,
        /// Source error.
        #[source]
        source: crate::registry_source::RegistrySourceError,
    },
    /// Configured language registry is missing from lockfile.
    #[error("enabled language {language_id} registry is missing from wax lockfile")]
    MissingRegistryLock {
        /// Language id.
        language_id: LanguageId,
    },
    /// Configured language registry source differs from lockfile.
    #[error(
        "enabled language {language_id} registry source drift: lockfile={lockfile_source} resolved={resolved_source}"
    )]
    RegistrySourceDrift {
        /// Language id.
        language_id: LanguageId,
        /// Lockfile source.
        lockfile_source: String,
        /// Resolved source.
        resolved_source: String,
    },
    /// Configured language registry digest differs from lockfile.
    #[error(
        "enabled language {language_id} registry digest drift: lockfile={lockfile_sha256} resolved={resolved_sha256}"
    )]
    RegistryDigestDrift {
        /// Language id.
        language_id: LanguageId,
        /// Lockfile digest.
        lockfile_sha256: String,
        /// Resolved digest.
        resolved_sha256: String,
    },
}

/// Validates repository-local wax configuration for CI workflows.
///
/// # Errors
///
/// Returns [`ValidateError::WaxRc`] or [`ValidateError::Lockfile`] for invalid
/// repository inputs; [`ValidateError::RegistrySource`] when a registry source
/// cannot be resolved; [`ValidateError::RegistryRead`],
/// [`ValidateError::RegistryMalformedJson`],
/// [`ValidateError::RegistryUnsupportedSchemaVersion`], or
/// [`ValidateError::RegistryInvalidShape`] for invalid registry content; and
/// [`ValidateError::MissingRegistryLock`],
/// [`ValidateError::RegistrySourceDrift`], or
/// [`ValidateError::RegistryDigestDrift`] for a missing or stale registry lock.
pub fn validate_repo(repo_root: impl AsRef<Path>) -> Result<ValidateReport, ValidateError> {
    validate_repo_with_progress(repo_root, ValidateProgress::default())
}

/// Validates repository-local wax configuration, emitting optional progress events.
///
/// # Errors
///
/// Returns [`ValidateError::WaxRc`] or [`ValidateError::Lockfile`] for invalid
/// repository inputs, [`ValidateError::RegistrySource`] for registry resolution
/// failures, and [`ValidateError::RegistryRead`],
/// [`ValidateError::RegistryMalformedJson`],
/// [`ValidateError::RegistryUnsupportedSchemaVersion`],
/// [`ValidateError::RegistryInvalidShape`],
/// [`ValidateError::MissingRegistryLock`],
/// [`ValidateError::RegistrySourceDrift`], or
/// [`ValidateError::RegistryDigestDrift`] for registry validation failures.
///
/// # Panics
///
/// Panics if the callback configured in `progress` panics while handling a
/// validation progress event.
pub fn validate_repo_with_progress(
    repo_root: impl AsRef<Path>,
    progress: ValidateProgress,
) -> Result<ValidateReport, ValidateError> {
    let repo_root = repo_root.as_ref();
    progress.emit(ValidateProgressEvent::LoadingConfig);
    let repo_files = discover_repo_files(repo_root);
    let waxrc = load_waxrc(&repo_files.config_path)?;

    let lockfile = if waxrc.languages.is_empty() {
        None
    } else {
        Some(load_lockfile(&repo_files.lockfile_path)?)
    };

    let mut warnings = Vec::new();

    for entry in &waxrc.languages {
        progress.emit(ValidateProgressEvent::ValidatingLanguage {
            language_id: entry.id.clone(),
        });
        let registry_field = format!("languages.{}.registry", entry.id.as_str());
        let resolved = resolve_registry_source_allowing_missing_components(RegistrySourceInput {
            repo_root,
            language_id: entry.id.as_str(),
            source: entry
                .registry_source
                .as_ref()
                .map(|setting| setting.source.as_str()),
        })
        .map_err(|source| ValidateError::RegistrySource {
            field: registry_field.clone(),
            source,
        })?;

        if let Some(lockfile) = &lockfile {
            registry_lock::verify_registry_lock(&entry.id, &resolved, lockfile)
                .map_err(registry_lock_mismatch_to_validate_error)?;
        }

        let resolved_registry = repo_root.join(&resolved.repo_relative_path);
        let contents = fs::read_to_string(&resolved_registry).map_err(|source| {
            ValidateError::RegistryRead {
                field: registry_field.clone(),
                path: resolved_registry.display().to_string(),
                source,
            }
        })?;
        let value: Value = serde_json::from_str(&contents).map_err(|source| {
            ValidateError::RegistryMalformedJson {
                field: registry_field.clone(),
                path: resolved_registry.display().to_string(),
                source,
            }
        })?;

        let Some(obj) = value.as_object() else {
            return Err(ValidateError::RegistryInvalidShape {
                field: registry_field,
                path: resolved_registry.display().to_string(),
                reason: "expected top-level object",
            });
        };

        let Some(schema_version) = obj.get("schema_version").and_then(Value::as_u64) else {
            return Err(ValidateError::RegistryInvalidShape {
                field: registry_field,
                path: resolved_registry.display().to_string(),
                reason: "missing numeric schema_version",
            });
        };

        if schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(ValidateError::RegistryUnsupportedSchemaVersion {
                field: registry_field,
                path: resolved_registry.display().to_string(),
                found: schema_version,
                supported: REGISTRY_SCHEMA_VERSION,
            });
        }

        let components_missing_or_empty = match obj.get("components") {
            Some(Value::Array(components)) => components.is_empty(),
            Some(_) => {
                return Err(ValidateError::RegistryInvalidShape {
                    field: registry_field,
                    path: resolved_registry.display().to_string(),
                    reason: "components must be an array when present",
                });
            }
            None => true,
        };

        if components_missing_or_empty {
            warnings.push(ValidateWarning::EmptyRegistryComponents {
                language_id: entry.id.clone(),
                registry_path: resolved.repo_relative_path,
            });
        }
    }

    Ok(ValidateReport { warnings })
}

fn registry_lock_mismatch_to_validate_error(mismatch: RegistryLockMismatch) -> ValidateError {
    match mismatch {
        RegistryLockMismatch::Missing { language_id } => {
            ValidateError::MissingRegistryLock { language_id }
        }
        RegistryLockMismatch::SourceDrift {
            language_id,
            lockfile_source,
            resolved_source,
        } => ValidateError::RegistrySourceDrift {
            language_id,
            lockfile_source,
            resolved_source,
        },
        RegistryLockMismatch::DigestDrift {
            language_id,
            lockfile_sha256,
            resolved_sha256,
        } => ValidateError::RegistryDigestDrift {
            language_id,
            lockfile_sha256,
            resolved_sha256,
        },
    }
}
