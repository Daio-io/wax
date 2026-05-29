//! Repository validation rules for `wax validate`.

use crate::config::lockfile::{LockfileError, load_lockfile};
use crate::config::waxrc::{WaxRcError, load_waxrc};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};
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
    /// `.waxrc` could not be loaded.
    #[error(transparent)]
    WaxRc(#[from] WaxRcError),
    /// `wax.lock.json` could not be loaded.
    #[error(transparent)]
    Lockfile(#[from] LockfileError),
    /// Enabled language ids must be unique.
    #[error("invalid .waxrc field {field}: duplicate enabled language id `{language_id}`")]
    DuplicateEnabledLanguageId {
        /// `.waxrc` field path.
        field: String,
        /// Duplicate language id.
        language_id: LanguageId,
    },
    /// Enabled language requires `design_system_registry`.
    #[error("invalid .waxrc field {field}: missing required `design_system_registry`")]
    MissingDesignSystemRegistry {
        /// `.waxrc` field path.
        field: String,
    },
    /// Registry path was not a non-empty repo-relative path.
    #[error("invalid .waxrc field {field}: {reason}")]
    InvalidDesignSystemRegistryPath {
        /// `.waxrc` field path.
        field: String,
        /// Validation reason.
        reason: &'static str,
    },
    /// Registry file could not be read.
    #[error("failed to read design-system registry for {field} from {path}: {source}")]
    RegistryRead {
        /// `.waxrc` field path.
        field: String,
        /// Resolved filesystem path.
        path: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Registry path resolved outside the repository root after canonicalization.
    #[error("invalid .waxrc field {field}: resolved path escapes repository root")]
    RegistryPathEscapesRepo {
        /// `.waxrc` field path.
        field: String,
    },
    /// Registry JSON was malformed.
    #[error("malformed design-system registry JSON for {field} in {path}: {source}")]
    RegistryMalformedJson {
        /// `.waxrc` field path.
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
        /// `.waxrc` field path.
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
        /// `.waxrc` field path.
        field: String,
        /// Resolved filesystem path.
        path: String,
        /// Validation reason.
        reason: &'static str,
    },
}

/// Validates repository-local wax configuration for CI workflows.
pub fn validate_repo(repo_root: impl AsRef<Path>) -> Result<ValidateReport, ValidateError> {
    let repo_root = repo_root.as_ref();
    let canonical_repo_root =
        fs::canonicalize(repo_root).map_err(|source| ValidateError::RegistryRead {
            field: "repo_root".to_owned(),
            path: repo_root.display().to_string(),
            source,
        })?;
    let waxrc = load_waxrc(repo_root.join(".waxrc"))?;

    let enabled = waxrc
        .languages
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.enabled)
        .collect::<Vec<_>>();

    if !enabled.is_empty() {
        load_lockfile(repo_root.join("wax.lock.json"))?;
    }

    let mut seen_ids = BTreeSet::new();
    let mut warnings = Vec::new();

    for (index, entry) in enabled {
        let field = format!("languages[{index}].id");
        if !seen_ids.insert(entry.id.clone()) {
            return Err(ValidateError::DuplicateEnabledLanguageId {
                field,
                language_id: entry.id.clone(),
            });
        }

        let registry_field = format!("languages[{index}].design_system_registry");
        let registry_path = entry
            .extra
            .get("design_system_registry")
            .and_then(Value::as_str)
            .ok_or_else(|| ValidateError::MissingDesignSystemRegistry {
                field: registry_field.clone(),
            })?;

        validate_repo_relative_registry_path(&registry_field, registry_path)?;

        let resolved_registry = repo_root.join(registry_path);
        let canonical_registry =
            canonicalize_registry_path(&registry_field, &resolved_registry, &canonical_repo_root)?;
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
                path: canonical_registry.display().to_string(),
                source,
            }
        })?;

        let Some(obj) = value.as_object() else {
            return Err(ValidateError::RegistryInvalidShape {
                field: registry_field,
                path: canonical_registry.display().to_string(),
                reason: "expected top-level object",
            });
        };

        let Some(schema_version) = obj.get("schema_version").and_then(Value::as_u64) else {
            return Err(ValidateError::RegistryInvalidShape {
                field: format!("languages[{index}].design_system_registry"),
                path: canonical_registry.display().to_string(),
                reason: "missing numeric schema_version",
            });
        };

        if schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(ValidateError::RegistryUnsupportedSchemaVersion {
                field: format!("languages[{index}].design_system_registry"),
                path: canonical_registry.display().to_string(),
                found: schema_version,
                supported: REGISTRY_SCHEMA_VERSION,
            });
        }

        let components_missing_or_empty = match obj.get("components") {
            Some(Value::Array(components)) => components.is_empty(),
            Some(_) => {
                return Err(ValidateError::RegistryInvalidShape {
                    field: format!("languages[{index}].design_system_registry"),
                    path: canonical_registry.display().to_string(),
                    reason: "components must be an array when present",
                });
            }
            None => true,
        };

        if components_missing_or_empty {
            warnings.push(ValidateWarning::EmptyRegistryComponents {
                language_id: entry.id.clone(),
                registry_path: registry_path.to_owned(),
            });
        }
    }

    Ok(ValidateReport { warnings })
}

fn validate_repo_relative_registry_path(field: &str, value: &str) -> Result<(), ValidateError> {
    if value.trim().is_empty() {
        return Err(ValidateError::InvalidDesignSystemRegistryPath {
            field: field.to_owned(),
            reason: "path must be a non-empty string",
        });
    }

    let path = Path::new(value);
    if path.is_absolute() {
        return Err(ValidateError::InvalidDesignSystemRegistryPath {
            field: field.to_owned(),
            reason: "path must be repo-relative (not absolute)",
        });
    }

    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(ValidateError::InvalidDesignSystemRegistryPath {
                field: field.to_owned(),
                reason: "path must not escape the repository root",
            });
        }
    }

    Ok(())
}

fn canonicalize_registry_path(
    field: &str,
    registry_path: &Path,
    canonical_repo_root: &Path,
) -> Result<std::path::PathBuf, ValidateError> {
    let canonical_registry =
        fs::canonicalize(registry_path).map_err(|source| ValidateError::RegistryRead {
            field: field.to_owned(),
            path: registry_path.display().to_string(),
            source,
        })?;

    if !canonical_registry.starts_with(canonical_repo_root) {
        return Err(ValidateError::RegistryPathEscapesRepo {
            field: field.to_owned(),
        });
    }

    Ok(canonical_registry)
}
