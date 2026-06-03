//! Shared design-system registry lock checks for validate and scan.

use crate::config::lockfile::WaxLock;
use crate::registry_source::ResolvedRegistrySource;
use wax_contract::LanguageId;

/// Registry lock mismatch discovered while comparing lockfile to resolved source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryLockMismatch {
    /// Enabled language has no `registries` entry in the lockfile.
    Missing {
        /// Language id.
        language_id: LanguageId,
    },
    /// Locked source string differs from the resolved registry source.
    SourceDrift {
        /// Language id.
        language_id: LanguageId,
        /// Source recorded in the lockfile.
        lockfile_source: String,
        /// Source resolved from current config.
        resolved_source: String,
    },
    /// Locked digest differs from the resolved registry content.
    DigestDrift {
        /// Language id.
        language_id: LanguageId,
        /// Digest recorded in the lockfile.
        lockfile_sha256: String,
        /// Digest resolved from current registry bytes.
        resolved_sha256: String,
    },
}

/// Verifies that `lockfile` registry locks match `resolved` for `language_id`.
pub fn verify_registry_lock(
    language_id: &LanguageId,
    resolved: &ResolvedRegistrySource,
    lockfile: &WaxLock,
) -> Result<(), RegistryLockMismatch> {
    let locked =
        lockfile
            .registries
            .get(language_id)
            .ok_or_else(|| RegistryLockMismatch::Missing {
                language_id: language_id.clone(),
            })?;

    if locked.source != resolved.source {
        return Err(RegistryLockMismatch::SourceDrift {
            language_id: language_id.clone(),
            lockfile_source: locked.source.clone(),
            resolved_source: resolved.source.clone(),
        });
    }

    if locked.sha256 != resolved.sha256 {
        return Err(RegistryLockMismatch::DigestDrift {
            language_id: language_id.clone(),
            lockfile_sha256: locked.sha256.clone(),
            resolved_sha256: resolved.sha256.clone(),
        });
    }

    Ok(())
}
