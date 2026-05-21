//! Auto-install policy evaluation for enabled language packs.

use crate::config::lockfile::LockedLanguage;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use wax_contract::LanguageId;

/// Pure inputs required to evaluate language-pack auto-install policy.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoInstallPolicyInput {
    /// Enabled language ids from `.waxrc`.
    pub enabled_language_ids: BTreeSet<LanguageId>,
    /// Locked language-pack entries from `wax.lock.json`.
    pub locked_languages: BTreeMap<LanguageId, LockedLanguage>,
    /// Locally installed manifests by language id.
    pub installed_manifests: BTreeMap<LanguageId, Vec<InstalledManifest>>,
    /// Whether the CLI invocation allows auto-installing missing packs.
    pub allow_auto_install: bool,
    /// Pack-index digest metadata keyed by language id and version.
    pub pack_index_digests: BTreeMap<LanguageId, BTreeMap<String, String>>,
}

/// Minimal installed-manifest metadata used by policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledManifest {
    /// Installed language pack version.
    pub version: String,
}

/// Policy result split into ready, installable, and blocking outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoInstallPolicyDecision {
    /// Enabled languages that are already installed at the locked version.
    pub ready: BTreeSet<LanguageId>,
    /// Install actions required to satisfy enabled locked languages.
    pub needs_install: Vec<InstallPlan>,
    /// Blocking policy errors.
    pub errors: Vec<AutoInstallPolicyError>,
}

/// Install action for one language pack locked by the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Language pack id.
    pub language_id: LanguageId,
    /// Exact lockfile version to install.
    pub version: String,
    /// Exact lockfile digest to verify after download.
    pub sha256: String,
}

/// Typed policy failures produced while evaluating auto-install decisions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AutoInstallPolicyError {
    /// Enabled language id is not present in `wax.lock.json`.
    #[error("enabled language {language_id} is missing from wax.lock.json")]
    MissingLockfileEntry {
        /// Enabled language id.
        language_id: LanguageId,
    },
    /// Locked language is not installed and auto-install was disabled.
    #[error(
        "language {language_id} locked at {version} is not installed and auto-install is disabled"
    )]
    MissingInstalledWithAutoInstallDisabled {
        /// Language pack id.
        language_id: LanguageId,
        /// Locked version that is required locally.
        version: String,
    },
    /// Locked digest differs from the pack-index digest for the same version.
    #[error(
        "language {language_id} locked at {version} has digest drift: lockfile={lockfile_sha256} pack-index={pack_index_sha256}"
    )]
    DigestDrift {
        /// Language pack id.
        language_id: LanguageId,
        /// Locked version being evaluated.
        version: String,
        /// Digest from lockfile.
        lockfile_sha256: String,
        /// Digest from pack index.
        pack_index_sha256: String,
    },
    /// Required lockfile version is not present in the current pack index.
    #[error(
        "language {language_id} locked at {version} is missing from the pack index; refusing auto-install"
    )]
    MissingPackIndexEntry {
        /// Language pack id.
        language_id: LanguageId,
        /// Locked version required for install.
        version: String,
    },
}

/// Evaluates auto-install policy for enabled language packs.
pub fn evaluate_auto_install_policy(input: &AutoInstallPolicyInput) -> AutoInstallPolicyDecision {
    let mut ready = BTreeSet::new();
    let mut needs_install = Vec::new();
    let mut errors = Vec::new();

    for language_id in &input.enabled_language_ids {
        let Some(locked) = input.locked_languages.get(language_id) else {
            errors.push(AutoInstallPolicyError::MissingLockfileEntry {
                language_id: language_id.clone(),
            });
            continue;
        };

        if has_installed_version(&input.installed_manifests, language_id, &locked.version) {
            ready.insert(language_id.clone());
            continue;
        }

        if !input.allow_auto_install {
            errors.push(
                AutoInstallPolicyError::MissingInstalledWithAutoInstallDisabled {
                    language_id: language_id.clone(),
                    version: locked.version.clone(),
                },
            );
            continue;
        }

        let Some(pack_index_sha) =
            lookup_pack_index_digest(&input.pack_index_digests, language_id, &locked.version)
        else {
            errors.push(AutoInstallPolicyError::MissingPackIndexEntry {
                language_id: language_id.clone(),
                version: locked.version.clone(),
            });
            continue;
        };

        if pack_index_sha != locked.resolved.sha256 {
            errors.push(AutoInstallPolicyError::DigestDrift {
                language_id: language_id.clone(),
                version: locked.version.clone(),
                lockfile_sha256: locked.resolved.sha256.clone(),
                pack_index_sha256: pack_index_sha.to_owned(),
            });
            continue;
        }

        needs_install.push(InstallPlan {
            language_id: language_id.clone(),
            version: locked.version.clone(),
            sha256: locked.resolved.sha256.clone(),
        });
    }

    AutoInstallPolicyDecision {
        ready,
        needs_install,
        errors,
    }
}

fn has_installed_version(
    installed_manifests: &BTreeMap<LanguageId, Vec<InstalledManifest>>,
    language_id: &LanguageId,
    version: &str,
) -> bool {
    installed_manifests
        .get(language_id)
        .is_some_and(|manifests| manifests.iter().any(|manifest| manifest.version == version))
}

fn lookup_pack_index_digest<'a>(
    pack_index_digests: &'a BTreeMap<LanguageId, BTreeMap<String, String>>,
    language_id: &LanguageId,
    version: &str,
) -> Option<&'a str> {
    pack_index_digests
        .get(language_id)
        .and_then(|versions| versions.get(version))
        .map(String::as_str)
}
