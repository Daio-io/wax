//! Catalogue entries identify versioned design-system bundles independently of Git snapshots.

use crate::LanguageId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// A versioned collection of immutable system bundle revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Catalogue format version, independent of registry and lock versions.
    pub schema_version: u32,
    /// Entries in ascending `(id, revision)` order.
    pub entries: Vec<CatalogEntry>,
}
/// One revision of an entire design-system bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    /// Lowercase system slug.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Positive bundle revision; all languages advance together.
    pub revision: u64,
    /// Repository-relative immutable bundle directory.
    pub subdir: String,
    /// Minimum compatible engine SemVer.
    pub minimum_wax_version: String,
    /// Publisher definition relative to the bundle directory.
    pub definition: ArtifactDigest,
    /// Language registry artifacts, sorted by language id.
    pub registries: BTreeMap<LanguageId, ArtifactDigest>,
    /// Provenance artifact relative to the bundle directory.
    pub provenance: ArtifactDigest,
    /// Original source version pins, independent of bundle revision.
    pub upstream: Vec<UpstreamPin>,
    /// Bounded descriptive coverage labels.
    pub coverage: Vec<String>,
    /// Bounded descriptive gaps.
    pub gaps: Vec<String>,
}
/// An exact artifact within a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDigest {
    /// Slash-separated relative artifact path.
    pub path: String,
    /// Lowercase SHA-256 of exact bytes.
    pub sha256: String,
}
/// An upstream source and its independently versioned revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamPin {
    /// Original source URL or package identifier.
    pub source: String,
    /// Original version or commit.
    pub version: String,
}
/// Publisher definition pinned alongside every language in a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryDefinitionPin {
    /// Selected design-system slug in the definition.
    pub system_id: String,
    /// Definition path relative to the Git repository.
    pub path: String,
    /// Lowercase SHA-256 of the definition bytes.
    pub sha256: String,
}
/// Invalid catalogue metadata.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid registry catalogue: {0}")]
pub struct CatalogError(pub String);
impl Catalog {
    /// Validates the catalogue contract.
    ///
    /// # Errors
    /// Returns a typed error for invalid versions, ordering, pins, or unbounded metadata.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != 1 {
            return Err(CatalogError("unsupported schema version".into()));
        }
        if self.entries.len() > 10_000 {
            return Err(CatalogError("too many entries".into()));
        }
        let mut previous = None;
        for entry in &self.entries {
            let key = (&entry.id, entry.revision);
            if previous.is_some_and(|previous| previous >= key) {
                return Err(CatalogError(
                    "entries must be unique and sorted by (id, revision)".into(),
                ));
            }
            previous = Some(key);
            if LanguageId::try_from(entry.id.as_str()).is_err()
                || entry.id.len() > 128
                || entry.revision == 0
                || entry.name.trim().is_empty()
                || entry.name.len() > 256
                || semver::Version::parse(&entry.minimum_wax_version).is_err()
                || entry.registries.is_empty()
                || entry.registries.len() > 64
            {
                return Err(CatalogError("invalid bundle metadata".into()));
            }
            validate_artifact_path(&entry.subdir)?;
            entry.definition.validate()?;
            entry.provenance.validate()?;
            for registry in entry.registries.values() {
                registry.validate()?;
            }
            if entry.upstream.len() > 64
                || entry.upstream.iter().any(|pin| {
                    pin.source.is_empty()
                        || pin.source.len() > 2048
                        || pin.version.is_empty()
                        || pin.version.len() > 256
                })
                || [&entry.coverage, &entry.gaps].iter().any(|labels| {
                    labels.len() > 64 || labels.iter().any(|label| label.len() > 2048)
                })
            {
                return Err(CatalogError("metadata exceeds bounds".into()));
            }
        }
        Ok(())
    }
}
impl ArtifactDigest {
    /// Validates the relative artifact path and lowercase SHA-256.
    ///
    /// # Errors
    /// Returns a typed error for unsafe paths or malformed digests.
    pub fn validate(&self) -> Result<(), CatalogError> {
        validate_artifact_path(&self.path)?;
        if !valid_sha256(&self.sha256) {
            return Err(CatalogError("invalid SHA-256".into()));
        }
        Ok(())
    }
}
impl RegistryDefinitionPin {
    /// Validates a definition pin without reading any files.
    ///
    /// # Errors
    /// Returns a typed error for an invalid slug, path, or digest.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if LanguageId::try_from(self.system_id.as_str()).is_err() {
            return Err(CatalogError("invalid system id".into()));
        }
        ArtifactDigest {
            path: self.path.clone(),
            sha256: self.sha256.clone(),
        }
        .validate()
    }
}
/// Whether a digest is exactly 64 lowercase hexadecimal characters.
#[must_use]
pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
/// Validates a bounded slash-separated relative artifact path.
///
/// # Errors
/// Rejects absolute, empty, traversal, control, backslash, and option-like segments.
pub fn validate_artifact_path(path: &str) -> Result<(), CatalogError> {
    if path.len() > 4096
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.starts_with('-')
                || part.contains(':')
        })
    {
        return Err(CatalogError("invalid artifact path".into()));
    }
    Ok(())
}

/// All artifacts selected from one immutable Git design-system bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryBundlePin {
    /// Git repository URL.
    pub repo: String,
    /// Full immutable commit.
    pub commit: String,
    /// Repository-relative bundle directory.
    pub subdir: String,
    /// Publisher selected from the definition.
    pub system_id: String,
    /// Exact publisher definition.
    pub definition: RegistryDefinitionPin,
    /// Optional verified catalogue provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ArtifactDigest>,
    /// All language snapshots belonging to this bundle.
    pub registries: BTreeMap<LanguageId, crate::RegistrySnapshotRef>,
}

impl crate::RegistrySnapshotRef {
    /// Checks typed bundle extension fields without source or cache access.
    ///
    /// # Errors
    /// Rejects partial pins, malformed digests, and mismatched definition identities.
    pub fn validate_bundle_metadata(&self) -> Result<(), CatalogError> {
        let subdir = self.extra.get("subdir");
        let definition = self.extra.get("definition");
        let provenance = self.extra.get("provenance");
        let nested = self.source.starts_with("git-system:");
        if !nested && subdir.is_none() && definition.is_none() && provenance.is_none() {
            return Ok(());
        }
        let subdir = subdir
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CatalogError("bundle snapshot requires subdir".into()))?;
        validate_artifact_path(subdir)?;
        if self
            .git_commit
            .as_deref()
            .is_none_or(|commit| !valid_git_commit(commit))
            || !valid_sha256(&self.digest)
        {
            return Err(CatalogError(
                "invalid bundle snapshot commit or digest".into(),
            ));
        }
        if self.system_id.is_some() || definition.is_some() || provenance.is_some() {
            let pin: RegistryDefinitionPin =
                serde_json::from_value(definition.cloned().unwrap_or_default())
                    .map_err(|error| CatalogError(error.to_string()))?;
            pin.validate()?;
            if self.system_id.as_deref() != Some(&pin.system_id)
                || pin.path != format!("{subdir}/.wax/wax.config.json")
            {
                return Err(CatalogError(
                    "bundle definition does not match snapshot".into(),
                ));
            }
        }
        if let Some(provenance) = provenance {
            let artifact: ArtifactDigest = serde_json::from_value(provenance.clone())
                .map_err(|error| CatalogError(error.to_string()))?;
            artifact.validate()?;
        }
        Ok(())
    }
}
/// Whether an object id is a full lowercase Git SHA-1 or SHA-256 commit id.
#[must_use]
pub fn valid_git_commit(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64)
        && commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl RegistryBundlePin {
    /// Validates agreement between the definition and all language snapshot pins.
    ///
    /// # Errors
    /// Rejects partial, empty, or mixed bundle metadata.
    pub fn validate(&self) -> Result<(), CatalogError> {
        validate_artifact_path(&self.subdir)?;
        self.definition.validate()?;
        if self.repo.trim().is_empty()
            || !valid_git_commit(&self.commit)
            || self.registries.is_empty()
            || self.definition.system_id != self.system_id
            || self.definition.path != format!("{}/.wax/wax.config.json", self.subdir)
        {
            return Err(CatalogError("invalid bundle pin".into()));
        }
        if let Some(provenance) = &self.provenance {
            provenance.validate()?;
        }
        for (language, snapshot) in &self.registries {
            snapshot.validate_bundle_metadata()?;
            if language != &snapshot.language
                || snapshot.git_commit.as_deref() != Some(&self.commit)
                || snapshot.system_id.as_deref() != Some(&self.system_id)
                || snapshot
                    .extra
                    .get("subdir")
                    .and_then(serde_json::Value::as_str)
                    != Some(&self.subdir)
                || snapshot.extra.get("definition") != Some(&serde_json::json!(self.definition))
                || snapshot.extra.get("provenance")
                    != self
                        .provenance
                        .as_ref()
                        .map(|pin| serde_json::json!(pin))
                        .as_ref()
            {
                return Err(CatalogError("bundle snapshots disagree".into()));
            }
        }
        Ok(())
    }
}
