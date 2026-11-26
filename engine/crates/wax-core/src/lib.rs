#![deny(missing_docs)]

//! Core engine functionality for wax.

mod advisory_lock;
mod atomic_file;
/// Shared durable-write types for the CLI's lockfile persistence boundary.
#[doc(hidden)]
pub use atomic_file::{AtomicWriteError, AtomicWriteOptions, write_atomically};
pub mod adoption_merge;
pub mod auto_install;
pub mod config;
pub mod defaults;
pub mod digest;
pub mod global_state;
pub mod install;
pub mod pack_store;
pub mod paths;
mod process_control;
pub mod progress;
pub mod registry;
pub mod registry_attachments;
pub mod registry_cache;
pub mod registry_discovery;
pub mod registry_git;
pub mod registry_lock;
pub mod registry_lookup;
pub mod registry_lookup_error;
pub mod registry_memory;
pub mod registry_merge;
pub mod registry_search;
pub mod registry_source;
pub use registry_lookup_error::{RegistryLookupError, RegistryLookupErrorCode};
mod root_group;
mod scan_plan;
pub mod subprocess_discover;
mod subprocess_exchange;
pub mod subprocess_lang;
pub mod sync;
pub mod token_inference;
pub mod validate;

use adoption_merge::merge_language_scans_with_options;
use auto_install::{AutoInstallPolicyInput, InstalledManifest, PackIndexArtifact};
use config::lockfile::{LockfileError, WaxLock, load_lockfile, save_lockfile};
use config::waxrc::{WaxRc, WaxRcError, load_waxrc};
use global_state::{GlobalState, GlobalStateError, load_global_state};
use install::InstallError;
use pack_store::{InstalledManifestFile, PackArtifact, PackStore, PackStoreError};
use paths::{PathsError, WaxHome};
use progress::{ScanProgress, ScanProgressEvent};
use scan_plan::{PlannedScan, ScanPlan, ScanSelection, prepare_scan};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subprocess_lang::{
    LanguageCancellationToken, LanguageError, SubprocessLanguageExtractor,
    SubprocessLanguageManifest,
};
use thiserror::Error;
use wax_contract::{LanguageId, MergedScan, ScanFacts};
use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};

/// Spec default: 10 minutes per language pack (`docs/specs/2026-05-16-language-packs-and-distribution.md`).
const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_secs(600);
const SCAN_TIMEOUT_ENV: &str = "WAX_SCAN_TIMEOUT_SECS";

/// Resolves the per-language scan subprocess timeout.
///
/// `WAX_SCAN_TIMEOUT_SECS` overrides the default when set to a non-negative integer.
/// Invalid values fall back to [`DEFAULT_SCAN_TIMEOUT`].
fn resolve_scan_timeout(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.trim().parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_SCAN_TIMEOUT)
}

fn scan_timeout() -> Duration {
    resolve_scan_timeout(std::env::var(SCAN_TIMEOUT_ENV).ok().as_deref())
}
#[derive(Debug)]
struct ScanJob {
    language_id: LanguageId,
    command: Vec<String>,
    config: ScanConfig,
}

type ScanJobResult = Result<(LanguageId, ScanFacts), EngineError>;

enum ScanWorkerMessage {
    Started(LanguageId),
    Job(Box<ScanJobResult>),
    Done,
}

/// Engine scan orchestrator for repository scans.
#[derive(Debug, Default)]
pub struct Engine;

/// Runtime options for repository scans.
#[derive(Debug)]
pub struct ScanOptions {
    /// Overrides the wax home. When absent, resolve it when global state is needed.
    pub wax_home: Option<WaxHome>,
    /// Overrides `.wax/wax.config.json` `engine.scan_concurrency` when set.
    ///
    /// Values less than 1 are treated as serial execution.
    pub scan_concurrency: Option<u32>,
    /// Restricts a configured grouped-root scan to one repository-wide root group.
    pub root_group: Option<String>,
    /// Allows scan to install missing locked language packs before execution.
    pub allow_auto_install: bool,
    /// Optional progress callbacks for CLI or tooling.
    pub progress: ScanProgress,
    /// In-memory config and lockfile for ephemeral scans that must not write repo files.
    pub ephemeral: Option<EphemeralScanConfig>,
}

/// In-memory repository config used for ephemeral scans.
#[derive(Debug)]
pub struct EphemeralScanConfig {
    /// Parsed wax config for the scan invocation.
    pub waxrc: WaxRc,
    /// Parsed lockfile for the scan invocation.
    pub lockfile: WaxLock,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            wax_home: None,
            scan_concurrency: None,
            root_group: None,
            allow_auto_install: true,
            progress: ScanProgress::default(),
            ephemeral: None,
        }
    }
}

/// Typed failures while resolving and running repository scans.
#[derive(Debug, Error)]
pub enum EngineError {
    /// `.wax/wax.config.json` could not be loaded from the repository root.
    #[error(transparent)]
    WaxRc(#[from] WaxRcError),
    /// `.wax/wax.lock.json` could not be loaded from the repository root.
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
    /// Installed pack artifact metadata did not match the requested artifact pin.
    #[error(
        "installed artifact mismatch for language {language_id} version {version}; remove it before installing the requested artifact"
    )]
    InstalledArtifactMismatch {
        /// Language id being resolved.
        language_id: LanguageId,
        /// Requested language version.
        version: String,
    },
    /// Registry index could not be loaded while evaluating auto-install policy.
    #[error(transparent)]
    Registry(#[from] registry::RegistryError),
    /// Design-system registry source could not be resolved before scan.
    #[error(transparent)]
    RegistrySource(#[from] registry_source::RegistrySourceError),
    /// Design-system registry lock did not match resolved registry content.
    #[error("registry lock mismatch for language {language_id}: {reason}")]
    RegistryLock {
        /// Language id whose registry lock failed.
        language_id: LanguageId,
        /// Human-readable mismatch reason.
        reason: String,
    },
    /// Auto-install policy blocked scan execution.
    #[error("scan auto-install policy blocked execution: {errors:?}")]
    AutoInstallPolicyBlocked {
        /// Typed policy errors returned by evaluation.
        errors: Vec<auto_install::AutoInstallPolicyError>,
    },
    /// Auto-install would be required, but scan auto-install is disabled.
    #[error(
        "scan requires language packs to be installed; run `wax language install` or enable auto-install: {plans:?}"
    )]
    AutoInstallRequired {
        /// Required installs returned by policy.
        plans: Vec<auto_install::InstallPlan>,
    },
    /// A language pack could not be installed before scan execution.
    #[error(transparent)]
    Install(#[from] InstallError),
    /// A promoted language pack could not be removed after state persistence failed.
    #[error("failed to clean up installed language pack at {path}: {source}")]
    InstallCleanup {
        /// Install directory cleanup path.
        path: String,
        /// Underlying source error.
        #[source]
        source: io::Error,
    },
    /// A language scan subprocess failed.
    #[error(transparent)]
    Language(#[from] LanguageError),
    /// Scan output could not be written.
    #[error("failed to write scan output to {path}: {source}")]
    ScanOutput {
        /// Output path.
        path: String,
        /// Underlying source error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Atomic scan-output replacement failed.
    #[error(transparent)]
    AtomicWrite(#[from] AtomicWriteError),
    /// Scan facts failed contract validation during merge.
    #[error(transparent)]
    ScanFacts(#[from] wax_contract::ScanFactsError),
    /// A language scan worker thread panicked.
    #[error("language scan worker panicked")]
    ScanWorkerPanicked,
    /// The requested root group is not configured in any language entry.
    #[error("unknown root group `{root_group}`; configured root groups: {configured:?}")]
    UnknownRootGroup {
        /// Group requested by the scan invocation.
        root_group: String,
        /// Configured repository-wide group ids in deterministic order.
        configured: Vec<String>,
    },
}

impl From<PackStoreError> for EngineError {
    fn from(error: PackStoreError) -> Self {
        match error {
            PackStoreError::Install(error) => Self::Install(error),
            PackStoreError::GlobalState(error) => Self::GlobalState(error),
            PackStoreError::Paths(error) => Self::Paths(error),
            PackStoreError::ManifestRead { path, source } => {
                Self::InstalledManifest { path, source }
            }
            PackStoreError::ManifestMismatch {
                language_id,
                expected_version,
            } => Self::InstalledManifestMismatch {
                language_id,
                expected_version,
            },
            PackStoreError::ArtifactMismatch {
                language_id,
                version,
            } => Self::InstalledArtifactMismatch {
                language_id,
                version,
            },
            PackStoreError::Cleanup { path, source } => Self::InstallCleanup { path, source },
        }
    }
}

impl Engine {
    /// Scans a repository by running enabled language packs.
    ///
    /// On success, scan output is persisted under `.wax/out/` in the scanned
    /// repository. The merged scan is written to `.wax/out/scan-merged.json`,
    /// and per-language facts are written to `.wax/out/languages/`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::WaxRc`] or [`EngineError::Lockfile`] for invalid
    /// repository inputs; [`EngineError::GlobalState`] or [`EngineError::Paths`]
    /// for unavailable global state; [`EngineError::RegistrySource`] or
    /// [`EngineError::RegistryLock`] for invalid registry inputs;
    /// [`EngineError::InstalledManifest`] or
    /// [`EngineError::InstalledManifestMismatch`] for invalid installed packs;
    /// [`EngineError::Registry`], [`EngineError::AutoInstallPolicyBlocked`],
    /// [`EngineError::Install`], or [`EngineError::InstallCleanup`] when automatic
    /// installation cannot complete; [`EngineError::Language`] or
    /// [`EngineError::ScanWorkerPanicked`] when a scan worker fails; and
    /// [`EngineError::ScanFacts`] or [`EngineError::ScanOutput`] when merged facts
    /// are invalid or cannot be persisted.
    pub fn scan_repo(repo_root: impl AsRef<Path>) -> Result<MergedScan, EngineError> {
        Self::scan_repo_with_options(repo_root, ScanOptions::default())
    }

    /// Scans a repository by running enabled language packs with runtime options.
    ///
    /// On success, scan output is persisted under `.wax/out/` in the scanned
    /// repository. The merged scan is written to `.wax/out/scan-merged.json`,
    /// and per-language facts are written to `.wax/out/languages/`. Output
    /// write failures are returned as [`EngineError::ScanOutput`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::WaxRc`] or [`EngineError::Lockfile`] for invalid
    /// repository inputs; [`EngineError::GlobalState`] or [`EngineError::Paths`]
    /// for unavailable global state; [`EngineError::RegistrySource`] or
    /// [`EngineError::RegistryLock`] for invalid registry inputs;
    /// [`EngineError::InstalledManifest`] or
    /// [`EngineError::InstalledManifestMismatch`] for invalid installed packs;
    /// [`EngineError::AutoInstallRequired`] when installation is disabled but
    /// needed; [`EngineError::Registry`],
    /// [`EngineError::AutoInstallPolicyBlocked`], [`EngineError::Install`], or
    /// [`EngineError::InstallCleanup`] when automatic installation cannot
    /// complete; [`EngineError::Language`] or
    /// [`EngineError::ScanWorkerPanicked`] when a scan worker fails; and
    /// [`EngineError::ScanFacts`] or [`EngineError::ScanOutput`] when merged facts
    /// are invalid or cannot be persisted.
    ///
    /// # Panics
    ///
    /// Panics if the callback configured in [`ScanOptions::progress`] panics
    /// while handling a progress event.
    pub fn scan_repo_with_options(
        repo_root: impl AsRef<Path>,
        options: ScanOptions,
    ) -> Result<MergedScan, EngineError> {
        let repo_root = repo_root.as_ref();
        let ScanOptions {
            wax_home,
            scan_concurrency,
            root_group,
            allow_auto_install,
            progress,
            ephemeral,
        } = options;
        progress.emit(ScanProgressEvent::Preparing);
        let is_ephemeral = ephemeral.is_some();
        let (waxrc, lockfile) = if let Some(ephemeral) = ephemeral {
            (ephemeral.waxrc, ephemeral.lockfile)
        } else {
            let repo_files = config::repo_files::discover_repo_files(repo_root);
            (
                load_waxrc(&repo_files.config_path)?,
                load_lockfile(&repo_files.lockfile_path)?,
            )
        };
        let selection = ScanSelection {
            root_group,
            scan_concurrency,
        };
        let (store, state) = match load_scan_pack_state(wax_home) {
            Ok(pack_state) => pack_state,
            Err(error) => {
                // An invalid group takes priority over unavailable pack state.
                scan_plan::configured_root_groups(&waxrc, &selection)?;
                return Err(error);
            }
        };
        let ScanPlan {
            scans,
            merge,
            concurrency,
            lockfile,
            lockfile_changed,
        } = prepare_scan(repo_root, waxrc, lockfile, &selection)?;
        if lockfile_changed && !is_ephemeral {
            let path = config::repo_files::discover_repo_files(repo_root).lockfile_path;
            save_lockfile(path, &lockfile)?;
        }

        let jobs = ensure_packs(
            scans,
            &lockfile,
            &store,
            &state,
            allow_auto_install,
            &progress,
        )?;
        let languages = run_scan_jobs(repo_root, jobs, concurrency, &progress)?;
        let merged =
            merge_language_scans_with_options(languages, &merge).map_err(EngineError::ScanFacts)?;
        progress.emit(ScanProgressEvent::WritingOutputs);
        write_scan_outputs(repo_root, &merged)?;

        Ok(merged)
    }
}

fn load_scan_pack_state(
    wax_home: Option<WaxHome>,
) -> Result<(PackStore, GlobalState), EngineError> {
    let home = match wax_home {
        Some(home) => home,
        None => WaxHome::from_env()?,
    };
    let state = load_global_state(home.state_file())?;
    Ok((PackStore::new(home), state))
}

fn ensure_packs(
    scans: Vec<PlannedScan>,
    lockfile: &WaxLock,
    store: &PackStore,
    state: &GlobalState,
    allow_auto_install: bool,
    progress: &ScanProgress,
) -> Result<Vec<ScanJob>, EngineError> {
    let enabled_ids = scans.iter().map(|scan| scan.language_id.clone()).collect();
    let installed_manifests = collect_installed_manifests(&enabled_ids, lockfile, state)?;
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
        if !allow_auto_install {
            let plans = missing_install_ids
                .iter()
                .map(|language_id| auto_install::InstallPlan {
                    language_id: language_id.clone(),
                    version: lockfile.languages[language_id].version.clone(),
                    sha256: lockfile.languages[language_id].resolved.sha256.clone(),
                })
                .collect();
            return Err(EngineError::AutoInstallRequired { plans });
        }

        let pack_index_artifacts = collect_pack_index_artifacts(&missing_install_ids, lockfile)?;
        let policy_with_auto_install =
            auto_install::evaluate_auto_install_policy(&AutoInstallPolicyInput {
                enabled_language_ids: enabled_ids.clone(),
                locked_languages: lockfile.languages.clone(),
                installed_manifests: collect_installed_manifests(&enabled_ids, lockfile, state)?,
                allow_auto_install: true,
                pack_index_artifacts: pack_index_artifacts.clone(),
            });

        if !policy_with_auto_install.errors.is_empty() {
            return Err(EngineError::AutoInstallPolicyBlocked {
                errors: policy_with_auto_install.errors,
            });
        }
        if !policy_with_auto_install.needs_install.is_empty() {
            execute_install_plans(
                &policy_with_auto_install.needs_install,
                lockfile,
                &pack_index_artifacts,
                store,
                progress,
            )?;
        }
    }

    let mut jobs = Vec::with_capacity(scans.len());
    for PlannedScan {
        language_id,
        config,
    } in scans
    {
        let locked = &lockfile.languages[&language_id];
        let pack_spec = store.installed(&language_id, &locked.version)?;
        jobs.push(ScanJob {
            language_id,
            command: pack_spec.command,
            config,
        });
    }
    Ok(jobs)
}

fn run_scan_jobs(
    repo_root: &Path,
    jobs: Vec<ScanJob>,
    scan_concurrency: usize,
    progress: &ScanProgress,
) -> Result<BTreeMap<LanguageId, ScanFacts>, EngineError> {
    if jobs.is_empty() {
        return Ok(BTreeMap::new());
    }

    let repo_root = repo_root.display().to_string();
    let total_jobs = jobs.len();
    let worker_count = scan_concurrency.min(total_jobs);
    let parallel_scans = worker_count > 1;
    if parallel_scans {
        progress.emit(ScanProgressEvent::LanguagesScanning {
            completed: 0,
            total: total_jobs,
        });
    }
    let queue = Arc::new(Mutex::new(jobs.into_iter().collect::<VecDeque<_>>()));
    let stop = Arc::new(AtomicBool::new(false));
    let cancellation = LanguageCancellationToken::new();
    let (tx, rx) = mpsc::channel();
    let mut languages = BTreeMap::new();

    let handles = (0..worker_count)
        .map(|_| {
            let queue = Arc::clone(&queue);
            let stop = Arc::clone(&stop);
            let cancellation = cancellation.clone();
            let repo_root = repo_root.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let job = match queue.lock() {
                        Ok(mut queue_guard) => queue_guard.pop_front(),
                        Err(_) => break,
                    };
                    let Some(job) = job else {
                        break;
                    };

                    let language_id = job.language_id.clone();
                    if tx.send(ScanWorkerMessage::Started(language_id)).is_err() {
                        break;
                    }
                    let result = run_scan_job(repo_root.clone(), job, &cancellation);
                    if result.is_err() {
                        stop.store(true, Ordering::SeqCst);
                    }
                    if tx.send(ScanWorkerMessage::Job(Box::new(result))).is_err() {
                        break;
                    }
                }
                let _ = tx.send(ScanWorkerMessage::Done);
            })
        })
        .collect::<Vec<_>>();
    drop(tx);

    let mut first_error = None;
    let mut finished_workers = 0;
    let mut completed_scans = 0;
    while finished_workers < worker_count {
        match rx.recv() {
            Ok(ScanWorkerMessage::Started(language_id)) => {
                if !parallel_scans {
                    progress.emit(ScanProgressEvent::Scanning { language_id });
                }
            }
            Ok(ScanWorkerMessage::Job(result)) => match *result {
                Ok((language_id, facts)) => {
                    if first_error.is_none() {
                        if parallel_scans {
                            completed_scans += 1;
                            progress.emit(ScanProgressEvent::LanguagesScanning {
                                completed: completed_scans,
                                total: total_jobs,
                            });
                        } else {
                            progress.emit(ScanProgressEvent::ScanComplete {
                                language_id: language_id.clone(),
                            });
                        }
                        languages.insert(language_id, facts);
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                        stop.store(true, Ordering::SeqCst);
                        cancellation.cancel();
                    }
                }
            },
            Ok(ScanWorkerMessage::Done) => {
                finished_workers += 1;
            }
            Err(_) => {
                break;
            }
        }
    }

    for handle in handles {
        if handle.join().is_err() {
            stop.store(true, Ordering::SeqCst);
            cancellation.cancel();
            if first_error.is_none() {
                first_error = Some(EngineError::ScanWorkerPanicked);
            }
        }
    }

    if let Some(error) = first_error {
        return Err(error);
    }

    Ok(languages)
}

fn run_scan_job(
    repo_root: String,
    job: ScanJob,
    cancellation: &LanguageCancellationToken,
) -> Result<(LanguageId, ScanFacts), EngineError> {
    let extractor = SubprocessLanguageExtractor::new(SubprocessLanguageManifest {
        command: job.command,
        timeout: scan_timeout(),
    });
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: job.language_id.clone(),
        repo_root,
        snapshot_id: new_snapshot_id(),
        config: job.config,
    };
    let facts = extractor.scan_with_cancellation(request, cancellation)?;
    Ok((job.language_id, facts))
}

fn write_scan_outputs(repo_root: &Path, merged: &MergedScan) -> Result<(), EngineError> {
    let out_dir = repo_root.join(".wax/out");
    let languages_dir = out_dir.join("languages");
    create_output_dir(&languages_dir)?;

    for (language_id, facts) in &merged.languages {
        write_json_atomic(
            &languages_dir.join(format!("{}.json", language_id.as_str())),
            facts,
        )?;
    }
    write_json_atomic(&out_dir.join("scan-merged.json"), merged)?;
    remove_stale_language_outputs(&languages_dir, merged)
}

fn create_output_dir(path: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(path).map_err(|source| EngineError::ScanOutput {
        path: path.display().to_string(),
        source: Box::new(source),
    })
}

fn remove_stale_language_outputs(
    languages_dir: &Path,
    merged: &MergedScan,
) -> Result<(), EngineError> {
    let expected_files = merged
        .languages
        .keys()
        .map(|language_id| format!("{}.json", language_id.as_str()))
        .collect::<BTreeSet<_>>();

    let entries = fs::read_dir(languages_dir).map_err(|source| EngineError::ScanOutput {
        path: languages_dir.display().to_string(),
        source: Box::new(source),
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| EngineError::ScanOutput {
            path: languages_dir.display().to_string(),
            source: Box::new(source),
        })?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.ends_with(".json") || expected_files.contains(file_name.as_ref()) {
            continue;
        }

        let path = entry.path();
        fs::remove_file(&path).map_err(|source| EngineError::ScanOutput {
            path: path.display().to_string(),
            source: Box::new(source),
        })?;
    }

    Ok(())
}

fn write_json_atomic<T>(path: &Path, value: &T) -> Result<(), EngineError>
where
    T: Serialize,
{
    let mut contents =
        serde_json::to_vec_pretty(value).map_err(|source| EngineError::ScanOutput {
            path: path.display().to_string(),
            source: Box::new(source),
        })?;
    contents.push(b'\n');
    write_atomically(path, &contents, AtomicWriteOptions::default())?;
    Ok(())
}

fn collect_installed_manifests(
    enabled_ids: &BTreeSet<LanguageId>,
    lockfile: &config::lockfile::WaxLock,
    state: &global_state::GlobalState,
) -> Result<BTreeMap<LanguageId, Vec<InstalledManifest>>, EngineError> {
    let mut by_language = BTreeMap::new();
    for language_id in enabled_ids {
        let Some(locked) = lockfile.languages.get(language_id) else {
            continue;
        };
        match PackStore::installed_manifest(state, language_id, &locked.version) {
            Ok((_, manifest)) => {
                by_language.insert(language_id.clone(), vec![manifest.metadata]);
            }
            Err(PackStoreError::ManifestMismatch { .. }) => {}
            Err(error) => return Err(error.into()),
        }
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

fn execute_install_plans(
    plans: &[auto_install::InstallPlan],
    lockfile: &config::lockfile::WaxLock,
    pack_index_artifacts: &BTreeMap<LanguageId, BTreeMap<String, Vec<PackIndexArtifact>>>,
    store: &PackStore,
    progress: &ScanProgress,
) -> Result<(), EngineError> {
    for plan in plans {
        progress.emit(ScanProgressEvent::Installing {
            language_id: plan.language_id.clone(),
            version: plan.version.clone(),
        });
        let locked = &lockfile.languages[&plan.language_id];
        let artifact = pack_index_artifacts
            .get(&plan.language_id)
            .and_then(|versions| versions.get(&plan.version))
            .and_then(|artifacts| {
                artifacts
                    .iter()
                    .find(|artifact| artifact.target == locked.resolved.target)
            })
            .ok_or_else(|| EngineError::AutoInstallPolicyBlocked {
                errors: vec![
                    auto_install::AutoInstallPolicyError::MissingPackIndexEntry {
                        language_id: plan.language_id.clone(),
                        version: plan.version.clone(),
                    },
                ],
            })?;

        let installed = store.install(
            &plan.language_id,
            &plan.version,
            PackArtifact {
                target: &locked.resolved.target,
                url: &locked.resolved.url,
                sha256: &plan.sha256,
                pack_index_sha256: Some(&artifact.sha256),
                api_version: locked.api_version,
            },
        )?;
        let manifest = InstalledManifestFile::load(&installed.install_dir)?;
        if !auto_install::installed_manifest_matches_locked(&manifest.metadata, locked) {
            return Err(InstallError::AlreadyInstalled {
                path: installed.install_dir.display().to_string(),
            }
            .into());
        }
    }
    Ok(())
}

fn new_snapshot_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("scan-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_timeout_defaults_to_ten_minutes() {
        assert_eq!(
            DEFAULT_SCAN_TIMEOUT,
            Duration::from_secs(600),
            "DEFAULT_SCAN_TIMEOUT must remain the spec 10-minute default"
        );
        assert_eq!(
            resolve_scan_timeout(None),
            DEFAULT_SCAN_TIMEOUT,
            "unset WAX_SCAN_TIMEOUT_SECS must use the default"
        );
    }

    #[test]
    fn scan_timeout_honors_wax_scan_timeout_secs_override() {
        assert_eq!(resolve_scan_timeout(Some("45")), Duration::from_secs(45));
        assert_eq!(resolve_scan_timeout(Some("0")), Duration::ZERO);
        assert_eq!(
            resolve_scan_timeout(Some("not-a-number")),
            DEFAULT_SCAN_TIMEOUT
        );
        assert_eq!(resolve_scan_timeout(Some("")), DEFAULT_SCAN_TIMEOUT);
        assert_eq!(resolve_scan_timeout(Some("   ")), DEFAULT_SCAN_TIMEOUT);
        assert_eq!(resolve_scan_timeout(Some("-1")), DEFAULT_SCAN_TIMEOUT);
        assert_eq!(resolve_scan_timeout(Some(" 90 ")), Duration::from_secs(90));
    }
}

/// Built-in public catalogue discovery and exact bundle installation.
pub mod registry_catalog;
