#![deny(missing_docs)]

//! Core engine functionality for wax.

pub mod auto_install;
pub mod config;
pub mod global_state;
pub mod install;
pub mod paths;
pub mod registry;
pub mod subprocess_lang;

use auto_install::{AutoInstallPolicyInput, InstalledManifest, PackIndexArtifact};
use config::lockfile::{LockfileError, load_lockfile};
use config::waxrc::{WaxRcError, load_waxrc};
use global_state::{GlobalStateError, load_global_state};
use paths::{PathsError, state_file};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subprocess_lang::{
    LanguageError, LanguageExtractor, SubprocessLanguageExtractor, SubprocessLanguageManifest,
};
use thiserror::Error;
use wax_contract::{LanguageId, MergedScan, SCHEMA_VERSION};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};

const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
struct InstalledManifestFile {
    id: LanguageId,
    version: String,
    api_version: u32,
    command: Vec<String>,
    #[serde(default)]
    target: String,
    #[serde(default)]
    sha256: String,
    #[serde(flatten)]
    _extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug)]
struct InstalledPackScanSpec {
    command: Vec<String>,
}

/// Engine scan orchestrator for repository scans.
#[derive(Debug, Default)]
pub struct Engine;

/// Typed failures while resolving and running repository scans.
#[derive(Debug, Error)]
pub enum EngineError {
    /// `.waxrc` could not be loaded from the repository root.
    #[error(transparent)]
    WaxRc(#[from] WaxRcError),
    /// `wax.lock.json` could not be loaded from the repository root.
    #[error(transparent)]
    Lockfile(#[from] LockfileError),
    /// Global wax state could not be loaded.
    #[error(transparent)]
    GlobalState(#[from] GlobalStateError),
    /// Global path resolution failed.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Installed manifest could not be read or parsed.
    #[error("failed to load installed manifest from {path}: {source}")]
    InstalledManifest {
        /// Manifest path.
        path: String,
        /// Underlying source error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Installed manifest id/version did not match expected lockfile values.
    #[error(
        "installed manifest mismatch for language {language_id}: expected version {expected_version}"
    )]
    InstalledManifestMismatch {
        /// Language id being resolved.
        language_id: LanguageId,
        /// Locked language version expected by scan policy.
        expected_version: String,
    },
    /// Registry index could not be loaded while evaluating auto-install policy.
    #[error(transparent)]
    Registry(#[from] registry::RegistryError),
    /// Auto-install policy blocked scan execution.
    #[error("scan auto-install policy blocked execution: {errors:?}")]
    AutoInstallPolicyBlocked {
        /// Typed policy errors returned by evaluation.
        errors: Vec<auto_install::AutoInstallPolicyError>,
    },
    /// Auto-install would be required, but install orchestration is deferred.
    #[error("scan requires auto-install before execution: {plans:?}")]
    AutoInstallRequired {
        /// Required installs returned by policy.
        plans: Vec<auto_install::InstallPlan>,
    },
    /// A language scan subprocess failed.
    #[error(transparent)]
    Language(#[from] LanguageError),
}

impl Engine {
    /// Scans a repository by running enabled language packs serially.
    pub fn scan_repo(repo_root: impl AsRef<Path>) -> Result<MergedScan, EngineError> {
        let repo_root = repo_root.as_ref();
        let waxrc = load_waxrc(repo_root.join(".waxrc"))?;
        let lockfile = load_lockfile(repo_root.join("wax.lock.json"))?;
        let state = load_global_state(state_file()?)?;

        let enabled_ids: BTreeSet<LanguageId> = waxrc
            .languages
            .into_iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.id)
            .collect();

        let installed_manifests = collect_installed_manifests(&enabled_ids, &lockfile, &state)?;
        let policy_without_auto_install =
            auto_install::evaluate_auto_install_policy(&AutoInstallPolicyInput {
                enabled_language_ids: enabled_ids.clone(),
                locked_languages: lockfile.languages.clone(),
                installed_manifests,
                allow_auto_install: false,
                pack_index_artifacts: BTreeMap::new(),
            });

        let mut missing_install_ids = BTreeSet::new();
        for error in &policy_without_auto_install.errors {
            match error {
                auto_install::AutoInstallPolicyError::MissingInstalledWithAutoInstallDisabled {
                    language_id,
                    ..
                } => {
                    missing_install_ids.insert(language_id.clone());
                }
                _ => {
                    return Err(EngineError::AutoInstallPolicyBlocked {
                        errors: policy_without_auto_install.errors,
                    });
                }
            }
        }

        if !missing_install_ids.is_empty() {
            let pack_index_artifacts =
                collect_pack_index_artifacts(&missing_install_ids, &lockfile)?;
            let policy_with_auto_install =
                auto_install::evaluate_auto_install_policy(&AutoInstallPolicyInput {
                    enabled_language_ids: enabled_ids.clone(),
                    locked_languages: lockfile.languages.clone(),
                    installed_manifests: collect_installed_manifests(
                        &enabled_ids,
                        &lockfile,
                        &state,
                    )?,
                    allow_auto_install: true,
                    pack_index_artifacts,
                });

            if !policy_with_auto_install.errors.is_empty() {
                return Err(EngineError::AutoInstallPolicyBlocked {
                    errors: policy_with_auto_install.errors,
                });
            }
            if !policy_with_auto_install.needs_install.is_empty() {
                return Err(EngineError::AutoInstallRequired {
                    plans: policy_with_auto_install.needs_install,
                });
            }
        }

        let mut languages = BTreeMap::new();
        for language_id in enabled_ids {
            let locked = &lockfile.languages[&language_id];
            let pack_spec =
                load_installed_manifest_for_locked(&state, &language_id, &locked.version)?;
            let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
                command: pack_spec.command,
                timeout: DEFAULT_SCAN_TIMEOUT,
            });
            let request = ScanRequest {
                request_type: ScanRequestType::Scan,
                api_version: WIRE_API_VERSION,
                language_id: language_id.clone(),
                repo_root: repo_root.display().to_string(),
                snapshot_id: new_snapshot_id(),
                config: serde_json::Map::new(),
            };
            let facts = extractor.scan(request)?;
            languages.insert(language_id, facts);
        }

        Ok(MergedScan {
            schema_version: SCHEMA_VERSION,
            recorded_at: time::OffsetDateTime::now_utc(),
            languages,
        })
    }
}

fn collect_installed_manifests(
    enabled_ids: &BTreeSet<LanguageId>,
    lockfile: &config::lockfile::WaxLock,
    state: &global_state::GlobalState,
) -> Result<BTreeMap<LanguageId, Vec<InstalledManifest>>, EngineError> {
    let mut by_language = BTreeMap::new();
    for language_id in enabled_ids {
        if !lockfile.languages.contains_key(language_id) {
            continue;
        }
        let Some(locked) = lockfile.languages.get(language_id) else {
            continue;
        };
        let Some(pack) = state
            .installed_languages
            .get(language_id)
            .and_then(|versions| versions.get(&locked.version))
        else {
            continue;
        };

        let manifest = load_manifest_file(pack.install_dir.join("manifest.json"))?;
        if manifest.id != *language_id || manifest.version != locked.version {
            continue;
        }
        by_language.insert(
            language_id.clone(),
            vec![InstalledManifest {
                version: locked.version.clone(),
                api_version: manifest.api_version,
                target: manifest.target.clone(),
                sha256: manifest.sha256.clone(),
            }],
        );
    }
    Ok(by_language)
}

fn collect_pack_index_artifacts(
    enabled_ids: &BTreeSet<LanguageId>,
    lockfile: &config::lockfile::WaxLock,
) -> Result<BTreeMap<LanguageId, BTreeMap<String, Vec<PackIndexArtifact>>>, EngineError> {
    let mut out = BTreeMap::new();
    for language_id in enabled_ids {
        let Some(locked) = lockfile.languages.get(language_id) else {
            continue;
        };
        let manifests = registry::fetch_pack_index(&locked.source)?;
        for manifest in manifests {
            if manifest.id != *language_id {
                continue;
            }
            let entries = manifest
                .targets
                .into_iter()
                .map(|(target, artifact)| PackIndexArtifact {
                    target,
                    sha256: artifact.sha256,
                })
                .collect::<Vec<_>>();
            out.entry(language_id.clone())
                .or_insert_with(BTreeMap::new)
                .insert(manifest.version, entries);
        }
    }
    Ok(out)
}

fn load_installed_manifest_for_locked(
    state: &global_state::GlobalState,
    language_id: &LanguageId,
    expected_version: &str,
) -> Result<InstalledPackScanSpec, EngineError> {
    let pack = state
        .installed_languages
        .get(language_id)
        .and_then(|versions| versions.get(expected_version))
        .ok_or_else(|| EngineError::InstalledManifestMismatch {
            language_id: language_id.clone(),
            expected_version: expected_version.to_owned(),
        })?;
    let manifest = load_manifest_file(pack.install_dir.join("manifest.json"))?;
    if manifest.id != *language_id || manifest.version != expected_version {
        return Err(EngineError::InstalledManifestMismatch {
            language_id: language_id.clone(),
            expected_version: expected_version.to_owned(),
        });
    }
    Ok(InstalledPackScanSpec {
        command: resolve_manifest_command(pack.install_dir.as_path(), manifest.command),
    })
}

fn resolve_manifest_command(install_dir: &Path, mut command: Vec<String>) -> Vec<String> {
    if let Some(primary) = command.first_mut()
        && let Some(relative) = primary.strip_prefix("./")
    {
        let resolved = install_dir.join(relative);
        *primary = resolved.display().to_string();
    }
    command
}

fn load_manifest_file(path: PathBuf) -> Result<InstalledManifestFile, EngineError> {
    let path_display = path.display().to_string();
    let raw = fs::read_to_string(&path).map_err(|source| EngineError::InstalledManifest {
        path: path_display.clone(),
        source: Box::new(source),
    })?;
    let parsed = serde_json::from_str::<InstalledManifestFile>(&raw).map_err(|source| {
        EngineError::InstalledManifest {
            path: path_display,
            source: Box::new(source),
        }
    })?;
    Ok(parsed)
}

fn new_snapshot_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("scan-{nanos}")
}
