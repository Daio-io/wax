//! Repository validation rules for `wax validate`.

use crate::config::lockfile::{LockfileError, load_lockfile};
use crate::config::repo_files::{RepoFileWarning, discover_repo_files};
use crate::config::waxrc::{WaxRcError, load_waxrc};
use crate::registry_source::{
    RegistrySourceInput, resolve_registry_source_allowing_missing_components_with_deprecation,
};
use serde_json::Value;
use std::collections::BTreeSet;
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
    /// Deprecated `design_system_registry` key was used.
    DeprecatedDesignSystemRegistry {
        /// Language this registry belongs to.
        language_id: LanguageId,
        /// Field path.
        field: String,
    },
    /// Legacy config was ignored in favor of centralized config.
    IgnoredLegacyConfig {
        /// Ignored legacy config path.
        path: String,
    },
    /// Legacy lockfile was ignored in favor of centralized lockfile.
    IgnoredLegacyLockfile {
        /// Ignored legacy lockfile path.
        path: String,
    },
    /// Centralized config is in use but the lockfile is still at the legacy path.
    PreferredConfigWithLegacyLockfile {
        /// Preferred config path.
        config_path: String,
        /// Legacy lockfile path in use.
        lockfile_path: String,
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
    /// Registry source resolution failed.
    #[error("invalid .wax config field {field}: {source}")]
    RegistrySource {
        /// Config field path.
        field: String,
        /// Source error.
        #[source]
        source: crate::registry_source::RegistrySourceError,
    },
    /// Enabled language registry is missing from lockfile.
    #[error("enabled language {language_id} registry is missing from wax lockfile")]
    MissingRegistryLock {
        /// Language id.
        language_id: LanguageId,
    },
    /// Enabled language registry source differs from lockfile.
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
    /// Enabled language registry digest differs from lockfile.
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
pub fn validate_repo(repo_root: impl AsRef<Path>) -> Result<ValidateReport, ValidateError> {
    let repo_root = repo_root.as_ref();
    let repo_files = discover_repo_files(repo_root);
    let waxrc = load_waxrc(&repo_files.config_path)?;

    let enabled = waxrc
        .languages
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.enabled)
        .collect::<Vec<_>>();

    let lockfile = if enabled.is_empty() {
        None
    } else {
        Some(load_lockfile(&repo_files.lockfile_path)?)
    };

    let mut seen_ids = BTreeSet::new();
    let mut warnings = Vec::new();

    for (index, entry) in &enabled {
        let field = format!("languages[{index}].id");
        if !seen_ids.insert(entry.id.clone()) {
            return Err(ValidateError::DuplicateEnabledLanguageId {
                field,
                language_id: entry.id.clone(),
            });
        }
    }

    for (index, entry) in enabled {
        let registry_setting = entry.registry_source();
        let registry_field = format!(
            "languages[{index}].{}",
            registry_setting
                .as_ref()
                .map(|setting| setting.field_name)
                .unwrap_or("registry")
        );
        let resolved = resolve_registry_source_allowing_missing_components_with_deprecation(
            RegistrySourceInput {
                repo_root,
                language_id: entry.id.as_str(),
                source: registry_setting
                    .as_ref()
                    .map(|setting| setting.source.as_str()),
            },
            registry_setting
                .as_ref()
                .is_some_and(|setting| setting.deprecated),
        )
        .map_err(|source| ValidateError::RegistrySource {
            field: registry_field.clone(),
            source,
        })?;

        if resolved.deprecated {
            warnings.push(ValidateWarning::DeprecatedDesignSystemRegistry {
                language_id: entry.id.clone(),
                field: registry_field.clone(),
            });
        }

        if let Some(lockfile) = &lockfile {
            let Some(locked_registry) = lockfile.registries.get(&entry.id) else {
                return Err(ValidateError::MissingRegistryLock {
                    language_id: entry.id.clone(),
                });
            };
            if locked_registry.source != resolved.source {
                return Err(ValidateError::RegistrySourceDrift {
                    language_id: entry.id.clone(),
                    lockfile_source: locked_registry.source.clone(),
                    resolved_source: resolved.source,
                });
            }
            if locked_registry.sha256 != resolved.sha256 {
                return Err(ValidateError::RegistryDigestDrift {
                    language_id: entry.id.clone(),
                    lockfile_sha256: locked_registry.sha256.clone(),
                    resolved_sha256: resolved.sha256,
                });
            }
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

    for warning in repo_files.warnings {
        match warning {
            RepoFileWarning::IgnoredLegacyConfig { legacy, .. } => {
                warnings.push(ValidateWarning::IgnoredLegacyConfig {
                    path: legacy.display().to_string(),
                });
            }
            RepoFileWarning::IgnoredLegacyLockfile { legacy, .. } => {
                warnings.push(ValidateWarning::IgnoredLegacyLockfile {
                    path: legacy.display().to_string(),
                });
            }
            RepoFileWarning::PreferredConfigWithLegacyLockfile {
                preferred_config,
                legacy_lockfile,
            } => {
                warnings.push(ValidateWarning::PreferredConfigWithLegacyLockfile {
                    config_path: preferred_config.display().to_string(),
                    lockfile_path: legacy_lockfile.display().to_string(),
                });
            }
        }
    }

    Ok(ValidateReport { warnings })
}
