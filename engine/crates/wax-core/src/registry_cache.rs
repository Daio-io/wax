//! Immutable design-system registry snapshots cached by source identity.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Map;
use thiserror::Error;
use wax_contract::{
    LanguageId, RegistryDocument, RegistryError, RegistrySnapshotRef, registry_from_json,
};

use crate::advisory_lock::AdvisoryLock;
use crate::atomic_file::write_atomically_no_clobber;
use crate::config::waxrc::{WaxRcError, load_waxrc};
use crate::paths::{PathsError, registry_cache_dir};
use crate::registry_git::{RegistryGitError, fetch_git_registry, fetch_git_registry_at_commit};
use crate::registry_source::{
    RegistrySourceError, fetch_http_bytes, file_url_to_path,
    validate_repo_relative_registry_path_within_repo,
};
use crate::{AtomicWriteError, AtomicWriteOptions, write_atomically};

/// A registry source that can be resolved into one or more immutable snapshots.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegistrySourceSpec {
    /// One local registry JSON file.
    LocalFile {
        /// Filesystem path to the registry JSON.
        path: PathBuf,
        /// Language represented by the registry when the artifact does not declare it.
        language: Option<LanguageId>,
    },
    /// A publisher repository containing wax publication config or conventional registries.
    PublisherRepository {
        /// Filesystem path to the publisher repository.
        root: PathBuf,
        /// Optional publisher id used to select one configured design system.
        system_id: Option<String>,
        /// Optional language filter.
        language: Option<LanguageId>,
    },
    /// One registry JSON document fetched over HTTP.
    HttpJson {
        /// HTTP or HTTPS URL.
        url: String,
        /// Language represented by the registry when the artifact does not declare it.
        language: Option<LanguageId>,
    },
    /// A Git repository containing conventional registry files.
    Git {
        /// Git remote URL.
        url: String,
        /// Optional tag or commit. An omitted tag selects the default branch once.
        tag: Option<String>,
        /// Language whose conventional registry file should be read.
        language: LanguageId,
    },
}

/// Controls whether resolution may fetch and whether it must select an exact snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryReadPolicy {
    /// Refuse source I/O and use only an already cached snapshot.
    pub offline: bool,
    /// Exact snapshot identity required by the caller.
    pub expected_snapshot: Option<RegistrySnapshotRef>,
}

/// A validated registry document together with its immutable identity.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistrySnapshot {
    /// Source, language, digest, and optional Git or publisher context.
    pub reference: RegistrySnapshotRef,
    /// Parsed and validated registry document.
    pub document: RegistryDocument,
}

/// Cache for immutable registry blobs and selected source records.
#[derive(Debug, Clone)]
pub struct RegistryCache {
    root: PathBuf,
}

impl RegistryCache {
    /// Opens a registry cache rooted at `root`.
    ///
    /// Production callers should pass [`crate::paths::registry_cache_dir`]
    /// (`~/.wax/cache/registries`) or use [`Self::open_default`].
    #[must_use]
    pub fn open(root: PathBuf) -> Self {
        Self { root }
    }

    /// Opens the process-global registry cache under [`registry_cache_dir`].
    ///
    /// # Errors
    ///
    /// Returns [`PathsError::HomeUnavailable`] when the wax home cannot be resolved.
    pub fn open_default() -> Result<Self, PathsError> {
        Ok(Self::open(registry_cache_dir()?))
    }

    /// Returns the cache root that holds `blobs/` and `sources/`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a source from its selected cache record or fetches it when allowed.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryCacheError::OfflineCacheMiss`] when offline resolution has
    /// no matching cached selection.
    pub fn resolve(
        &self,
        source: &RegistrySourceSpec,
        policy: &RegistryReadPolicy,
    ) -> Result<Vec<RegistrySnapshot>, RegistryCacheError> {
        let identity = source.identity();
        let selection_identity = source.selection_identity();
        if let Some(expected) = &policy.expected_snapshot {
            self.validate_expected_source(source, expected)?;
            match self.load_references(std::slice::from_ref(expected)) {
                Ok(snapshot) => return Ok(snapshot),
                Err(RegistryCacheError::SnapshotUnavailable { .. }) if !policy.offline => {
                    let mut fetched = self.fetch_expected(source, expected)?;
                    if fetched.len() == 1
                        && fetched[0].reference.source == expected.source
                        && fetched[0].reference.language == expected.language
                        && fetched[0].reference.digest == expected.digest
                        && fetched[0].reference.git_commit == expected.git_commit
                    {
                        fetched[0].reference = expected.clone();
                        return Ok(fetched);
                    }
                    return Err(RegistryCacheError::SnapshotMismatch {
                        expected: Box::new(expected.clone()),
                        requested: source.identity(),
                    });
                }
                Err(error) => return Err(error),
            }
        }
        if let Some(references) = self.load_selection(&selection_identity)? {
            return self.load_references(&references);
        }
        if policy.offline {
            return Err(RegistryCacheError::OfflineCacheMiss { identity });
        }
        let _lock = self.acquire_selection_lock(&selection_identity)?;
        if let Some(references) = self.load_selection(&selection_identity)? {
            return self.load_references(&references);
        }
        self.fetch_and_select(source, &selection_identity)
    }

    fn fetch_expected(
        &self,
        source: &RegistrySourceSpec,
        expected: &RegistrySnapshotRef,
    ) -> Result<Vec<RegistrySnapshot>, RegistryCacheError> {
        if let RegistrySourceSpec::Git { url, language, .. } = source
            && let Some(commit) = expected.git_commit.as_deref()
        {
            let fetched =
                fetch_git_registry_at_commit(url, commit, language, &self.root.join("git"))?;
            return Ok(vec![self.validate_and_store(FetchedRegistry {
                source: source.identity(),
                language: Some(language.clone()),
                system_id: expected.system_id.clone(),
                git_commit: Some(fetched.commit),
                bytes: fetched.bytes,
            })?]);
        }
        self.refresh(source)
    }

    /// Fetches and validates a source before selecting its new immutable snapshots.
    ///
    /// # Errors
    ///
    /// Returns a typed cache error when source resolution or validation fails.
    pub fn refresh(
        &self,
        source: &RegistrySourceSpec,
    ) -> Result<Vec<RegistrySnapshot>, RegistryCacheError> {
        let selection_identity = source.selection_identity();
        let _lock = self.acquire_selection_lock(&selection_identity)?;
        self.fetch_and_select(source, &selection_identity)
    }

    fn acquire_selection_lock(
        &self,
        selection_identity: &str,
    ) -> Result<AdvisoryLock, RegistryCacheError> {
        let path = self.selection_lock_path(selection_identity);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| RegistryCacheError::CacheLock {
                path: path.clone(),
                source,
            })?;
        }
        AdvisoryLock::acquire(&path)
            .map_err(|source| RegistryCacheError::CacheLock { path, source })
    }

    fn fetch_and_select(
        &self,
        source: &RegistrySourceSpec,
        selection_identity: &str,
    ) -> Result<Vec<RegistrySnapshot>, RegistryCacheError> {
        let fetched = match source {
            RegistrySourceSpec::HttpJson { url, language } => {
                let bytes = fetch_http_registry_bytes(url)?;
                vec![FetchedRegistry {
                    source: redact_url(url),
                    language: language.clone(),
                    system_id: None,
                    git_commit: None,
                    bytes,
                }]
            }
            RegistrySourceSpec::LocalFile { path, language } => {
                let bytes = fs::read(path).map_err(|source| RegistryCacheError::ReadSource {
                    path: path.to_owned(),
                    source,
                })?;
                vec![FetchedRegistry {
                    source: local_file_identity(path),
                    language: language.clone(),
                    system_id: None,
                    git_commit: None,
                    bytes,
                }]
            }
            RegistrySourceSpec::PublisherRepository {
                root,
                system_id,
                language,
            } => self.fetch_publisher(root, system_id.as_deref(), language.as_ref())?,
            RegistrySourceSpec::Git { url, tag, language } => {
                let reference = tag.as_deref().unwrap_or("HEAD");
                let fetched = fetch_git_registry(url, reference, language, &self.root.join("git"))?;
                vec![FetchedRegistry {
                    source: source.identity(),
                    language: Some(language.clone()),
                    system_id: None,
                    git_commit: Some(fetched.commit),
                    bytes: fetched.bytes,
                }]
            }
        };

        let snapshots = fetched
            .into_iter()
            .map(|fetched| self.validate_and_store(fetched))
            .collect::<Result<Vec<_>, _>>()?;
        self.write_selection(
            selection_identity,
            snapshots
                .iter()
                .map(|snapshot| snapshot.reference.clone())
                .collect(),
        )?;
        Ok(snapshots)
    }

    fn fetch_publisher(
        &self,
        root: &Path,
        requested_system: Option<&str>,
        requested_language: Option<&LanguageId>,
    ) -> Result<Vec<FetchedRegistry>, RegistryCacheError> {
        let config_path = root.join(".wax/wax.config.json");
        if config_path.is_file() {
            let config = load_waxrc(&config_path)?;
            if !config.design_systems.is_empty() {
                let (system_id, system) = match requested_system {
                    Some(id) => config.design_systems.get_key_value(id).ok_or_else(|| {
                        RegistryCacheError::PublisherNotFound {
                            root: root.to_owned(),
                            system_id: id.to_owned(),
                        }
                    })?,
                    None if config.design_systems.len() == 1 => config
                        .design_systems
                        .first_key_value()
                        .expect("one publisher was checked"),
                    None => {
                        return Err(RegistryCacheError::AmbiguousPublisher {
                            root: root.to_owned(),
                        });
                    }
                };
                let registries: Vec<_> = match requested_language {
                    Some(language) => vec![(
                        language,
                        system.registries.get(language).ok_or_else(|| {
                            RegistryCacheError::MissingPublishedRegistry {
                                root: root.to_owned(),
                                system_id: Some(system_id.clone()),
                                language: language.clone(),
                            }
                        })?,
                    )],
                    None => system.registries.iter().collect(),
                };
                if registries.is_empty() {
                    return Err(RegistryCacheError::PublisherHasNoRegistries {
                        root: root.to_owned(),
                        system_id: system_id.clone(),
                    });
                }
                return registries
                    .into_iter()
                    .map(|(language, registry)| {
                        let bytes = read_publisher_mapping(root, &registry.source)?;
                        Ok(FetchedRegistry {
                            source: format!("publisher:{}#{system_id}/{language}", root.display()),
                            language: Some(language.clone()),
                            system_id: Some(system_id.clone()),
                            git_commit: None,
                            bytes,
                        })
                    })
                    .collect();
            }
        }

        self.fetch_conventional_publisher(root, requested_language)
    }

    fn fetch_conventional_publisher(
        &self,
        root: &Path,
        requested_language: Option<&LanguageId>,
    ) -> Result<Vec<FetchedRegistry>, RegistryCacheError> {
        let directory = root.join(".wax/registries");
        let languages = match requested_language {
            Some(language) => vec![language.clone()],
            None => {
                let entries =
                    fs::read_dir(&directory).map_err(|source| RegistryCacheError::ReadSource {
                        path: directory.clone(),
                        source,
                    })?;
                entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        let path = entry.path();
                        if path.extension().and_then(|value| value.to_str()) != Some("json") {
                            return None;
                        }
                        path.file_stem()?.to_str().map(str::to_owned)
                    })
                    .filter_map(|stem| LanguageId::try_from(stem).ok())
                    .collect()
            }
        };
        if languages.is_empty() {
            return Err(RegistryCacheError::MissingLanguage {
                identity: format!("publisher:{}", root.display()),
            });
        }
        languages
            .into_iter()
            .map(|language| {
                let path = directory.join(format!("{language}.json"));
                let bytes = fs::read(&path).map_err(|source| {
                    if source.kind() == io::ErrorKind::NotFound {
                        RegistryCacheError::MissingPublishedRegistry {
                            root: root.to_owned(),
                            system_id: None,
                            language: language.clone(),
                        }
                    } else {
                        RegistryCacheError::ReadSource {
                            path: path.clone(),
                            source,
                        }
                    }
                })?;
                Ok(FetchedRegistry {
                    source: format!("publisher:{}#{language}", root.display()),
                    language: Some(language),
                    system_id: None,
                    git_commit: None,
                    bytes,
                })
            })
            .collect()
    }

    fn validate_and_store(
        &self,
        fetched: FetchedRegistry,
    ) -> Result<RegistrySnapshot, RegistryCacheError> {
        let document = registry_document_from_bytes(&fetched.bytes).map_err(|source| {
            RegistryCacheError::InvalidRegistry {
                identity: fetched.source.clone(),
                source,
            }
        })?;
        let artifact_language = document
            .extra
            .get("language")
            .and_then(serde_json::Value::as_str)
            .map(LanguageId::try_from)
            .transpose()
            .map_err(|_| RegistryCacheError::InvalidLanguage {
                identity: fetched.source.clone(),
            })?;
        let language = match (fetched.language, artifact_language) {
            (Some(expected), Some(actual)) if expected != actual => {
                return Err(RegistryCacheError::LanguageMismatch {
                    identity: fetched.source,
                    expected,
                    actual,
                });
            }
            (Some(language), _) | (None, Some(language)) => language,
            (None, None) => {
                return Err(RegistryCacheError::MissingLanguage {
                    identity: fetched.source,
                });
            }
        };
        let digest = crate::digest::sha256_hex(&fetched.bytes);
        self.write_blob(&digest, &fetched.bytes)?;
        Ok(RegistrySnapshot {
            reference: RegistrySnapshotRef {
                source: fetched.source,
                language,
                digest,
                system_id: fetched.system_id,
                git_commit: fetched.git_commit,
                extra: Map::new(),
            },
            document,
        })
    }

    fn write_blob(&self, digest: &str, bytes: &[u8]) -> Result<(), RegistryCacheError> {
        let path = self.blob_path(digest);
        match write_atomically_no_clobber(&path, bytes, AtomicWriteOptions::default()) {
            Ok(()) => Ok(()),
            Err(AtomicWriteError::DestinationExists { .. }) => {
                let existing = fs::read(&path).map_err(|source| RegistryCacheError::CacheRead {
                    path: path.clone(),
                    source,
                })?;
                if crate::digest::sha256_hex(&existing) == digest {
                    Ok(())
                } else {
                    Err(RegistryCacheError::DigestMismatch {
                        path,
                        expected: digest.to_owned(),
                        actual: crate::digest::sha256_hex(&existing),
                    })
                }
            }
            Err(source) => Err(RegistryCacheError::AtomicWrite { path, source }),
        }
    }

    fn write_selection(
        &self,
        identity: &str,
        snapshots: Vec<RegistrySnapshotRef>,
    ) -> Result<(), RegistryCacheError> {
        let path = self.selection_path(identity);
        let bytes = serde_json::to_vec(&SelectionRecord { snapshots }).map_err(|source| {
            RegistryCacheError::SelectionEncoding {
                identity: identity.to_owned(),
                source,
            }
        })?;
        write_atomically(&path, &bytes, AtomicWriteOptions::default())
            .map_err(|source| RegistryCacheError::AtomicWrite { path, source })
    }

    fn load_selection(
        &self,
        identity: &str,
    ) -> Result<Option<Vec<RegistrySnapshotRef>>, RegistryCacheError> {
        let path = self.selection_path(identity);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(RegistryCacheError::CacheRead { path, source }),
        };
        let selection: SelectionRecord = serde_json::from_slice(&bytes).map_err(|source| {
            RegistryCacheError::InvalidSelection {
                path: path.clone(),
                source,
            }
        })?;
        Ok(Some(selection.snapshots))
    }

    fn load_references(
        &self,
        references: &[RegistrySnapshotRef],
    ) -> Result<Vec<RegistrySnapshot>, RegistryCacheError> {
        references
            .iter()
            .map(|reference| {
                let path = self.blob_path(&reference.digest);
                let bytes = fs::read(&path).map_err(|source| {
                    if source.kind() == io::ErrorKind::NotFound {
                        RegistryCacheError::SnapshotUnavailable {
                            snapshot: Box::new(reference.clone()),
                        }
                    } else {
                        RegistryCacheError::CacheRead {
                            path: path.clone(),
                            source,
                        }
                    }
                })?;
                let actual = crate::digest::sha256_hex(&bytes);
                if actual != reference.digest {
                    return Err(RegistryCacheError::DigestMismatch {
                        path,
                        expected: reference.digest.clone(),
                        actual,
                    });
                }
                let document = registry_document_from_bytes(&bytes).map_err(|source| {
                    RegistryCacheError::InvalidCachedRegistry {
                        snapshot: Box::new(reference.clone()),
                        source,
                    }
                })?;
                Ok(RegistrySnapshot {
                    reference: reference.clone(),
                    document,
                })
            })
            .collect()
    }

    fn validate_expected_source(
        &self,
        source: &RegistrySourceSpec,
        expected: &RegistrySnapshotRef,
    ) -> Result<(), RegistryCacheError> {
        if source.matches_reference(expected) {
            Ok(())
        } else {
            Err(RegistryCacheError::SnapshotMismatch {
                expected: Box::new(expected.clone()),
                requested: source.identity(),
            })
        }
    }

    fn blob_path(&self, digest: &str) -> PathBuf {
        self.root.join("blobs").join(format!("{digest}.json"))
    }

    fn selection_path(&self, identity: &str) -> PathBuf {
        self.root.join("sources").join(format!(
            "{}.json",
            crate::digest::sha256_hex(identity.as_bytes())
        ))
    }

    fn selection_lock_path(&self, identity: &str) -> PathBuf {
        self.root.join("sources").join(format!(
            "{}.lock",
            crate::digest::sha256_hex(identity.as_bytes())
        ))
    }
}

impl RegistrySourceSpec {
    /// Returns the stable, credential-redacted identity used by snapshot references.
    #[must_use]
    pub fn canonical_identity(&self) -> String {
        self.identity()
    }

    /// Applies a Git tag or commit pin when this source is Git-backed.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryCacheError::InvalidSourceSpec`] when `--tag` is supplied
    /// for a non-Git source.
    pub fn with_tag(mut self, tag: String) -> Result<Self, RegistryCacheError> {
        match &mut self {
            Self::Git {
                tag: source_tag, ..
            } => {
                *source_tag = Some(tag);
                Ok(self)
            }
            _ => Err(RegistryCacheError::InvalidSourceSpec {
                input: self.identity(),
                reason: "--tag is only valid for a Git registry source",
            }),
        }
    }

    /// Parses a user-facing registry source into a local, HTTP, or Git source spec.
    ///
    /// Git sources accept `git+<url>`, Git/SSH URLs, `.git` URLs, scp-style
    /// remotes, and `host/owner/repository` shorthand. A `#tag` suffix pins a
    /// Git ref. Existing local directories are publisher repositories.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryCacheError::MissingLanguage`] for Git without a
    /// language, or [`RegistryCacheError::InvalidSourceUrl`] for invalid file URLs.
    pub fn parse(input: &str, language: Option<LanguageId>) -> Result<Self, RegistryCacheError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(RegistryCacheError::InvalidSourceSpec {
                input: input.to_owned(),
                reason: "source must not be empty",
            });
        }
        if input.starts_with("file://") {
            let path = file_url_to_path(input).map_err(|error| match error {
                RegistrySourceError::InvalidFileUrl { reason, .. } => {
                    RegistryCacheError::InvalidSourceUrl {
                        identity: redact_url(input),
                        reason,
                    }
                }
                other => other.into(),
            })?;
            return Ok(Self::LocalFile { path, language });
        }

        let local_path = PathBuf::from(input);
        if local_path.is_dir() {
            return Ok(Self::PublisherRepository {
                root: local_path,
                system_id: None,
                language,
            });
        }

        let explicit_git = input.strip_prefix("git+");
        let is_git = explicit_git.is_some()
            || input.starts_with("git://")
            || input.starts_with("ssh://")
            || input.starts_with("git@")
            || input
                .split('#')
                .next()
                .is_some_and(|value| value.ends_with(".git"))
            || is_host_repo_shorthand(input);
        if is_git {
            let language = language.ok_or_else(|| RegistryCacheError::MissingLanguage {
                identity: redact_url(input),
            })?;
            let raw = explicit_git.unwrap_or(input);
            let (url, tag) = raw
                .rsplit_once('#')
                .map_or((raw, None), |(url, tag)| (url, Some(tag.to_owned())));
            let url = if is_host_repo_shorthand(url) {
                format!("https://{url}.git")
            } else {
                url.to_owned()
            };
            return Ok(Self::Git { url, tag, language });
        }

        if input.starts_with("http://") || input.starts_with("https://") {
            return Ok(Self::HttpJson {
                url: input.to_owned(),
                language,
            });
        }
        Ok(Self::LocalFile {
            path: local_path,
            language,
        })
    }

    fn identity(&self) -> String {
        match self {
            Self::LocalFile { path, .. } => local_file_identity(path),
            Self::PublisherRepository { root, .. } => {
                format!("publisher:{}", root.display())
            }
            Self::HttpJson { url, .. } => redact_url(url),
            Self::Git { url, tag, .. } => match tag {
                Some(tag) => format!("git:{}#{tag}", redact_url(url)),
                None => format!("git:{}", redact_url(url)),
            },
        }
    }

    fn selection_identity(&self) -> String {
        match self {
            Self::LocalFile { language, .. } | Self::HttpJson { language, .. } => format!(
                "{}|language={}",
                self.identity(),
                language.as_ref().map_or("", LanguageId::as_str)
            ),
            Self::PublisherRepository {
                system_id,
                language,
                ..
            } => format!(
                "{}|system={}|language={}",
                self.identity(),
                system_id.as_deref().unwrap_or_default(),
                language.as_ref().map_or("", LanguageId::as_str)
            ),
            Self::Git { language, .. } => {
                format!("{}|language={}", self.identity(), language.as_str())
            }
        }
    }

    fn matches_reference(&self, reference: &RegistrySnapshotRef) -> bool {
        match self {
            Self::LocalFile { language, .. } | Self::HttpJson { language, .. } => {
                language
                    .as_ref()
                    .is_none_or(|language| language == &reference.language)
                    && reference.source == self.identity()
            }
            Self::Git { language, .. } => {
                language == &reference.language && reference.source == self.identity()
            }
            Self::PublisherRepository {
                root,
                system_id,
                language,
            } => {
                let language_matches = language
                    .as_ref()
                    .is_none_or(|language| language == &reference.language);
                let system_matches = system_id.as_ref().is_none_or(|system_id| {
                    reference.system_id.as_deref() == Some(system_id.as_str())
                });
                let root_identity = format!("publisher:{}", root.display());
                let source_matches = reference.source == root_identity
                    || reference.source.starts_with(&format!("{root_identity}#"));
                language_matches && system_matches && source_matches
            }
        }
    }
}

fn is_host_repo_shorthand(value: &str) -> bool {
    !value.contains("://")
        && !value.starts_with('.')
        && !value.starts_with('/')
        && value
            .split('/')
            .next()
            .is_some_and(|host| host.contains('.'))
        && value.matches('/').count() >= 2
}

#[derive(Debug)]
struct FetchedRegistry {
    source: String,
    language: Option<LanguageId>,
    system_id: Option<String>,
    git_commit: Option<String>,
    bytes: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SelectionRecord {
    snapshots: Vec<RegistrySnapshotRef>,
}

fn local_file_identity(path: &Path) -> String {
    format!("file:{}", path.display())
}

fn registry_document_from_bytes(bytes: &[u8]) -> Result<RegistryDocument, RegistryError> {
    let text = std::str::from_utf8(bytes).map_err(|source| {
        RegistryError::InvalidJson(serde_json::Error::io(io::Error::new(
            io::ErrorKind::InvalidData,
            source,
        )))
    })?;
    registry_from_json(text)
}

fn redact_url(value: &str) -> String {
    let Some(scheme_end) = value.find("://") else {
        return value.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority_end = value[authority_start..]
        .find('/')
        .map_or(value.len(), |offset| authority_start + offset);
    let Some(at) = value[authority_start..authority_end].rfind('@') else {
        return value.to_owned();
    };
    let host_start = authority_start + at + 1;
    format!("{}{}", &value[..authority_start], &value[host_start..])
}

fn redact_embedded_userinfo(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(scheme_end) = rest.find("://") {
        out.push_str(&rest[..scheme_end + 3]);
        let after_scheme = &rest[scheme_end + 3..];
        let authority_end = after_scheme
            .find(|c: char| c == '/' || c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(after_scheme.len());
        let authority = &after_scheme[..authority_end];
        if let Some(at) = authority.rfind('@') {
            out.push_str(&authority[at + 1..]);
        } else {
            out.push_str(authority);
        }
        rest = &after_scheme[authority_end..];
    }
    out.push_str(rest);
    out
}

fn map_http_fetch_error(url: &str, error: RegistrySourceError) -> RegistryCacheError {
    let identity = redact_url(url);
    match error {
        RegistrySourceError::Fetch { http, .. } => RegistryCacheError::HttpRequest {
            identity,
            source: http,
        },
        RegistrySourceError::HttpStatus { status, .. } => {
            RegistryCacheError::HttpStatus { identity, status }
        }
        other => other.into(),
    }
}

fn fetch_http_registry_bytes(url: &str) -> Result<Vec<u8>, RegistryCacheError> {
    fetch_http_bytes(url).map_err(|error| map_http_fetch_error(url, error))
}

fn read_publisher_mapping(root: &Path, source: &str) -> Result<Vec<u8>, RegistryCacheError> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return fetch_http_registry_bytes(source);
    }
    if source.starts_with("file://") {
        let path = file_url_to_path(source).map_err(|error| match error {
            RegistrySourceError::InvalidFileUrl { reason, .. } => {
                RegistryCacheError::InvalidSourceUrl {
                    identity: redact_url(source),
                    reason,
                }
            }
            other => other.into(),
        })?;
        return fs::read(&path).map_err(|source| RegistryCacheError::ReadSource { path, source });
    }
    let path = validate_repo_relative_registry_path_within_repo(root, source)?;
    fs::read(&path).map_err(|source| RegistryCacheError::ReadSource { path, source })
}

/// Typed failures returned while resolving registry snapshots.
#[derive(Debug, Error)]
pub enum RegistryCacheError {
    /// Offline mode had no selected snapshot for the requested source.
    #[error("registry source {identity} is not available in the offline cache")]
    OfflineCacheMiss {
        /// Canonical source identity.
        identity: String,
    },
    /// A source string was empty or otherwise invalid.
    #[error("invalid registry source {input:?}: {reason}")]
    InvalidSourceSpec {
        /// Original source string.
        input: String,
        /// Static validation reason.
        reason: &'static str,
    },
    /// Git source resolution failed.
    #[error("failed to resolve Git registry: {0}")]
    Git(#[from] RegistryGitError),
    /// Publisher config could not be loaded.
    #[error(transparent)]
    PublisherConfig(#[from] WaxRcError),
    /// A publisher mapping used an invalid repo-relative source.
    #[error(transparent)]
    RegistrySource(#[from] RegistrySourceError),
    /// A source URL could not be parsed.
    #[error("invalid registry source URL {identity}: {reason}")]
    InvalidSourceUrl {
        /// Credential-free source identity.
        identity: String,
        /// Static URL parse failure reason.
        reason: &'static str,
    },
    /// A requested publisher id was absent from the repository config.
    #[error("publisher {system_id} is not configured in {root}")]
    PublisherNotFound {
        /// Publisher repository root.
        root: PathBuf,
        /// Requested publisher id.
        system_id: String,
    },
    /// Multiple configured publishers require an explicit id.
    #[error("publisher repository {root} contains multiple design systems; specify one")]
    AmbiguousPublisher {
        /// Publisher repository root.
        root: PathBuf,
    },
    /// A configured publisher contains no registry mappings.
    #[error("publisher {system_id} in {root} has no registry mappings")]
    PublisherHasNoRegistries {
        /// Publisher repository root.
        root: PathBuf,
        /// Selected publisher id.
        system_id: String,
    },
    /// The publisher does not provide the requested conventional registry.
    #[error("publisher repository {root} does not provide a registry for {language}")]
    MissingPublishedRegistry {
        /// Publisher repository root.
        root: PathBuf,
        /// Configured publisher id, when selected through config.
        system_id: Option<String>,
        /// Requested language.
        language: LanguageId,
    },
    /// Neither the source spec nor registry artifact identified a language.
    #[error("registry source {identity} does not identify a language")]
    MissingLanguage {
        /// Canonical source identity.
        identity: String,
    },
    /// The registry artifact declared an invalid language id.
    #[error("registry source {identity} declares an invalid language id")]
    InvalidLanguage {
        /// Canonical source identity.
        identity: String,
    },
    /// Requested and artifact-declared languages differ.
    #[error(
        "registry source {identity} language mismatch: expected {expected}, artifact declares {actual}"
    )]
    LanguageMismatch {
        /// Canonical source identity.
        identity: String,
        /// Language requested by the caller.
        expected: LanguageId,
        /// Language declared by the artifact.
        actual: LanguageId,
    },
    /// A local source could not be read.
    #[error("failed to read registry source {path}: {source}")]
    ReadSource {
        /// Source path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// An HTTP request failed.
    #[error(
        "failed to fetch registry source {identity}: {}",
        redact_embedded_userinfo(&.source.to_string())
    )]
    HttpRequest {
        /// Credential-free source identity.
        identity: String,
        /// Underlying HTTP transport failure.
        #[source]
        source: reqwest::Error,
    },
    /// An HTTP source returned a non-success status.
    #[error("failed to fetch registry source {identity}: HTTP {status}")]
    HttpStatus {
        /// Credential-free source identity.
        identity: String,
        /// Response status.
        status: reqwest::StatusCode,
    },
    /// Source bytes did not satisfy the registry contract.
    #[error("invalid registry from {identity}: {source}")]
    InvalidRegistry {
        /// Credential-free source identity.
        identity: String,
        /// Registry contract failure.
        #[source]
        source: RegistryError,
    },
    /// A cache file could not be read.
    #[error("failed to read registry cache file {path}: {source}")]
    CacheRead {
        /// Cache file path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A source selection could not be locked for fetch or refresh.
    #[error("failed to lock registry cache selection {path}: {source}")]
    CacheLock {
        /// Lock file path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A cache file could not be written atomically.
    #[error("failed to write registry cache file {path}: {source}")]
    AtomicWrite {
        /// Cache file path.
        path: PathBuf,
        /// Atomic write failure.
        #[source]
        source: AtomicWriteError,
    },
    /// A selected-source record could not be encoded.
    #[error("failed to encode selected registry source {identity}: {source}")]
    SelectionEncoding {
        /// Canonical source identity.
        identity: String,
        /// JSON encoding failure.
        #[source]
        source: serde_json::Error,
    },
    /// A selected-source record is malformed.
    #[error("invalid selected registry source record {path}: {source}")]
    InvalidSelection {
        /// Record path.
        path: PathBuf,
        /// JSON decoding failure.
        #[source]
        source: serde_json::Error,
    },
    /// A cached blob does not match its content-addressed path.
    #[error("registry cache digest mismatch at {path}: expected {expected}, got {actual}")]
    DigestMismatch {
        /// Blob path.
        path: PathBuf,
        /// Digest named by the cache record.
        expected: String,
        /// Digest computed from stored bytes.
        actual: String,
    },
    /// A requested immutable blob is absent.
    #[error("registry snapshot is unavailable: {snapshot:?}")]
    SnapshotUnavailable {
        /// Exact requested snapshot.
        snapshot: Box<RegistrySnapshotRef>,
    },
    /// A cached blob no longer parses as a valid registry.
    #[error("cached registry snapshot {snapshot:?} is invalid: {source}")]
    InvalidCachedRegistry {
        /// Selected snapshot identity.
        snapshot: Box<RegistrySnapshotRef>,
        /// Registry contract failure.
        #[source]
        source: RegistryError,
    },
    /// An expected snapshot belongs to a different source or language.
    #[error("expected registry snapshot {expected:?} does not match requested source {requested}")]
    SnapshotMismatch {
        /// Exact expected snapshot.
        expected: Box<RegistrySnapshotRef>,
        /// Requested source identity.
        requested: String,
    },
}
