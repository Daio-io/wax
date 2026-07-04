#![deny(missing_docs)]

//! Core engine functionality for wax.

pub mod adoption_merge;
pub mod auto_install;
pub mod config;
pub mod defaults;
pub mod global_state;
pub mod install;
pub mod paths;
pub mod progress;
pub mod registry;
pub mod registry_discovery;
pub mod registry_lock;
pub mod registry_memory;
pub mod registry_source;
pub mod subprocess_discover;
pub mod subprocess_lang;
pub mod sync;
pub mod validate;

use adoption_merge::merge_language_scans_with_parent_scope_limit;
use auto_install::{AutoInstallPolicyInput, InstalledManifest, PackIndexArtifact};
use config::lockfile::{LockfileError, WaxLock, load_lockfile};
use config::waxrc::{WaxRc, WaxRcError, load_waxrc};
use global_state::{GlobalStateError, InstalledLanguagePack, load_global_state, save_global_state};
use install::{InstallError, LanguagePackManifestSpec, install_language};
use paths::{PathsError, state_file};
use progress::{ScanProgress, ScanProgressEvent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
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

const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_SCAN_OUTPUT_TEMP_ATTEMPTS: u32 = 1000;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ptr;

#[cfg(windows)]
const ERROR_UNABLE_TO_REMOVE_REPLACED: i32 = 1175;
#[cfg(windows)]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT: i32 = 1176;
#[cfg(windows)]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

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
    /// Overrides `.wax/wax.config.json` `engine.scan_concurrency` when set.
    ///
    /// Values less than 1 are treated as serial execution.
    pub scan_concurrency: Option<u32>,
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
            scan_concurrency: None,
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
    /// Scan facts failed contract validation during merge.
    #[error(transparent)]
    ScanFacts(#[from] wax_contract::ScanFactsError),
    /// A language scan worker thread panicked.
    #[error("language scan worker panicked")]
    ScanWorkerPanicked,
}

impl Engine {
    /// Scans a repository by running enabled language packs.
    ///
    /// On success, scan output is persisted under `.wax/out/` in the scanned
    /// repository. The merged scan is written to `.wax/out/scan-merged.json`,
    /// and per-language facts are written to `.wax/out/languages/`.
    pub fn scan_repo(repo_root: impl AsRef<Path>) -> Result<MergedScan, EngineError> {
        Self::scan_repo_with_options(repo_root, ScanOptions::default())
    }

    /// Scans a repository by running enabled language packs with runtime options.
    ///
    /// On success, scan output is persisted under `.wax/out/` in the scanned
    /// repository. The merged scan is written to `.wax/out/scan-merged.json`,
    /// and per-language facts are written to `.wax/out/languages/`. Output
    /// write failures are returned as [`EngineError::ScanOutput`].
    pub fn scan_repo_with_options(
        repo_root: impl AsRef<Path>,
        mut options: ScanOptions,
    ) -> Result<MergedScan, EngineError> {
        let repo_root = repo_root.as_ref();
        let progress = options.progress.clone();
        progress.emit(ScanProgressEvent::Preparing);
        let (waxrc, lockfile) = if let Some(ephemeral) = options.ephemeral.take() {
            (ephemeral.waxrc, ephemeral.lockfile)
        } else {
            let repo_files = config::repo_files::discover_repo_files(repo_root);
            (
                load_waxrc(&repo_files.config_path)?,
                load_lockfile(&repo_files.lockfile_path)?,
            )
        };
        let scan_concurrency = effective_scan_concurrency(&waxrc.engine, &options);
        let parent_scope_limit = waxrc.adoption.symbol_usage_summary.parent_scope_limit;
        let state_path = state_file()?;
        let mut state = load_global_state(&state_path)?;

        let mut enabled_ids = BTreeSet::new();
        let mut language_configs = BTreeMap::new();
        for entry in waxrc.languages {
            let resolved_registry =
                registry_source::resolve_registry_source(registry_source::RegistrySourceInput {
                    repo_root,
                    language_id: entry.id.as_str(),
                    source: entry
                        .registry_source
                        .as_ref()
                        .map(|setting| setting.source.as_str()),
                })?;
            registry_lock::verify_registry_lock(&entry.id, &resolved_registry, &lockfile)
                .map_err(registry_lock_mismatch_to_engine_error)?;

            let mut config = entry.extra;
            if !entry.roots.is_empty() {
                config.insert(
                    "roots".to_owned(),
                    serde_json::Value::Array(
                        entry
                            .roots
                            .into_iter()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
            config.insert(
                "registry".to_owned(),
                serde_json::Value::String(resolved_registry.repo_relative_path),
            );
            language_configs.insert(entry.id.clone(), config);
            enabled_ids.insert(entry.id);
        }

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
            if !options.allow_auto_install {
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
                    &lockfile,
                    &pack_index_artifacts,
                    &state_path,
                    &progress,
                )?;
                state = load_global_state(&state_path)?;
            }
        }

        let mut jobs = Vec::new();
        for language_id in enabled_ids {
            let locked = &lockfile.languages[&language_id];
            let pack_spec =
                load_installed_manifest_for_locked(&state, &language_id, &locked.version)?;
            let config = language_configs.remove(&language_id).unwrap_or_default();
            jobs.push(ScanJob {
                language_id,
                command: pack_spec.command,
                config,
            });
        }
        let languages = run_scan_jobs(repo_root, jobs, scan_concurrency, &progress)?;
        let merged = merge_language_scans_with_parent_scope_limit(languages, parent_scope_limit)
            .map_err(EngineError::ScanFacts)?;
        progress.emit(ScanProgressEvent::WritingOutputs);
        write_scan_outputs(repo_root, &merged)?;

        Ok(merged)
    }
}

fn effective_scan_concurrency(
    engine_config: &config::waxrc::EngineConfig,
    options: &ScanOptions,
) -> usize {
    let configured = options
        .scan_concurrency
        .unwrap_or(engine_config.scan_concurrency);
    configured.max(1) as usize
}

fn registry_lock_mismatch_to_engine_error(
    mismatch: registry_lock::RegistryLockMismatch,
) -> EngineError {
    match mismatch {
        registry_lock::RegistryLockMismatch::Missing { language_id } => EngineError::RegistryLock {
            language_id,
            reason: "missing registry lock entry".to_owned(),
        },
        registry_lock::RegistryLockMismatch::SourceDrift {
            language_id,
            lockfile_source,
            resolved_source,
        } => EngineError::RegistryLock {
            language_id,
            reason: format!("source changed from {lockfile_source} to {resolved_source}"),
        },
        registry_lock::RegistryLockMismatch::DigestDrift {
            language_id,
            lockfile_sha256,
            resolved_sha256,
        } => EngineError::RegistryLock {
            language_id,
            reason: format!("digest changed from {lockfile_sha256} to {resolved_sha256}"),
        },
    }
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
                    let job = queue.lock().expect("scan job queue poisoned").pop_front();
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
        timeout: DEFAULT_SCAN_TIMEOUT,
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
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_output_dir(parent)?;
    let (tmp_path, mut file) = create_temp_output_file(path)?;

    let result = (|| {
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        replace_output_file(&tmp_path, path)?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })();

    match result {
        Ok(()) => Ok(()),
        Err(source) => {
            remove_failed_scan_output_temp_file(&tmp_path, source.as_ref());
            Err(EngineError::ScanOutput {
                path: path.display().to_string(),
                source,
            })
        }
    }
}

#[cfg(not(windows))]
fn remove_failed_scan_output_temp_file(
    tmp_path: &Path,
    _source: &(dyn std::error::Error + Send + Sync + 'static),
) {
    let _ = fs::remove_file(tmp_path);
}

#[cfg(windows)]
fn remove_failed_scan_output_temp_file(
    tmp_path: &Path,
    source: &(dyn std::error::Error + Send + Sync + 'static),
) {
    if let Some(source) = source.downcast_ref::<io::Error>()
        && is_documented_windows_partial_scan_output_replace_failure(source)
    {
        return;
    }

    let _ = fs::remove_file(tmp_path);
}

fn create_temp_output_file(path: &Path) -> Result<(PathBuf, File), EngineError> {
    let temp_stem = new_snapshot_id();
    for attempt in 0..MAX_SCAN_OUTPUT_TEMP_ATTEMPTS {
        let tmp_path = temp_output_path(path, &temp_stem, attempt);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => return Ok((tmp_path, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(EngineError::ScanOutput {
                    path: path.display().to_string(),
                    source: Box::new(source),
                });
            }
        }
    }

    Err(EngineError::ScanOutput {
        path: path.display().to_string(),
        source: Box::new(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate unique temporary scan output path",
        )),
    })
}

fn temp_output_path(path: &Path, temp_stem: &str, attempt: u32) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("scan-output");
    parent.join(format!(
        ".{file_name}.{}.{temp_stem}.{attempt}.tmp",
        std::process::id(),
    ))
}

#[cfg(not(windows))]
fn replace_output_file(tmp_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(tmp_path, path)
}

#[cfg(windows)]
fn replace_output_file(tmp_path: &Path, path: &Path) -> io::Result<()> {
    if !path.exists() {
        return fs::rename(tmp_path, path);
    }

    replace_existing_output_file(tmp_path, path)
}

#[cfg(windows)]
fn replace_existing_output_file(tmp_path: &Path, path: &Path) -> io::Result<()> {
    let replaced = wide_null(path.as_os_str());
    let replacement = wide_null(tmp_path.as_os_str());

    // SAFETY: both path buffers are null-terminated and live for the duration
    // of the call; null backup/exclude/reserved pointers match ReplaceFileW.
    let replaced = unsafe {
        replace_file_w(
            replaced.as_ptr(),
            replacement.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };

    if replaced == 0 {
        let source = io::Error::last_os_error();
        if recover_windows_partial_scan_output_replace_failure(&source, tmp_path, path)
            .unwrap_or(false)
        {
            return Ok(());
        }
        return Err(source);
    }

    Ok(())
}

#[cfg(windows)]
fn recover_windows_partial_scan_output_replace_failure(
    source: &io::Error,
    tmp_path: &Path,
    path: &Path,
) -> io::Result<bool> {
    if source.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT)
        && !path.exists()
        && tmp_path.exists()
    {
        fs::rename(tmp_path, path)?;
        return Ok(true);
    }

    Ok(false)
}

#[cfg(windows)]
fn is_documented_windows_partial_scan_output_replace_failure(source: &io::Error) -> bool {
    matches!(
        source.raw_os_error(),
        Some(
            ERROR_UNABLE_TO_REMOVE_REPLACED
                | ERROR_UNABLE_TO_MOVE_REPLACEMENT
                | ERROR_UNABLE_TO_MOVE_REPLACEMENT_2
        )
    )
}

#[cfg(windows)]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "ReplaceFileW"]
    fn replace_file_w(
        replaced_file_name: *const u16,
        replacement_file_name: *const u16,
        backup_file_name: *const u16,
        replace_flags: u32,
        exclude: *mut std::ffi::c_void,
        reserved: *mut std::ffi::c_void,
    ) -> i32;
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

fn execute_install_plans(
    plans: &[auto_install::InstallPlan],
    lockfile: &config::lockfile::WaxLock,
    pack_index_artifacts: &BTreeMap<LanguageId, BTreeMap<String, Vec<PackIndexArtifact>>>,
    state_path: &Path,
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

        let manifest = LanguagePackManifestSpec {
            id: plan.language_id.clone(),
            version: plan.version.clone(),
            api_version: locked.api_version,
            command: vec![
                format!("./wax-lang-{}", plan.language_id),
                "--stdio".to_owned(),
            ],
            ecosystem: plan.language_id.to_string(),
            parser_name: plan.language_id.to_string(),
            parser_version: plan.version.clone(),
        };

        let install_dir = install_language(
            &plan.language_id,
            &plan.version,
            &locked.resolved.target,
            &locked.resolved.url,
            &plan.sha256,
            Some(&artifact.sha256),
            &manifest,
        )?;

        record_installed_language_or_cleanup(
            state_path,
            &plan.language_id,
            &plan.version,
            install_dir,
        )?;
    }
    Ok(())
}

fn record_installed_language_or_cleanup(
    state_path: &Path,
    language_id: &LanguageId,
    version: &str,
    install_dir: PathBuf,
) -> Result<(), EngineError> {
    let record_result = (|| {
        let mut state = load_global_state(state_path)?;
        state
            .installed_languages
            .entry(language_id.clone())
            .or_default()
            .insert(
                version.to_owned(),
                InstalledLanguagePack {
                    install_dir: install_dir.clone(),
                },
            );
        save_global_state(state_path, &state)?;
        Ok(())
    })();

    if let Err(err) = record_result {
        remove_installed_dir(&install_dir)?;
        return Err(err);
    }

    Ok(())
}

fn remove_installed_dir(path: &Path) -> Result<(), EngineError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(EngineError::InstallCleanup {
            path: path.display().to_string(),
            source,
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_installed_language_cleans_up_when_state_load_fails() {
        let root = std::env::temp_dir().join(format!(
            "wax-core-record-cleanup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let install_dir = root.join("langs/compose/0.1.0");
        fs::create_dir_all(&install_dir).unwrap();
        let state_path = root.join("state.json");
        fs::write(&state_path, "{not valid json").unwrap();

        let language_id = LanguageId::try_from("compose").unwrap();
        let err = record_installed_language_or_cleanup(
            &state_path,
            &language_id,
            "0.1.0",
            install_dir.clone(),
        )
        .expect_err("malformed state should fail");

        assert!(matches!(err, EngineError::GlobalState(_)));
        assert!(!install_dir.exists());

        let _ = fs::remove_dir_all(root);
    }
}
