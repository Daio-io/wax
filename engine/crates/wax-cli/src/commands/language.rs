//! `wax language` command implementations.

use super::state_path::resolve_state_path;
use crate::progress::CliProgress;
use semver::Version;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use wax_contract::{LanguageId, LanguageIdError};
use wax_core::config::lockfile::{
    LockedLanguage, LockedRegistry, LockfileError, ResolvedLanguage, WAX_LOCK_SCHEMA_VERSION,
    WaxLock, load_lockfile,
};
use wax_core::config::waxrc::{WaxRc, WaxRcError, load_waxrc};
use wax_core::defaults::DEFAULT_WAX_PACK_INDEX;
use wax_core::global_state::{
    GlobalState, GlobalStateError, InstalledLanguagePack, load_global_state, save_global_state,
};
use wax_core::install::{InstallError, LanguagePackManifestSpec, install_language};
use wax_core::paths::{PathsError, lang_install_dir};
use wax_core::registry::{
    RegistryArtifact, RegistryError, RegistryManifest, fetch_pack_index, select_target_artifact,
};
use wax_lang_api::build_version;

/// Options for `wax language list`.
#[derive(Debug, Clone)]
pub struct ListOptions {
    /// Deprecated registry URL retained for CLI compatibility.
    pub registry_url: Option<String>,
    /// State path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Parsed `wax language install <id>[@version]` target.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LanguageInstallSpec {
    /// Language id to install.
    pub language_id: LanguageId,
    /// Optional exact version pin from the command line.
    pub version: Option<String>,
}

impl LanguageInstallSpec {
    /// Parses a language install spec from `<id>` or `<id>@<version>`.
    pub fn parse(value: &str) -> Result<Self, LanguageCommandError> {
        let (language_id, version) = match value.split_once('@') {
            Some((id, version)) if !version.is_empty() => {
                (LanguageId::try_from(id)?, Some(version.to_owned()))
            }
            Some(_) => {
                return Err(LanguageCommandError::InvalidLanguageSpec {
                    spec: value.to_owned(),
                });
            }
            None => (LanguageId::try_from(value)?, None),
        };

        Ok(Self {
            language_id,
            version,
        })
    }
}

impl FromStr for LanguageInstallSpec {
    type Err = LanguageCommandError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// Options for `wax language install`.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Language id to install.
    pub language_id: LanguageId,
    /// Optional exact version to install.
    pub version: Option<String>,
    /// Pack index URL. Resolution precedence: `--pack-index` > `WAX_PACK_INDEX` > built-in default.
    pub registry_url: Option<String>,
    /// Target triple override for tests.
    pub target_triple: Option<String>,
    /// State path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Options for `wax language uninstall`.
#[derive(Debug, Clone)]
pub struct UninstallOptions {
    /// Language id to uninstall.
    pub language_id: LanguageId,
    /// Optional specific version to remove.
    pub version: Option<String>,
    /// State path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Options for `wax language update`.
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// Language id to update, unless `all` is true.
    pub language_id: Option<LanguageId>,
    /// Whether every installed language should be updated.
    pub all: bool,
    /// Pack index URL. Resolution precedence: `--pack-index` > `WAX_PACK_INDEX` > built-in default.
    pub registry_url: Option<String>,
    /// Target triple override for tests.
    pub target_triple: Option<String>,
    /// State path override for tests.
    pub state_path: Option<PathBuf>,
    /// Repository root; config and lock paths are discovered under `.wax/wax.config.json` and `.wax/wax.lock.json`.
    pub repo_root: PathBuf,
}

/// Options for `wax language doctor`.
#[derive(Debug, Clone)]
pub struct DoctorOptions {
    /// Repository root; config and lock paths are discovered under `.wax/wax.config.json` and `.wax/wax.lock.json`.
    pub repo_root: PathBuf,
    /// State path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Errors returned by language lifecycle commands.
#[derive(Debug, Error)]
pub enum LanguageCommandError {
    /// Install spec was malformed.
    #[error("invalid language install spec {spec:?}; expected <id> or <id>@<version>")]
    InvalidLanguageSpec {
        /// Spec passed by the user.
        spec: String,
    },
    /// Update did not identify a language or all installed languages.
    #[error("update requires a language id or --all")]
    MissingUpdateSelection,
    /// Requested language id was not present in the registry.
    #[error("language {language_id} was not found in registry")]
    LanguageNotFound {
        /// Requested language id.
        language_id: LanguageId,
    },
    /// Requested language version was not present in the registry.
    #[error("language {language_id} version {version} was not found in registry")]
    LanguageVersionNotFound {
        /// Requested language id.
        language_id: LanguageId,
        /// Requested version.
        version: String,
    },
    /// Language id construction failed.
    #[error(transparent)]
    LanguageId(#[from] LanguageIdError),
    /// Registry loading failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Install failed.
    #[error(transparent)]
    Install(#[from] InstallError),
    /// Global state load/save failed.
    #[error(transparent)]
    GlobalState(#[from] GlobalStateError),
    /// Global path resolution failed.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// `.wax/wax.config.json` loading failed.
    #[error(transparent)]
    WaxRc(#[from] WaxRcError),
    /// Lockfile loading failed.
    #[error(transparent)]
    Lockfile(#[from] LockfileError),
    /// Registry source resolution failed.
    #[error(transparent)]
    RegistrySource(#[from] wax_core::registry_source::RegistrySourceError),
    /// Filesystem operation failed.
    #[error("{context}: {source}")]
    Io {
        /// Human-readable context.
        context: String,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax language list`.
pub fn run_list(options: ListOptions, writer: &mut impl Write) -> Result<(), LanguageCommandError> {
    let ListOptions {
        registry_url,
        state_path,
    } = options;
    drop(registry_url);
    let state = load_state(state_path.as_deref())?;

    writeln!(writer, "language\tversion\tstatus").map_err(write_error)?;
    for (language_id, versions) in state.installed_languages {
        for version in versions.keys() {
            writeln!(writer, "{language_id}\t{version}\tinstalled").map_err(write_error)?;
        }
    }

    Ok(())
}

/// Runs `wax language install`.
pub fn run_install(
    options: InstallOptions,
    writer: &mut impl Write,
) -> Result<(), LanguageCommandError> {
    let progress = Arc::new(CliProgress::new());
    progress.set_message("Fetching pack index…");

    let registry_url = resolve_registry_url(options.registry_url);
    let manifests = fetch_pack_index(&registry_url)?;
    let manifest =
        manifest_for_language(&manifests, &options.language_id, options.version.as_deref())?;
    progress.set_message(format!("Installing {}@{}…", manifest.id, manifest.version));
    let install_dir = install_manifest(&manifest, options.target_triple.as_deref())?;
    progress.set_message(format!(
        "Recording {}@{} install…",
        manifest.id, manifest.version
    ));
    if let Err(err) = record_installed_language(
        options.state_path,
        &manifest.id,
        &manifest.version,
        install_dir,
    ) {
        remove_dir_if_exists(&lang_install_dir(&manifest.id, &manifest.version)?)?;
        return Err(err);
    }
    progress.finish();
    writeln!(writer, "installed {} {}", manifest.id, manifest.version).map_err(write_error)?;
    Ok(())
}

/// Runs `wax language uninstall`.
pub fn run_uninstall(
    options: UninstallOptions,
    writer: &mut impl Write,
) -> Result<(), LanguageCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let mut state = load_global_state(&state_path)?;
    let removed = remove_installed_versions(
        &state_path,
        &mut state,
        &options.language_id,
        options.version.as_deref(),
    )?;
    writeln!(
        writer,
        "uninstalled {} version(s) of {}",
        removed, options.language_id
    )
    .map_err(write_error)?;
    Ok(())
}

/// Runs `wax language update`.
pub fn run_update(
    options: UpdateOptions,
    writer: &mut impl Write,
) -> Result<(), LanguageCommandError> {
    let registry_url = resolve_registry_url(options.registry_url);
    let manifests = fetch_pack_index(&registry_url)?;
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let mut state = load_global_state(&state_path)?;
    let repo_files = wax_core::config::repo_files::discover_repo_files(&options.repo_root);
    let lockfile_path = repo_files.lockfile_path;
    let should_refresh_registry_locks = repo_files.config_path.is_file();
    let mut lockfile = load_optional_lockfile(&lockfile_path)?;

    let language_ids = update_language_ids(&state, options.language_id.as_ref(), options.all)?;
    let mut updated = Vec::new();
    let mut total_removed = 0;
    for language_id in language_ids {
        let manifest = manifest_for_language(&manifests, &language_id, None)?;
        let target = options
            .target_triple
            .clone()
            .unwrap_or_else(default_target_triple);
        let artifact = select_target_artifact(&manifest, &target)?.clone();
        if !state
            .installed_languages
            .get(&language_id)
            .is_some_and(|versions| versions.contains_key(&manifest.version))
        {
            let install_dir = install_resolved_manifest(&manifest, &target, &artifact)?;
            state
                .installed_languages
                .entry(manifest.id.clone())
                .or_default()
                .insert(
                    manifest.version.clone(),
                    InstalledLanguagePack { install_dir },
                );
            if let Err(err) = save_global_state(&state_path, &state) {
                remove_installed_version_from_state(&mut state, &manifest.id, &manifest.version);
                remove_dir_if_exists(&lang_install_dir(&manifest.id, &manifest.version)?)?;
                return Err(err.into());
            }
        }

        if let Some(lockfile) = lockfile.as_mut() {
            update_lockfile_entry(lockfile, &manifest, &registry_url, &target, &artifact);
        }

        total_removed += remove_installed_versions_except(
            &state_path,
            &mut state,
            &language_id,
            &manifest.version,
        )?;
        updated.push((manifest.id, manifest.version));
    }

    if let Some(lockfile) = lockfile.as_mut() {
        if should_refresh_registry_locks {
            let waxrc = load_waxrc(&repo_files.config_path)?;
            refresh_registry_locks_in_lockfile(lockfile, &options.repo_root, &waxrc)?;
        }
        save_lockfile(&lockfile_path, lockfile)?;
    }

    for (language_id, version) in updated {
        writeln!(writer, "updated {language_id} to {version}").map_err(write_error)?;
    }
    writeln!(writer, "removed {total_removed} old version(s)").map_err(write_error)?;
    Ok(())
}

/// Runs `wax language doctor`.
pub fn run_doctor(
    options: DoctorOptions,
    writer: &mut impl Write,
) -> Result<(), LanguageCommandError> {
    let repo_files = wax_core::config::repo_files::discover_repo_files(&options.repo_root);
    let waxrc = load_waxrc(&repo_files.config_path)?;
    let lockfile = load_optional_lockfile(&repo_files.lockfile_path)?;
    let state = load_state(options.state_path.as_deref())?;

    let mut language_ids = BTreeSet::new();
    let mut config_status = BTreeMap::new();
    for language in &waxrc.languages {
        language_ids.insert(language.id.clone());
        config_status.insert(language.id.clone(), "yes");
    }
    if let Some(lockfile) = &lockfile {
        language_ids.extend(lockfile.languages.keys().cloned());
    }
    language_ids.extend(state.installed_languages.keys().cloned());

    let effective_registry = resolve_effective_registry_url(None);
    writeln!(writer, "pack index: {}", effective_registry.url).map_err(write_error)?;
    writeln!(
        writer,
        "  source: {}",
        match effective_registry.source {
            RegistryUrlSource::Cli => "--pack-index",
            RegistryUrlSource::Env => "WAX_PACK_INDEX",
            RegistryUrlSource::Default => "default",
        }
    )
    .map_err(write_error)?;

    for language_id in language_ids {
        let installed_versions = state
            .installed_languages
            .get(&language_id)
            .map(installed_version_list)
            .unwrap_or_else(|| "missing".to_owned());
        let lock_version = lockfile
            .as_ref()
            .and_then(|lock| lock.languages.get(&language_id))
            .map(|entry| entry.version.as_str())
            .unwrap_or("missing");
        let missing_binary = has_missing_binary(&state, &language_id);
        let enabled = config_status
            .get(&language_id)
            .copied()
            .unwrap_or("missing");

        writeln!(writer, "language: {language_id}").map_err(write_error)?;
        writeln!(writer, "  enabled: {enabled}").map_err(write_error)?;
        writeln!(writer, "  installed: {installed_versions}").map_err(write_error)?;
        writeln!(writer, "  lock: {lock_version}").map_err(write_error)?;
        writeln!(
            writer,
            "  missing binary: {}",
            if missing_binary { "yes" } else { "no" }
        )
        .map_err(write_error)?;
    }

    Ok(())
}

pub(crate) fn resolve_registry_url(registry_url: Option<String>) -> String {
    resolve_effective_registry_url(registry_url).url
}

fn resolve_effective_registry_url(registry_url: Option<String>) -> EffectiveRegistryUrl {
    if let Some(url) = registry_url.filter(|url| !url.trim().is_empty()) {
        return EffectiveRegistryUrl {
            url,
            source: RegistryUrlSource::Cli,
        };
    }
    if let Some(url) = std::env::var("WAX_PACK_INDEX")
        .ok()
        .filter(|url| !url.trim().is_empty())
    {
        return EffectiveRegistryUrl {
            url,
            source: RegistryUrlSource::Env,
        };
    }

    EffectiveRegistryUrl {
        url: DEFAULT_WAX_PACK_INDEX.to_owned(),
        source: RegistryUrlSource::Default,
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RegistryUrlSource {
    Cli,
    Env,
    Default,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct EffectiveRegistryUrl {
    url: String,
    source: RegistryUrlSource,
}

fn load_state(state_path: Option<&Path>) -> Result<GlobalState, LanguageCommandError> {
    Ok(load_global_state(&resolve_state_path(state_path)?)?)
}

pub(crate) fn manifest_for_language(
    manifests: &[RegistryManifest],
    language_id: &LanguageId,
    version: Option<&str>,
) -> Result<RegistryManifest, LanguageCommandError> {
    let matches = manifests
        .iter()
        .filter(|manifest| manifest.id == *language_id);

    let manifest = match version {
        Some(version) => matches
            .filter(|manifest| manifest.version == version)
            .max_by(|left, right| compare_registry_versions(&left.version, &right.version)),
        None => {
            matches.max_by(|left, right| compare_registry_versions(&left.version, &right.version))
        }
    };

    manifest.cloned().ok_or_else(|| match version {
        Some(version) => LanguageCommandError::LanguageVersionNotFound {
            language_id: language_id.clone(),
            version: version.to_owned(),
        },
        None => LanguageCommandError::LanguageNotFound {
            language_id: language_id.clone(),
        },
    })
}

fn compare_registry_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
        (Err(_), Ok(_)) => std::cmp::Ordering::Less,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

fn install_manifest(
    registry_manifest: &RegistryManifest,
    target_triple: Option<&str>,
) -> Result<PathBuf, LanguageCommandError> {
    let target = target_triple
        .map(str::to_owned)
        .unwrap_or_else(default_target_triple);
    let artifact = select_target_artifact(registry_manifest, &target)?;
    install_resolved_manifest(registry_manifest, &target, artifact)
}

/// Installs or reuses a language pack from registry metadata already resolved for lockfile pinning.
pub(crate) fn install_pinned_manifest(
    registry_manifest: &RegistryManifest,
    target: &str,
    artifact: &RegistryArtifact,
    state_path: Option<PathBuf>,
    writer: &mut impl Write,
) -> Result<(), LanguageCommandError> {
    let (install_dir, installed_fresh) =
        match install_resolved_manifest(registry_manifest, target, artifact) {
            Ok(install_dir) => (install_dir, true),
            Err(LanguageCommandError::Install(InstallError::AlreadyInstalled { path })) => {
                (path.into(), false)
            }
            Err(err) => return Err(err),
        };
    if let Err(err) = record_installed_language(
        state_path,
        &registry_manifest.id,
        &registry_manifest.version,
        install_dir,
    ) {
        if installed_fresh {
            remove_dir_if_exists(&lang_install_dir(
                &registry_manifest.id,
                &registry_manifest.version,
            )?)?;
        }
        return Err(err);
    }
    writeln!(
        writer,
        "installed {} {}",
        registry_manifest.id, registry_manifest.version
    )
    .map_err(write_error)?;
    Ok(())
}

fn install_resolved_manifest(
    registry_manifest: &RegistryManifest,
    target: &str,
    artifact: &RegistryArtifact,
) -> Result<PathBuf, LanguageCommandError> {
    let manifest = LanguagePackManifestSpec {
        id: registry_manifest.id.clone(),
        version: registry_manifest.version.clone(),
        api_version: registry_manifest.api_version,
        command: vec![
            format!("./wax-lang-{}", registry_manifest.id),
            "--stdio".to_owned(),
        ],
        ecosystem: registry_manifest.id.to_string(),
        parser_name: registry_manifest.id.to_string(),
        parser_version: registry_manifest.version.clone(),
    };

    Ok(install_language(
        &registry_manifest.id,
        &registry_manifest.version,
        target,
        &artifact.url,
        &artifact.sha256,
        Some(&artifact.sha256),
        &manifest,
    )?)
}

pub(crate) fn update_lockfile_entry(
    lockfile: &mut WaxLock,
    registry_manifest: &RegistryManifest,
    registry_url: &str,
    target: &str,
    artifact: &RegistryArtifact,
) {
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    lockfile.wax_version = build_version().to_owned();
    lockfile.locked_at = Some(time::OffsetDateTime::now_utc());
    lockfile.languages.insert(
        registry_manifest.id.clone(),
        LockedLanguage {
            version: registry_manifest.version.clone(),
            api_version: registry_manifest.api_version,
            source: registry_url.to_owned(),
            resolved: ResolvedLanguage {
                target: target.to_owned(),
                url: artifact.url.clone(),
                sha256: artifact.sha256.clone(),
                signature: None,
            },
        },
    );
}

pub(crate) fn save_lockfile(path: &Path, lockfile: &WaxLock) -> Result<(), LanguageCommandError> {
    let mut lockfile = lockfile.clone();
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    let contents =
        serde_json::to_string_pretty(&lockfile).map_err(|source| LanguageCommandError::Io {
            context: format!("serialize lockfile {}", path.display()),
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;
    fs::write(path, format!("{contents}\n")).map_err(|source| LanguageCommandError::Io {
        context: format!("write lockfile {}", path.display()),
        source,
    })
}

fn record_installed_language(
    state_path: Option<PathBuf>,
    language_id: &LanguageId,
    version: &str,
    install_dir: PathBuf,
) -> Result<(), LanguageCommandError> {
    let state_path = resolve_state_path(state_path.as_deref())?;
    let mut state = load_global_state(&state_path)?;
    state
        .installed_languages
        .entry(language_id.clone())
        .or_default()
        .insert(version.to_owned(), InstalledLanguagePack { install_dir });
    save_global_state(&state_path, &state)?;
    Ok(())
}

fn remove_installed_versions(
    state_path: &Path,
    state: &mut GlobalState,
    language_id: &LanguageId,
    version: Option<&str>,
) -> Result<usize, LanguageCommandError> {
    let Some(versions) = state.installed_languages.get(language_id) else {
        return Ok(0);
    };

    let removals: Vec<String> = match version {
        Some(version) => {
            if versions.contains_key(version) {
                vec![version.to_owned()]
            } else {
                Vec::new()
            }
        }
        None => versions.keys().cloned().collect(),
    };

    for version in &removals {
        let install_dir = lang_install_dir(language_id, version)?;
        remove_dir_if_exists(&install_dir)?;
        remove_installed_version_from_state(state, language_id, version);
        save_global_state(state_path, state)?;
    }

    Ok(removals.len())
}

fn remove_installed_versions_except(
    state_path: &Path,
    state: &mut GlobalState,
    language_id: &LanguageId,
    keep_version: &str,
) -> Result<usize, LanguageCommandError> {
    let Some(versions) = state.installed_languages.get(language_id) else {
        return Ok(0);
    };

    let removals: Vec<String> = versions
        .iter()
        .filter(|(version, _)| version.as_str() != keep_version)
        .map(|(version, _)| version.clone())
        .collect();

    for version in &removals {
        let install_dir = lang_install_dir(language_id, version)?;
        remove_dir_if_exists(&install_dir)?;
        remove_installed_version_from_state(state, language_id, version);
        save_global_state(state_path, state)?;
    }

    Ok(removals.len())
}

fn remove_installed_version_from_state(
    state: &mut GlobalState,
    language_id: &LanguageId,
    version: &str,
) {
    let Some(versions) = state.installed_languages.get_mut(language_id) else {
        return;
    };
    versions.remove(version);
    if versions.is_empty() {
        state.installed_languages.remove(language_id);
    }
}

fn update_language_ids(
    state: &GlobalState,
    language_id: Option<&LanguageId>,
    all: bool,
) -> Result<Vec<LanguageId>, LanguageCommandError> {
    if all {
        return Ok(state.installed_languages.keys().cloned().collect());
    }

    language_id
        .cloned()
        .map(|language_id| vec![language_id])
        .ok_or(LanguageCommandError::MissingUpdateSelection)
}

fn remove_dir_if_exists(path: &Path) -> Result<(), LanguageCommandError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(LanguageCommandError::Io {
            context: format!(
                "remove installed language pack directory {}",
                path.display()
            ),
            source,
        }),
    }
}

fn installed_version_list(versions: &BTreeMap<String, InstalledLanguagePack>) -> String {
    versions.keys().cloned().collect::<Vec<_>>().join(",")
}

fn load_optional_lockfile(path: &Path) -> Result<Option<WaxLock>, LanguageCommandError> {
    match fs::metadata(path) {
        Ok(_) => Ok(Some(load_lockfile(path)?)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(LanguageCommandError::Io {
            context: format!("read lockfile metadata {}", path.display()),
            source,
        }),
    }
}

#[cfg(test)]
fn load_optional_lockfile_for_repo(
    repo_root: &Path,
) -> Result<Option<WaxLock>, LanguageCommandError> {
    let repo_files = wax_core::config::repo_files::discover_repo_files(repo_root);
    load_optional_lockfile(&repo_files.lockfile_path)
}

fn refresh_registry_locks_in_lockfile(
    lockfile: &mut WaxLock,
    repo_root: &Path,
    waxrc: &WaxRc,
) -> Result<(), LanguageCommandError> {
    for entry in &waxrc.languages {
        let resolved = wax_core::registry_source::resolve_registry_source(
            wax_core::registry_source::RegistrySourceInput {
                repo_root,
                language_id: entry.id.as_str(),
                source: entry
                    .registry_source
                    .as_ref()
                    .map(|setting| setting.source.as_str()),
            },
        )?;
        lockfile.registries.insert(
            entry.id.clone(),
            LockedRegistry {
                source: resolved.source,
                sha256: resolved.sha256,
            },
        );
    }

    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    Ok(())
}

/// Refreshes registry lock entries for enabled languages and writes the lockfile.
#[cfg(test)]
pub(crate) fn refresh_registry_locks_for_repo(
    repo_root: &Path,
) -> Result<(), LanguageCommandError> {
    let repo_files = wax_core::config::repo_files::discover_repo_files(repo_root);
    let waxrc = load_waxrc(&repo_files.config_path)?;
    let mut lockfile = load_lockfile(&repo_files.lockfile_path)?;
    refresh_registry_locks_in_lockfile(&mut lockfile, repo_root, &waxrc)?;
    save_lockfile(&repo_files.lockfile_path, &lockfile)
}

fn has_missing_binary(state: &GlobalState, language_id: &LanguageId) -> bool {
    let Some(versions) = state.installed_languages.get(language_id) else {
        return true;
    };

    if versions.is_empty() {
        return true;
    }

    versions
        .values()
        .any(|pack| installed_pack_missing_binary(&pack.install_dir))
}

fn installed_pack_missing_binary(install_dir: &Path) -> bool {
    let manifest_path = install_dir.join("manifest.json");
    let Ok(contents) = fs::read_to_string(&manifest_path) else {
        return true;
    };
    let Ok(manifest) = serde_json::from_str::<InstalledManifest>(&contents) else {
        return true;
    };
    let Some(primary) = manifest.command.first() else {
        return true;
    };
    let Some(relative) = primary.strip_prefix("./") else {
        return true;
    };
    !install_dir.join(relative).is_file()
}

pub(crate) fn default_target_triple() -> String {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "aarch64-apple-darwin".to_owned()
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        "x86_64-apple-darwin".to_owned()
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
    {
        "x86_64-unknown-linux-gnu".to_owned()
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
    {
        "aarch64-unknown-linux-gnu".to_owned()
    }
    #[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"))]
    {
        "x86_64-pc-windows-msvc".to_owned()
    }
    #[cfg(all(target_arch = "aarch64", target_os = "windows", target_env = "msvc"))]
    {
        "aarch64-pc-windows-msvc".to_owned()
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
        all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"),
        all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"),
        all(target_arch = "aarch64", target_os = "windows", target_env = "msvc")
    )))]
    {
        format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
    }
}

fn write_error(source: io::Error) -> LanguageCommandError {
    LanguageCommandError::Io {
        context: "write command output".to_owned(),
        source,
    }
}

#[derive(Debug, Deserialize)]
struct InstalledManifest {
    command: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use wax_contract::LanguageId;
    use wax_core::global_state::{GlobalState, InstalledLanguagePack, save_global_state};

    use crate::testing::env_lock;

    #[test]
    fn clap_tree_exposes_language_lifecycle_subcommands() {
        let help = crate::cli::Cli::command().render_long_help().to_string();

        assert!(help.contains("language"));
        assert!(help.contains("init"));

        let command = crate::cli::Cli::command();
        let language = command
            .find_subcommand("language")
            .expect("language subcommand");

        for subcommand in ["list", "install", "uninstall", "update", "doctor"] {
            assert!(
                language.find_subcommand(subcommand).is_some(),
                "expected language {subcommand} subcommand"
            );
        }
    }

    #[test]
    fn list_with_registry_still_prints_installed_state_only() {
        let temp = TestDir::new("list");
        let registry_path = temp.path().join("registry.json");
        fs::write(
            &registry_path,
            r#"[
                {"id":"compose","version":"0.4.2","api_version":1,"targets":{"test-target":{"url":"file:///tmp/pack.tgz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}},
                {"id":"react","version":"1.0.0","api_version":1,"targets":{"test-target":{"url":"file:///tmp/react.tgz","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}}
            ]"#,
        )
        .unwrap();

        let state_path = temp.path().join("state.json");
        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([(
                "0.4.2".to_owned(),
                InstalledLanguagePack {
                    install_dir: temp.path().join("langs/compose/0.4.2"),
                },
            )]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_list(
            ListOptions {
                registry_url: Some(file_url(&registry_path)),
                state_path: Some(state_path),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("compose"));
        assert!(output.contains("0.4.2"));
        assert!(output.contains("installed"));
        assert!(!output.contains("react"));
        assert!(!output.contains("1.0.0"));
    }

    #[test]
    fn list_without_registry_prints_installed_state_only() {
        let _guard = env_lock();
        let _lang_index = EnvVarGuard::remove("WAX_PACK_INDEX");
        let _old_index = EnvVarGuard::remove("WAX_PACK_INDEX_URL");
        let temp = TestDir::new("list-installed");
        let state_path = temp.path().join("state.json");
        let install_dir = temp.path().join("langs/compose/0.4.2");
        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([("0.4.2".to_owned(), InstalledLanguagePack { install_dir })]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_list(
            ListOptions {
                registry_url: None,
                state_path: Some(state_path),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("compose"));
        assert!(output.contains("0.4.2"));
        assert!(output.contains("installed"));
    }

    #[test]
    fn list_ignores_wax_lang_index_env_registry() {
        let _guard = env_lock();
        let temp = TestDir::new("list-env");
        let registry_path = temp.path().join("registry.json");
        fs::write(
            &registry_path,
            r#"[{"id":"compose","version":"0.4.2","api_version":1,"targets":{"test-target":{"url":"file:///tmp/pack.tgz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}}]"#,
        )
        .unwrap();
        let _lang_index = EnvVarGuard::set("WAX_PACK_INDEX", file_url(&registry_path));
        let _old_index = EnvVarGuard::remove("WAX_PACK_INDEX_URL");
        let state_path = temp.path().join("state.json");
        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([(
                "0.4.2".to_owned(),
                InstalledLanguagePack {
                    install_dir: temp.path().join("langs/compose/0.4.2"),
                },
            )]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_list(
            ListOptions {
                registry_url: None,
                state_path: Some(state_path),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("compose"));
        assert!(!output.contains("react"));
    }

    #[test]
    fn resolve_registry_url_uses_default_when_cli_and_env_are_unset() {
        let _guard = env_lock();
        let _lang_index = EnvVarGuard::remove("WAX_PACK_INDEX");
        let resolved = resolve_registry_url(None);
        assert_eq!(
            resolved,
            "https://raw.githubusercontent.com/Daio-io/wax/gh-pages/index.json"
        );
        assert_eq!(resolved, DEFAULT_WAX_PACK_INDEX);
    }

    #[test]
    fn resolve_registry_url_prefers_env_over_default() {
        let _guard = env_lock();
        let expected = "https://example.invalid/index.json";
        let _lang_index = EnvVarGuard::set("WAX_PACK_INDEX", expected);
        let resolved = resolve_registry_url(None);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_registry_url_prefers_cli_over_env() {
        let _guard = env_lock();
        let _lang_index = EnvVarGuard::set("WAX_PACK_INDEX", "https://example.invalid/env.json");
        let resolved = resolve_registry_url(Some("https://example.invalid/cli.json".to_owned()));
        assert_eq!(resolved, "https://example.invalid/cli.json");
    }

    #[test]
    fn parses_language_install_spec_with_optional_version() {
        let latest = LanguageInstallSpec::parse("compose").unwrap();
        assert_eq!(latest.language_id, lang("compose"));
        assert_eq!(latest.version, None);

        let pinned = LanguageInstallSpec::parse("compose@0.4.2").unwrap();
        assert_eq!(pinned.language_id, lang("compose"));
        assert_eq!(pinned.version.as_deref(), Some("0.4.2"));
    }

    #[test]
    fn install_uses_registry_artifact_and_records_global_state() {
        let _guard = env_lock();
        let temp = TestDir::new("install");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());

        let artifact_path = temp.path().join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path().join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        let mut output = Vec::new();
        run_install(
            InstallOptions {
                language_id: lang("compose"),
                version: None,
                registry_url: Some(file_url(&registry_path)),
                target_triple: Some("test-target".to_owned()),
                state_path: None,
            },
            &mut output,
        )
        .unwrap();

        let state =
            wax_core::global_state::load_global_state(temp.path().join("state.json")).unwrap();
        assert!(state.installed_languages[&lang("compose")].contains_key("0.4.2"));
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("installed compose 0.4.2")
        );
    }

    #[test]
    fn install_uses_exact_registry_version_when_requested() {
        let _guard = env_lock();
        let temp = TestDir::new("install-exact");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());

        let artifact_path = temp.path().join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path().join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[
                    {{"id":"compose","version":"0.4.1","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}},
                    {{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}
                ]"#,
                file_url(&artifact_path),
                digest,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        let mut output = Vec::new();
        run_install(
            InstallOptions {
                language_id: lang("compose"),
                version: Some("0.4.1".to_owned()),
                registry_url: Some(file_url(&registry_path)),
                target_triple: Some("test-target".to_owned()),
                state_path: None,
            },
            &mut output,
        )
        .unwrap();

        let state =
            wax_core::global_state::load_global_state(temp.path().join("state.json")).unwrap();
        let versions = &state.installed_languages[&lang("compose")];
        assert!(versions.contains_key("0.4.1"));
        assert!(!versions.contains_key("0.4.2"));
    }

    #[test]
    fn install_latest_uses_semver_ordering() {
        let manifests = vec![
            registry_manifest("compose", "1.9.0"),
            registry_manifest("compose", "1.10.0"),
        ];

        let manifest = manifest_for_language(&manifests, &lang("compose"), None).unwrap();

        assert_eq!(manifest.version, "1.10.0");
    }

    #[test]
    fn exact_version_missing_reports_requested_version() {
        let manifests = vec![registry_manifest("compose", "1.10.0")];

        let err = manifest_for_language(&manifests, &lang("compose"), Some("1.9.0"))
            .expect_err("missing exact version should fail");

        assert!(err.to_string().contains("1.9.0"));
    }

    #[test]
    fn uninstall_removes_all_versions_when_version_is_omitted() {
        let _guard = env_lock();
        let temp = TestDir::new("uninstall");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());
        let state_path = temp.path().join("state.json");
        let install_a = temp.path().join("langs/compose/0.4.1");
        let install_b = temp.path().join("langs/compose/0.4.2");
        fs::create_dir_all(&install_a).unwrap();
        fs::create_dir_all(&install_b).unwrap();

        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([
                (
                    "0.4.1".to_owned(),
                    InstalledLanguagePack {
                        install_dir: install_a.clone(),
                    },
                ),
                (
                    "0.4.2".to_owned(),
                    InstalledLanguagePack {
                        install_dir: install_b.clone(),
                    },
                ),
            ]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_uninstall(
            UninstallOptions {
                language_id: lang("compose"),
                version: None,
                state_path: Some(state_path.clone()),
            },
            &mut output,
        )
        .unwrap();

        let state = wax_core::global_state::load_global_state(state_path).unwrap();
        assert!(!state.installed_languages.contains_key(&lang("compose")));
        assert!(!install_a.exists());
        assert!(!install_b.exists());
    }

    #[test]
    fn uninstall_derives_removal_path_instead_of_trusting_state_install_dir() {
        let _guard = env_lock();
        let temp = TestDir::new("uninstall-safe-path");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());
        let state_path = temp.path().join("state.json");
        let expected_dir = temp.path().join("langs/compose/0.4.2");
        let malicious_dir = temp.path().join("do-not-delete");
        fs::create_dir_all(&expected_dir).unwrap();
        fs::create_dir_all(&malicious_dir).unwrap();
        fs::write(malicious_dir.join("sentinel"), "keep").unwrap();

        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([(
                "0.4.2".to_owned(),
                InstalledLanguagePack {
                    install_dir: malicious_dir.clone(),
                },
            )]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_uninstall(
            UninstallOptions {
                language_id: lang("compose"),
                version: Some("0.4.2".to_owned()),
                state_path: Some(state_path),
            },
            &mut output,
        )
        .unwrap();

        assert!(!expected_dir.exists());
        assert!(malicious_dir.join("sentinel").exists());
    }

    #[test]
    fn uninstall_saves_state_after_each_successful_removal() {
        let _guard = env_lock();
        let temp = TestDir::new("uninstall-partial-failure");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());
        let state_path = temp.path().join("state.json");
        let install_a = temp.path().join("langs/compose/0.4.1");
        let install_b = temp.path().join("langs/compose/0.4.2");
        fs::create_dir_all(&install_a).unwrap();
        fs::create_dir_all(install_b.parent().unwrap()).unwrap();
        fs::write(&install_b, "not a directory").unwrap();

        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([
                (
                    "0.4.1".to_owned(),
                    InstalledLanguagePack {
                        install_dir: temp.path().join("ignored-a"),
                    },
                ),
                (
                    "0.4.2".to_owned(),
                    InstalledLanguagePack {
                        install_dir: temp.path().join("ignored-b"),
                    },
                ),
            ]),
        );
        save_global_state(&state_path, &state).unwrap();

        let err = run_uninstall(
            UninstallOptions {
                language_id: lang("compose"),
                version: None,
                state_path: Some(state_path.clone()),
            },
            &mut Vec::new(),
        )
        .expect_err("second removal should fail");

        assert!(
            err.to_string()
                .contains("remove installed language pack directory")
        );
        let state = wax_core::global_state::load_global_state(state_path).unwrap();
        let versions = &state.installed_languages[&lang("compose")];
        assert!(!versions.contains_key("0.4.1"));
        assert!(versions.contains_key("0.4.2"));
        assert!(!install_a.exists());
        assert!(install_b.exists());
    }

    #[test]
    fn update_installs_latest_registry_version_then_removes_old_versions() {
        let _guard = env_lock();
        let temp = TestDir::new("update");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());

        let old_dir = temp.path().join("langs/compose/0.4.1");
        fs::create_dir_all(&old_dir).unwrap();
        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([(
                "0.4.1".to_owned(),
                InstalledLanguagePack {
                    install_dir: old_dir.clone(),
                },
            )]),
        );
        save_global_state(temp.path().join("state.json"), &state).unwrap();

        let artifact_path = temp.path().join("compose-new.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path().join("registry.json");
        let registry_url = file_url(&registry_path);
        fs::write(
            &registry_path,
            format!(
                r#"[
                    {{"id":"compose","version":"0.4.1","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}},
                    {{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}
                ]"#,
                file_url(&artifact_path),
                digest,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();
        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.lock.json"),
            r#"{"schema_version":1,"engine_api_version":1,"wax_version":"0.0.0","locked_at":null,"languages":{"compose":{"version":"0.4.1","api_version":1,"source":"test","resolved":{"target":"test-target","url":"file:///tmp/old-pack.tgz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","signature":null}}}}"#,
        )
        .unwrap();

        let mut output = Vec::new();
        run_update(
            UpdateOptions {
                language_id: Some(lang("compose")),
                all: false,
                registry_url: Some(registry_url.clone()),
                target_triple: Some("test-target".to_owned()),
                state_path: None,
                repo_root: temp.path().to_path_buf(),
            },
            &mut output,
        )
        .unwrap();

        let state =
            wax_core::global_state::load_global_state(temp.path().join("state.json")).unwrap();
        let versions = &state.installed_languages[&lang("compose")];
        assert!(versions.contains_key("0.4.2"));
        assert!(!versions.contains_key("0.4.1"));
        assert!(!old_dir.exists());

        let lockfile =
            wax_core::config::lockfile::load_lockfile(temp.path().join(".wax/wax.lock.json"))
                .unwrap();
        assert_eq!(lockfile.schema_version, WAX_LOCK_SCHEMA_VERSION);
        assert!(lockfile.registries.is_empty());
        let locked = &lockfile.languages[&lang("compose")];
        assert_eq!(locked.version, "0.4.2");
        assert_eq!(locked.api_version, 1);
        assert_eq!(locked.source, registry_url);
        assert_eq!(locked.resolved.target, "test-target");
        assert_eq!(locked.resolved.url, file_url(&artifact_path));
        assert_eq!(locked.resolved.sha256, digest);
    }

    #[test]
    fn update_refreshes_registry_locks_for_centralized_layout() {
        let _guard = env_lock();
        let temp = TestDir::new("update-centralized-registry");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());

        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.config.json"),
            r#"{"schema_version": 2,"languages":{"compose": {}}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".wax/compose.registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".wax/wax.lock.json"),
            minimal_v1_lockfile_json(),
        )
        .unwrap();

        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([(
                "0.4.1".to_owned(),
                InstalledLanguagePack {
                    install_dir: temp.path().join("langs/compose/0.4.1"),
                },
            )]),
        );
        save_global_state(temp.path().join("state.json"), &state).unwrap();

        let artifact_path = temp.path().join("compose-new.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path().join("registry.json");
        let registry_url = file_url(&registry_path);
        fs::write(
            &registry_path,
            format!(
                r#"[
                    {{"id":"compose","version":"0.4.1","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}},
                    {{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}
                ]"#,
                file_url(&artifact_path),
                digest,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        run_update(
            UpdateOptions {
                language_id: Some(lang("compose")),
                all: false,
                registry_url: Some(registry_url),
                target_triple: Some("test-target".to_owned()),
                state_path: None,
                repo_root: temp.path().to_path_buf(),
            },
            &mut Vec::new(),
        )
        .unwrap();

        let lockfile =
            wax_core::config::lockfile::load_lockfile(temp.path().join(".wax/wax.lock.json"))
                .unwrap();
        let registry = lockfile
            .registries
            .get(&lang("compose"))
            .expect("compose registry lock should exist after update");
        assert_eq!(registry.source, ".wax/compose.registry.json");
        assert_eq!(registry.sha256.len(), 64);
        assert_eq!(lockfile.schema_version, WAX_LOCK_SCHEMA_VERSION);
        assert_eq!(lockfile.languages[&lang("compose")].version, "0.4.2");
    }

    #[test]
    fn install_rolls_back_promoted_directory_when_recording_state_fails() {
        let _guard = env_lock();
        let temp = TestDir::new("install-rollback");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());

        let artifact_path = temp.path().join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path().join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();
        let bad_state_path = temp.path().join("state-dir");
        fs::create_dir_all(&bad_state_path).unwrap();

        let err = run_install(
            InstallOptions {
                language_id: lang("compose"),
                version: None,
                registry_url: Some(file_url(&registry_path)),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(bad_state_path),
            },
            &mut Vec::new(),
        )
        .expect_err("recording state should fail");

        assert!(err.to_string().contains("global state"));
        assert!(!temp.path().join("langs/compose/0.4.2").exists());
    }

    #[test]
    fn install_pinned_manifest_keeps_reused_directory_when_recording_state_fails() {
        let _guard = env_lock();
        let temp = TestDir::new("init-reuse-rollback");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path());

        let install_dir = temp.path().join("langs/compose/0.4.2");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(install_dir.join("wax-lang-compose"), "#!/bin/sh\nexit 0\n").unwrap();

        let bad_state_path = temp.path().join("state-dir");
        fs::create_dir_all(&bad_state_path).unwrap();
        let manifest = registry_manifest("compose", "0.4.2");
        let artifact = RegistryArtifact {
            url: "file:///tmp/not-used-on-reuse.tgz".to_owned(),
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        };

        let err = install_pinned_manifest(
            &manifest,
            "test-target",
            &artifact,
            Some(bad_state_path),
            &mut Vec::new(),
        )
        .expect_err("recording state should fail");

        assert!(err.to_string().contains("global state"));
        assert!(install_dir.exists());
    }

    #[test]
    fn clap_accepts_update_all_without_language_id() {
        use clap::Parser;

        crate::cli::Cli::try_parse_from([
            "wax",
            "language",
            "update",
            "--all",
            "--pack-index",
            "file:///tmp/registry.json",
        ])
        .unwrap();
    }

    #[test]
    fn doctor_reports_union_of_config_lock_and_installed_languages() {
        let temp = TestDir::new("doctor");
        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.config.json"),
            r#"{"schema_version": 2,"languages":{"compose": {}}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".wax/wax.lock.json"),
            r#"{"schema_version":1,"engine_api_version":1,"wax_version":"0.0.0","locked_at":null,"languages":{"compose":{"version":"0.4.2","api_version":1,"source":"test","resolved":{"target":"test-target","url":"file:///tmp/pack.tgz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","signature":null}},"lock-only":{"version":"2.0.0","api_version":1,"source":"test","resolved":{"target":"test-target","url":"file:///tmp/lock-only.tgz","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","signature":null}}}}"#,
        )
        .unwrap();
        let install_dir = temp.path().join("langs/compose/0.4.2");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(
            install_dir.join("manifest.json"),
            r#"{"id":"compose","version":"0.4.2","api_version":1,"command":["./wax-lang-compose","--stdio"],"ecosystem":"compose","parser_name":"compose","parser_version":"0.4.2"}"#,
        )
        .unwrap();

        let state_path = temp.path().join("state.json");
        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([("0.4.2".to_owned(), InstalledLanguagePack { install_dir })]),
        );
        state.installed_languages.insert(
            lang("installed-only"),
            BTreeMap::from([(
                "3.1.4".to_owned(),
                InstalledLanguagePack {
                    install_dir: temp.path().join("langs/installed-only/3.1.4"),
                },
            )]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_doctor(
            DoctorOptions {
                repo_root: temp.path().to_path_buf(),
                state_path: Some(state_path),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("compose"));
        assert!(output.contains("enabled: yes"));
        assert!(output.contains("installed: 0.4.2"));
        assert!(output.contains("lock: 0.4.2"));
        assert!(output.contains("missing binary: yes"));

        assert!(!output.contains("language: react"));

        assert!(output.contains("language: lock-only"));
        assert!(output.contains("enabled: missing"));
        assert!(output.contains("lock: 2.0.0"));

        assert!(output.contains("language: installed-only"));
        assert!(output.contains("enabled: missing"));
        assert!(output.contains("installed: 3.1.4"));
        assert!(output.contains("lock: missing"));
    }

    #[test]
    fn doctor_reports_present_binary_as_not_missing() {
        let temp = TestDir::new("doctor-present-binary");
        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.config.json"),
            r#"{"schema_version": 2,"languages":{"compose": {}}}"#,
        )
        .unwrap();
        let install_dir = temp.path().join("langs/compose/0.4.2");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(
            install_dir.join("manifest.json"),
            r#"{"id":"compose","version":"0.4.2","api_version":1,"command":["./wax-lang-compose","--stdio"],"ecosystem":"compose","parser_name":"compose","parser_version":"0.4.2"}"#,
        )
        .unwrap();
        fs::write(install_dir.join("wax-lang-compose"), "#!/bin/sh\nexit 0\n").unwrap();

        let state_path = temp.path().join("state.json");
        let mut state = GlobalState::default();
        state.installed_languages.insert(
            lang("compose"),
            BTreeMap::from([("0.4.2".to_owned(), InstalledLanguagePack { install_dir })]),
        );
        save_global_state(&state_path, &state).unwrap();

        let mut output = Vec::new();
        run_doctor(
            DoctorOptions {
                repo_root: temp.path().to_path_buf(),
                state_path: Some(state_path),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(DEFAULT_WAX_PACK_INDEX));
        assert!(output.contains("pack index:"));
        assert!(output.contains("source: default"));
        assert!(output.contains("missing binary: no"));
    }

    #[test]
    fn language_update_uses_centralized_lockfile_when_present() {
        let temp = TestDir::new("language-centralized-lock");
        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.lock.json"),
            minimal_lockfile_json(),
        )
        .unwrap();
        fs::write(temp.path().join("wax.lock.json"), legacy_lockfile_json()).unwrap();

        let lockfile = load_optional_lockfile_for_repo(temp.path())
            .unwrap()
            .unwrap();

        assert!(lockfile.languages.contains_key(&lang("compose")));
    }

    #[test]
    fn language_doctor_reads_centralized_config() {
        let temp = TestDir::new("language-centralized-config");
        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.config.json"),
            r#"{"schema_version": 2,"languages":{"compose": {}}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".wax/wax.lock.json"),
            minimal_lockfile_json(),
        )
        .unwrap();

        let mut output = Vec::new();
        run_doctor(
            DoctorOptions {
                repo_root: temp.path().to_path_buf(),
                state_path: Some(temp.path().join("state.json")),
            },
            &mut output,
        )
        .unwrap();

        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("language: compose")
        );
    }

    #[test]
    fn language_update_refreshes_registry_locks_for_enabled_languages() {
        let temp = TestDir::new("language-registry-refresh");
        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.config.json"),
            r#"{"schema_version": 2,"languages":{"compose": {}}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".wax/compose.registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".wax/wax.lock.json"),
            minimal_v1_lockfile_json(),
        )
        .unwrap();

        refresh_registry_locks_for_repo(temp.path()).unwrap();

        let lock =
            wax_core::config::lockfile::load_lockfile(temp.path().join(".wax/wax.lock.json"))
                .unwrap();
        let registry = lock
            .registries
            .get(&lang("compose"))
            .expect("compose registry lock should exist");
        assert_eq!(registry.source, ".wax/compose.registry.json");
        assert_eq!(registry.sha256.len(), 64);
        assert_eq!(
            lock.schema_version,
            wax_core::config::lockfile::WAX_LOCK_SCHEMA_VERSION
        );
    }

    #[test]
    fn doctor_reports_env_registry_source() {
        let _guard = env_lock();
        let temp = TestDir::new("doctor-env-source");
        let _lang_index = EnvVarGuard::set("WAX_PACK_INDEX", "https://example.invalid/index.json");

        fs::create_dir_all(temp.path().join(".wax")).unwrap();
        fs::write(
            temp.path().join(".wax/wax.config.json"),
            r#"{"schema_version": 2,"languages":{}}"#,
        )
        .unwrap();

        let mut output = Vec::new();
        run_doctor(
            DoctorOptions {
                repo_root: temp.path().to_path_buf(),
                state_path: Some(temp.path().join("state.json")),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("pack index: https://example.invalid/index.json"));
        assert!(output.contains("source: WAX_PACK_INDEX"));
    }

    fn minimal_lockfile_json() -> String {
        r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "locked_at": null,
  "registries": {},
  "languages": {
    "compose": {
      "version": "0.4.2",
      "api_version": 1,
      "source": "test",
      "resolved": {
        "target": "test-target",
        "url": "file:///tmp/pack.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }
    }
  }
}"#
        .to_owned()
    }

    fn legacy_lockfile_json() -> String {
        r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "locked_at": null,
  "languages": {
    "legacy-only": {
      "version": "9.9.9",
      "api_version": 1,
      "source": "test",
      "resolved": {
        "target": "test-target",
        "url": "file:///tmp/legacy.tgz",
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "signature": null
      }
    }
  }
}"#
        .to_owned()
    }

    fn minimal_v1_lockfile_json() -> String {
        r#"{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "locked_at": null,
  "languages": {
    "compose": {
      "version": "0.4.2",
      "api_version": 1,
      "source": "test",
      "resolved": {
        "target": "test-target",
        "url": "file:///tmp/pack.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }
    }
  }
}"#
        .to_owned()
    }

    fn lang(id: &str) -> LanguageId {
        LanguageId::try_from(id).unwrap()
    }

    fn registry_manifest(id: &str, version: &str) -> RegistryManifest {
        RegistryManifest {
            id: lang(id),
            version: version.to_owned(),
            api_version: 1,
            targets: BTreeMap::new(),
        }
    }

    fn file_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    fn write_pack_artifact(path: &Path, binary: &str) -> String {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use sha2::{Digest, Sha256};

        let mut bytes = Vec::new();
        {
            let gz = GzEncoder::new(&mut bytes, Compression::default());
            let mut tar = tar::Builder::new(gz);
            let body = b"#!/bin/sh\nexit 0\n";
            let mut header = tar::Header::new_gnu();
            header.set_path(binary).unwrap();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append(&header, body.as_slice()).unwrap();
            tar.finish().unwrap();
        }
        fs::write(path, &bytes).unwrap();
        Sha256::digest(&bytes)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }

    struct TestDir {
        root: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "wax-cli-{name}-{}-{}",
                std::process::id(),
                std::thread::current().name().unwrap_or("test")
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(name);
            unsafe {
                std::env::set_var(name, value);
            }
            Self { name, previous }
        }

        fn remove(name: &'static str) -> Self {
            let previous = std::env::var_os(name);
            unsafe {
                std::env::remove_var(name);
            }
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.name, value),
                    None => std::env::remove_var(self.name),
                }
            }
        }
    }
}
