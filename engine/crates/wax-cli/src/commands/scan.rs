//! `wax scan` command implementation.

use super::diagnostic_output::format_diagnostic_line;
use super::language::{
    LanguageCommandError, default_target_triple, manifest_for_language, resolve_registry_url,
    update_lockfile_entry,
};
use super::state_path::resolve_state_path;
use crate::progress::{CliProgress, optional_scan_progress_sink};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use wax_contract::{Diagnostic, DiagnosticSeverity, LanguageId, MergedScan, ScanStatus};
use wax_core::config::lockfile::{LockedRegistry, WAX_LOCK_SCHEMA_VERSION, WaxLock};
use wax_core::config::repo_files::PREFERRED_CONFIG_RELATIVE_PATH;
use wax_core::config::waxrc::{
    AdoptionConfig, EngineConfig, LanguageEntry, LanguageRegistrySource, WAXRC_SCHEMA_VERSION,
    WaxRc, WaxRcError, load_waxrc,
};
use wax_core::paths::PathsError;
use wax_core::registry::{fetch_pack_index, select_target_artifact};
use wax_core::registry_memory::{
    RememberedDesignSystemSummary, list_remembered_design_systems, resolve_remembered_registry,
    show_remembered_design_system,
};
use wax_core::registry_source::{RegistrySourceInput, resolve_registry_source};
use wax_core::sync::{SyncError, SyncOptions, best_effort_sync_app_registries};
use wax_core::{Engine, EngineError, EphemeralScanConfig, ScanOptions};
use wax_lang_api::build_version;

const MAX_FAILURE_DIAGNOSTICS: usize = 5;
const SCAN_OUTPUT_RELATIVE_PATH: &str = ".wax/out/scan-merged.json";
const ENGINE_API_VERSION: u32 = 1;

/// Ephemeral scan selections used when no repo config exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EphemeralScanSelections {
    /// Language pack ids selected for this scan.
    pub languages: Vec<LanguageId>,
    /// Scan roots keyed by language id.
    pub scan_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    /// Remembered design-system id supplying registry inputs.
    pub design_system_id: String,
}

/// Options for `wax scan`.
#[derive(Debug, Clone)]
pub struct ScanCommandOptions {
    /// Repository root containing `.wax/wax.config.json` and `.wax/wax.lock.json`.
    pub repo_root: PathBuf,
    /// Whether missing packs may be auto-installed.
    pub allow_auto_install: bool,
    /// Optional scan concurrency override.
    pub scan_concurrency: Option<u32>,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
    /// Pack index URL override for ephemeral scans.
    pub pack_index_url: Option<String>,
    /// Target triple override for ephemeral scans.
    pub target_triple: Option<String>,
    /// Ephemeral selections for tests and scripted flows.
    pub ephemeral: Option<EphemeralScanSelections>,
}

/// Errors returned by `wax scan`.
#[derive(Debug, Error)]
pub enum ScanCommandError {
    /// Engine scan failed.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Language lifecycle command failed during ephemeral scan setup.
    #[error(transparent)]
    Language(#[from] LanguageCommandError),
    /// Pack index fetch or resolution failed.
    #[error(transparent)]
    Registry(#[from] wax_core::registry::RegistryError),
    /// Remembered design-system memory failed.
    #[error(transparent)]
    RegistryMemory(#[from] wax_core::registry_memory::RegistryMemoryError),
    /// Legacy sync wrapper retained for API compatibility.
    ///
    /// Current scan entry points report best-effort sync failures as warnings.
    #[error(transparent)]
    Sync(#[from] SyncError),
    /// Wax config could not be loaded before scan sync.
    #[error(transparent)]
    Config(#[from] WaxRcError),
    /// Global wax paths could not be resolved.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Repository config or usable ephemeral selections are required.
    #[error(
        "wax scan requires repository config at {config_path}; run `wax init` for CI or scripts"
    )]
    RequiresInit {
        /// Missing config path.
        config_path: PathBuf,
    },
    /// Legacy interactive-terminal error retained for API compatibility.
    #[error("wax scan needs an interactive terminal when no wax config exists")]
    RequiresInteractiveTerminal,
    /// Summary writing failed.
    #[error("failed to write scan summary: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax scan`, prompting for ephemeral selections when config is missing in a TTY.
///
/// # Errors
///
/// Returns [`ScanCommandError::RequiresInit`] when config and usable ephemeral
/// selections are unavailable; [`ScanCommandError::Paths`] when ephemeral setup
/// or pre-scan upstream sync needs an implicit global state path that cannot be
/// resolved;
/// `ScanCommandError::Engine(EngineError::RegistrySource(..))` when the
/// no-config ephemeral flow resolves a remembered design-system registry source
/// that cannot be read, fetched, validated, or materialized locally
/// (`RegistrySourceError::UnsupportedScheme`, `PlainAbsolutePath`,
/// `PathEscapesRepo`, `InvalidFileUrl`, `Read`, `Fetch`, `HttpStatus`,
/// `MalformedJson`, `InvalidShape`, or `CacheWrite`);
/// [`ScanCommandError::Config`] when configured pre-scan sync cannot load wax
/// config; [`ScanCommandError::Language`] or [`ScanCommandError::Registry`] when
/// ephemeral pack metadata cannot be resolved;
/// [`ScanCommandError::RegistryMemory`] when remembered registry state cannot be
/// listed or resolved; [`ScanCommandError::Engine`] when the scan fails; or
/// [`ScanCommandError::Io`] when prompts, sync warnings, or summary output cannot
/// be written.
pub fn run_scan_cli(
    options: ScanCommandOptions,
    writer: &mut impl Write,
) -> Result<(), ScanCommandError> {
    let config_path = options.repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    if config_path.is_file() {
        return run_scan(options, writer, false);
    }

    if let Some(selections) = options.ephemeral.clone() {
        return run_ephemeral_scan(options, selections, writer);
    }

    if !io::stdin().is_terminal() {
        return Err(ScanCommandError::RequiresInit { config_path });
    }

    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let registry_url = resolve_registry_url(options.pack_index_url.clone());
    let manifests = fetch_pack_index(&registry_url).map_err(LanguageCommandError::from)?;
    let mut prompts = DialoguerScanPrompts;
    let selections = collect_ephemeral_scan_selections(&manifests, &mut prompts, &state_path)?;
    run_ephemeral_scan(options, selections, writer)
}

/// Runs `wax scan` against committed repository config.
///
/// # Errors
///
/// Returns [`ScanCommandError::Config`] when pre-scan sync cannot load the wax
/// config. If that config has a non-empty registry upstream and no state-path
/// override, returns [`ScanCommandError::Paths`] when the global state path
/// cannot be resolved. Returns [`ScanCommandError::Engine`] when scanning fails,
/// or [`ScanCommandError::Io`] when a sync warning or scan summary cannot be
/// written.
pub fn run_scan(
    options: ScanCommandOptions,
    writer: &mut impl Write,
    ephemeral: bool,
) -> Result<(), ScanCommandError> {
    if !ephemeral {
        attempt_scan_time_registry_sync(&options, writer)?;
    }

    let progress = Arc::new(CliProgress::new());
    let merged = Engine::scan_repo_with_options(
        &options.repo_root,
        ScanOptions {
            scan_concurrency: options.scan_concurrency,
            allow_auto_install: options.allow_auto_install,
            progress: optional_scan_progress_sink(&progress),
            ephemeral: None,
        },
    )?;
    progress.finish();

    let output_path = options.repo_root.join(SCAN_OUTPUT_RELATIVE_PATH);
    write_scan_summary(writer, &merged, &output_path, ephemeral)
}

fn attempt_scan_time_registry_sync(
    options: &ScanCommandOptions,
    writer: &mut impl Write,
) -> Result<(), ScanCommandError> {
    let config_path = options.repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    if !config_path.is_file() {
        return Ok(());
    }

    let waxrc = load_waxrc(&config_path)?;
    let has_upstream = waxrc.languages.iter().any(|entry| {
        entry
            .registry_source
            .as_ref()
            .and_then(|registry| registry.upstream.as_ref())
            .is_some_and(|upstream| !upstream.trim().is_empty())
    });
    if !has_upstream {
        return Ok(());
    }

    let state_path = match resolve_state_path(options.state_path.as_deref()) {
        Ok(path) => path,
        Err(_error) => {
            write_scan_sync_warning(writer)?;
            return Ok(());
        }
    };
    match best_effort_sync_app_registries(&SyncOptions {
        repo_root: options.repo_root.clone(),
        state_path,
    }) {
        Ok((_updates, failures)) => {
            for (upstream, _error) in failures {
                writeln!(
                    writer,
                    "warning: registry sync failed for {upstream}; scanning with current registry source. Run `wax sync` for details."
                )
                .map_err(|source| ScanCommandError::Io { source })?;
            }
        }
        Err(error) => {
            write_scan_sync_warning(writer)?;
            let _ = error;
        }
    }
    Ok(())
}

fn run_ephemeral_scan(
    options: ScanCommandOptions,
    selections: EphemeralScanSelections,
    writer: &mut impl Write,
) -> Result<(), ScanCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let ephemeral = build_ephemeral_scan_config(&options, &selections, &state_path)?;
    let progress = Arc::new(CliProgress::new());
    let merged = Engine::scan_repo_with_options(
        &options.repo_root,
        ScanOptions {
            scan_concurrency: options.scan_concurrency,
            allow_auto_install: options.allow_auto_install,
            progress: optional_scan_progress_sink(&progress),
            ephemeral: Some(ephemeral),
        },
    )?;
    progress.finish();

    let output_path = options.repo_root.join(SCAN_OUTPUT_RELATIVE_PATH);
    write_scan_summary(writer, &merged, &output_path, true)
}

fn build_ephemeral_scan_config(
    options: &ScanCommandOptions,
    selections: &EphemeralScanSelections,
    state_path: &Path,
) -> Result<EphemeralScanConfig, ScanCommandError> {
    let remembered = show_remembered_design_system(state_path, &selections.design_system_id)?;
    let registry_url = resolve_registry_url(options.pack_index_url.clone());
    let manifests = fetch_pack_index(&registry_url).map_err(LanguageCommandError::from)?;
    let target = options
        .target_triple
        .clone()
        .unwrap_or_else(default_target_triple);

    let mut languages = Vec::new();
    let mut lockfile = WaxLock {
        schema_version: WAX_LOCK_SCHEMA_VERSION,
        engine_api_version: ENGINE_API_VERSION,
        wax_version: build_version().to_owned(),
        locked_at: None,
        registries: BTreeMap::new(),
        languages: BTreeMap::new(),
    };

    for language_id in &selections.languages {
        let roots = selections
            .scan_roots
            .get(language_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect();
        let resolved = resolve_remembered_registry(&remembered, language_id)?;
        let scan_source = if let Some(local_source) = resolved.design_system_local_source.as_deref()
        {
            format!(
                "file://{}",
                remembered.repo_root.join(local_source).display()
            )
        } else {
            resolved.config_source.clone()
        };
        let registry_source = resolve_registry_source(RegistrySourceInput {
            repo_root: &options.repo_root,
            language_id: language_id.as_str(),
            source: Some(&scan_source),
        })
        .map_err(EngineError::from)?;

        languages.push(LanguageEntry {
            id: language_id.clone(),
            roots,
            registry_source: Some(LanguageRegistrySource {
                source: scan_source,
                upstream: None,
            }),
            extra: serde_json::Map::new(),
        });
        lockfile.registries.insert(
            language_id.clone(),
            LockedRegistry {
                source: registry_source.source,
                sha256: registry_source.sha256,
            },
        );

        let manifest = manifest_for_language(&manifests, language_id, None)?;
        let artifact = select_target_artifact(&manifest, &target)?.clone();
        update_lockfile_entry(&mut lockfile, &manifest, &registry_url, &target, &artifact);
    }

    Ok(EphemeralScanConfig {
        waxrc: WaxRc {
            schema_version: WAXRC_SCHEMA_VERSION,
            engine: EngineConfig::default(),
            adoption: AdoptionConfig::default(),
            languages,
            design_systems: BTreeMap::new(),
        },
        lockfile,
    })
}

trait ScanPrompts {
    fn select_languages(
        &mut self,
        manifests: &[wax_core::registry::RegistryManifest],
    ) -> Result<Vec<LanguageId>, ScanCommandError>;

    fn scan_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, ScanCommandError>;

    fn select_remembered_design_system(
        &mut self,
        remembered: &[RememberedDesignSystemSummary],
    ) -> Result<String, ScanCommandError>;
}

struct DialoguerScanPrompts;

impl ScanPrompts for DialoguerScanPrompts {
    fn select_languages(
        &mut self,
        manifests: &[wax_core::registry::RegistryManifest],
    ) -> Result<Vec<LanguageId>, ScanCommandError> {
        use dialoguer::MultiSelect;

        let mut sorted = manifests.to_vec();
        sorted.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        let labels: Vec<String> = sorted
            .iter()
            .map(|manifest| format!("{} ({})", manifest.id.as_str(), manifest.version))
            .collect();
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let selected = MultiSelect::new()
            .with_prompt("Select language packs to scan")
            .items(&label_refs)
            .interact()
            .map_err(|source| ScanCommandError::Io {
                source: io::Error::other(source),
            })?;
        Ok(selected
            .into_iter()
            .map(|index| sorted[index].id.clone())
            .collect())
    }

    fn scan_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, ScanCommandError> {
        use dialoguer::Input;

        let input: String = Input::new()
            .with_prompt(format!(
                "Scan roots for {} (comma-separated)",
                language_id.as_str()
            ))
            .interact_text()
            .map_err(|source| ScanCommandError::Io {
                source: io::Error::other(source),
            })?;
        Ok(parse_roots(&input))
    }

    fn select_remembered_design_system(
        &mut self,
        remembered: &[RememberedDesignSystemSummary],
    ) -> Result<String, ScanCommandError> {
        use dialoguer::Select;

        if remembered.is_empty() {
            return Err(ScanCommandError::RequiresInit {
                config_path: PathBuf::from(PREFERRED_CONFIG_RELATIVE_PATH),
            });
        }

        let labels: Vec<String> = remembered
            .iter()
            .map(|entry| format!("{} ({})", entry.name, entry.id))
            .collect();
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let selected = Select::new()
            .with_prompt("Select a remembered design-system registry")
            .items(&label_refs)
            .default(0)
            .interact()
            .map_err(|source| ScanCommandError::Io {
                source: io::Error::other(source),
            })?;
        Ok(remembered[selected].id.clone())
    }
}

fn collect_ephemeral_scan_selections(
    manifests: &[wax_core::registry::RegistryManifest],
    prompts: &mut impl ScanPrompts,
    state_path: &Path,
) -> Result<EphemeralScanSelections, ScanCommandError> {
    let languages = prompts.select_languages(manifests)?;
    if languages.is_empty() {
        return Err(ScanCommandError::RequiresInit {
            config_path: PathBuf::from(PREFERRED_CONFIG_RELATIVE_PATH),
        });
    }

    let mut scan_roots = BTreeMap::new();
    for language_id in &languages {
        scan_roots.insert(language_id.clone(), prompts.scan_roots(language_id)?);
    }

    let remembered = list_remembered_design_systems(state_path)?;
    let design_system_id = prompts.select_remembered_design_system(&remembered)?;

    Ok(EphemeralScanSelections {
        languages,
        scan_roots,
        design_system_id,
    })
}

fn parse_roots(input: &str) -> Vec<PathBuf> {
    input
        .split(',')
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn write_error(source: io::Error) -> ScanCommandError {
    ScanCommandError::Io { source }
}

fn write_scan_sync_warning(writer: &mut impl Write) -> Result<(), ScanCommandError> {
    writeln!(
        writer,
        "warning: registry sync failed; scanning with current registry source. Run `wax sync` for details."
    )
    .map_err(write_error)
}

fn write_scan_summary(
    writer: &mut impl Write,
    merged: &MergedScan,
    output_path: &Path,
    ephemeral: bool,
) -> Result<(), ScanCommandError> {
    writeln!(writer, "scan output: {}", output_path.display()).map_err(write_error)?;
    writeln!(writer, "language status:").map_err(write_error)?;
    for (language_id, facts) in &merged.languages {
        write!(writer, "  {language_id}: {}", status_label(facts.status)).map_err(write_error)?;
        if let Some(ratio) = facts.metrics.invocation_adoption_ratio {
            write!(writer, " (UI invocation adoption: {:.1}%)", ratio * 100.0)
                .map_err(write_error)?;
        }
        writeln!(writer).map_err(write_error)?;
    }

    writeln!(writer, "adoption metrics:").map_err(write_error)?;
    let repo = &merged.repo_summary;
    if let Some(ratio) = repo.metrics.invocation_adoption_ratio {
        writeln!(writer, "  UI invocation adoption: {:.1}%", ratio * 100.0).map_err(write_error)?;
    } else {
        writeln!(writer, "  UI invocation adoption: n/a").map_err(write_error)?;
    }
    if let Some(ratio) = repo.metrics.registry_resolution_ratio {
        writeln!(writer, "  Registry resolution: {:.1}%", ratio * 100.0).map_err(write_error)?;
    } else {
        writeln!(writer, "  Registry resolution: n/a").map_err(write_error)?;
    }
    let raw = &repo.counts.raw_invocations;
    writeln!(
        writer,
        "  Raw DS invocations: {} resolved, {} candidate",
        raw.resolved, raw.candidate
    )
    .map_err(write_error)?;
    writeln!(writer, "  Local invocations: {}", raw.local).map_err(write_error)?;
    writeln!(
        writer,
        "  Local definitions: {} defined, {} invoked",
        repo.counts.definitions.local_definition_count,
        repo.counts.definitions.invoked_local_definition_count
    )
    .map_err(write_error)?;
    writeln!(writer, "  Unresolved UI calls: {}", raw.unresolved).map_err(write_error)?;

    writeln!(writer, "token metrics:").map_err(write_error)?;
    if let Some(ratio) = repo.metrics.token_reference_ratio {
        writeln!(writer, "  Token reference ratio: {:.1}%", ratio * 100.0).map_err(write_error)?;
    } else {
        writeln!(writer, "  Token reference ratio: n/a").map_err(write_error)?;
    }
    writeln!(
        writer,
        "  Token references: {}",
        repo.counts.tokens.token_reference_site_count
    )
    .map_err(write_error)?;
    writeln!(
        writer,
        "  Hard-coded style candidates: {}",
        repo.counts.tokens.hardcoded_style_candidate_count
    )
    .map_err(write_error)?;

    let diagnostics = merged
        .languages
        .values()
        .flat_map(|facts| facts.diagnostics.iter())
        .filter(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error || diagnostic.code == "parse_failed"
        })
        .take(MAX_FAILURE_DIAGNOSTICS)
        .collect::<Vec<_>>();
    write_failure_diagnostics(writer, &diagnostics)?;

    if ephemeral {
        writeln!(writer).map_err(write_error)?;
        writeln!(
            writer,
            "To save this setup for CI or teammates, run `wax init`."
        )
        .map_err(write_error)?;
    }

    Ok(())
}

fn write_failure_diagnostics(
    writer: &mut impl Write,
    diagnostics: &[&Diagnostic],
) -> Result<(), ScanCommandError> {
    if diagnostics.is_empty() {
        writeln!(writer, "failure diagnostics: none").map_err(write_error)?;
    } else {
        writeln!(
            writer,
            "failure diagnostics (up to {MAX_FAILURE_DIAGNOSTICS}):"
        )
        .map_err(write_error)?;
        for diagnostic in diagnostics {
            writeln!(writer, "  {}", format_diagnostic_line(diagnostic)).map_err(write_error)?;
        }
    }
    Ok(())
}

fn status_label(status: ScanStatus) -> &'static str {
    match status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "partial",
        ScanStatus::Failed => "failed",
    }
}

/// Returns whether a path existed before scan under `.wax`.
pub fn repo_relative_path_exists(repo_root: &Path, relative: &str) -> bool {
    repo_root.join(relative).exists()
}

/// Returns whether any file exists under a repo-relative directory.
pub fn repo_relative_dir_has_entries(repo_root: &Path, relative: &str) -> bool {
    let path = repo_root.join(relative);
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        EphemeralScanSelections, ScanCommandError, ScanCommandOptions,
        attempt_scan_time_registry_sync, run_scan_cli, write_scan_summary,
    };
    use crate::testing::env_lock;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::time::{SystemTime, UNIX_EPOCH};
    use time::OffsetDateTime;
    use wax_contract::{
        AdoptionCounts, CountSummary, DefinitionCounts, Diagnostic, DiagnosticSeverity, LanguageId,
        LanguageMetadata, MergedScan, Metrics, ParentScopeCounts, RawInvocationCounts,
        RegistryCounts, RepoSummary, SCHEMA_VERSION, ScanFacts, ScanStatus, SourceLocation,
    };
    use wax_core::paths::PathsError;

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        #[expect(
            unsafe_code,
            reason = "these tests hold env_lock while mutating process environment variables, which keeps env access serialized inside this test binary"
        )]
        fn remove(name: &'static str) -> Self {
            let previous = std::env::var_os(name);
            unsafe {
                std::env::remove_var(name);
            }
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        #[expect(
            unsafe_code,
            reason = "these tests hold env_lock while restoring process environment variables, which keeps env access serialized inside this test binary"
        )]
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.name, value),
                    None => std::env::remove_var(self.name),
                }
            }
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("wax-cli-scan-{name}-{nonce}"));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_committed_scan_repo_with_upstream(app_repo: &Path) {
        fs::create_dir_all(app_repo.join(".wax/registries/acme")).expect("create registries dir");
        fs::write(
            app_repo.join(".wax/registries/acme/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
        )
        .expect("write app registry");
        fs::write(
            app_repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/acme/react.json",
        "upstream": "acme/react"
      }
    }
  }
}
"#,
        )
        .expect("write app config");
        fs::write(
            app_repo.join(".wax/wax.lock.json"),
            r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {},
  "languages": {}
}
"#,
        )
        .expect("write app lockfile");
    }

    #[test]
    fn summary_renders_status_adoption_and_failure_diagnostics() {
        let mut output = Vec::new();
        let merged = MergedScan {
            schema_version: SCHEMA_VERSION,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            repo_summary: RepoSummary {
                languages: vec![
                    LanguageId::from_str("compose").unwrap(),
                    LanguageId::from_str("react").unwrap(),
                    LanguageId::from_str("swift").unwrap(),
                ],
                counts: sample_repo_counts(),
                metrics: Metrics {
                    invocation_adoption_ratio: Some(0.875),
                    registry_resolution_ratio: Some(0.7),
                    token_reference_ratio: Some(0.75),
                    parse_extract_ms: 2,
                    files_scanned: 2,
                },
            },
            symbol_usage_summary: vec![],
            token_usage_summary: vec![],
            languages: BTreeMap::from([
                (
                    LanguageId::from_str("compose").unwrap(),
                    facts_with_status(ScanStatus::Complete, Some(0.875), vec![]),
                ),
                (
                    LanguageId::from_str("react").unwrap(),
                    facts_with_status(
                        ScanStatus::Partial,
                        None,
                        vec![
                            diagnostic(DiagnosticSeverity::Error, "PACK_TIMEOUT", "timed out"),
                            diagnostic(DiagnosticSeverity::Warning, "PACK_WARN", "warn"),
                            Diagnostic {
                                severity: DiagnosticSeverity::Error,
                                code: "parse_failed".to_owned(),
                                message: "failed to parse source file; file skipped".to_owned(),
                                location: Some(SourceLocation {
                                    file: "src/Broken.tsx".to_owned(),
                                    line: 4,
                                    column: Some(12),
                                }),
                            },
                        ],
                    ),
                ),
                (
                    LanguageId::from_str("swift").unwrap(),
                    facts_with_status(
                        ScanStatus::Failed,
                        None,
                        vec![diagnostic(
                            DiagnosticSeverity::Error,
                            "PACK_CRASH",
                            "process exited",
                        )],
                    ),
                ),
            ]),
        };

        write_scan_summary(
            &mut output,
            &merged,
            std::path::Path::new("/tmp/repo/.wax/out/scan-merged.json"),
            false,
        )
        .unwrap();

        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains("scan output: /tmp/repo/.wax/out/scan-merged.json"));
        assert!(stdout.contains("compose: complete (UI invocation adoption: 87.5%)"));
        assert!(stdout.contains("react: partial"));
        assert!(stdout.contains("swift: failed"));
        assert!(stdout.contains("UI invocation adoption: 87.5%"));
        assert!(stdout.contains("Registry resolution: 70.0%"));
        assert!(stdout.contains("Raw DS invocations: 7 resolved, 1 candidate"));
        assert!(stdout.contains("Unresolved UI calls: 1"));
        assert!(stdout.contains("token metrics:"));
        assert!(stdout.contains("Token reference ratio: 75.0%"));
        assert!(stdout.contains("Token references: 3"));
        assert!(stdout.contains("Hard-coded style candidates: 1"));
        assert!(stdout.contains("PACK_TIMEOUT: timed out"));
        assert!(stdout.contains(
            "parse_failed (src/Broken.tsx:4:12): failed to parse source file; file skipped"
        ));
        assert!(stdout.contains("PACK_CRASH: process exited"));
        assert!(!stdout.contains("PACK_WARN: warn"));
    }

    #[test]
    fn scan_command_warns_when_registry_sync_fails() {
        let root = TestDir::new("sync-warning");
        let app_repo = root.path.join("app");
        write_committed_scan_repo_with_upstream(&app_repo);

        let wax_home = root.path.join("wax-home");
        fs::create_dir_all(&wax_home).expect("create wax home");
        fs::write(
            wax_home.join("state.json"),
            r#"{"installed_languages":{},"design_systems":{}}"#,
        )
        .expect("write empty state");

        let mut output = Vec::new();
        attempt_scan_time_registry_sync(
            &ScanCommandOptions {
                repo_root: app_repo,
                allow_auto_install: false,
                scan_concurrency: None,
                state_path: Some(wax_home.join("state.json")),
                pack_index_url: None,
                target_triple: None,
                ephemeral: None,
            },
            &mut output,
        )
        .expect("scan-time sync warning should not fail scan");

        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains(
            "warning: registry sync failed for acme/react; scanning with current registry source. Run `wax sync` for details."
        ));
    }

    #[test]
    fn ephemeral_scan_without_home_returns_paths_error() {
        let _guard = env_lock();
        let _home = EnvVarGuard::remove("HOME");
        let _wax_home = EnvVarGuard::remove("WAX_HOME");
        let _user_profile = EnvVarGuard::remove("USERPROFILE");
        let _home_drive = EnvVarGuard::remove("HOMEDRIVE");
        let _home_path = EnvVarGuard::remove("HOMEPATH");
        let root = TestDir::new("ephemeral-no-home");
        let mut output = Vec::new();

        let err = run_scan_cli(
            ScanCommandOptions {
                repo_root: root.path.clone(),
                allow_auto_install: false,
                scan_concurrency: None,
                state_path: None,
                pack_index_url: None,
                target_triple: None,
                ephemeral: Some(EphemeralScanSelections {
                    languages: vec![LanguageId::from_str("react").expect("valid language id")],
                    scan_roots: BTreeMap::from([(
                        LanguageId::from_str("react").expect("valid language id"),
                        vec![PathBuf::from("src")],
                    )]),
                    design_system_id: "acme".to_owned(),
                }),
            },
            &mut output,
        )
        .expect_err("missing wax home should fail");

        assert!(matches!(
            err,
            ScanCommandError::Paths(PathsError::HomeUnavailable)
        ));
    }

    #[test]
    fn scan_command_warns_and_continues_when_home_is_unavailable_for_best_effort_sync() {
        let _guard = env_lock();
        let _home = EnvVarGuard::remove("HOME");
        let _wax_home = EnvVarGuard::remove("WAX_HOME");
        let _user_profile = EnvVarGuard::remove("USERPROFILE");
        let _home_drive = EnvVarGuard::remove("HOMEDRIVE");
        let _home_path = EnvVarGuard::remove("HOMEPATH");
        let root = TestDir::new("sync-warning-no-home");
        let app_repo = root.path.join("app");
        write_committed_scan_repo_with_upstream(&app_repo);

        let mut output = Vec::new();
        let err = run_scan_cli(
            ScanCommandOptions {
                repo_root: app_repo,
                allow_auto_install: false,
                scan_concurrency: None,
                state_path: None,
                pack_index_url: None,
                target_triple: None,
                ephemeral: None,
            },
            &mut output,
        )
        .expect_err("missing wax home should fail after scan-time sync warning");

        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains(
            "warning: registry sync failed; scanning with current registry source. Run `wax sync` for details."
        ));
        assert!(
            matches!(
                err,
                ScanCommandError::Engine(wax_core::EngineError::Paths(PathsError::HomeUnavailable))
            ),
            "unexpected scan error: {err:?}"
        );
    }

    #[test]
    fn ephemeral_summary_includes_init_hint() {
        let mut output = Vec::new();
        let merged = MergedScan {
            schema_version: SCHEMA_VERSION,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            repo_summary: RepoSummary {
                languages: vec![LanguageId::from_str("react").unwrap()],
                counts: CountSummary::default(),
                metrics: Metrics {
                    invocation_adoption_ratio: None,
                    registry_resolution_ratio: None,
                    token_reference_ratio: None,
                    parse_extract_ms: 0,
                    files_scanned: 0,
                },
            },
            symbol_usage_summary: vec![],
            token_usage_summary: vec![],
            languages: BTreeMap::new(),
        };

        write_scan_summary(
            &mut output,
            &merged,
            std::path::Path::new("/tmp/repo/.wax/out/scan-merged.json"),
            true,
        )
        .unwrap();

        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains("run `wax init`"));
    }

    fn sample_repo_counts() -> CountSummary {
        CountSummary {
            registry: RegistryCounts {
                component_count: 2,
                used_component_count: 2,
                resolved_raw_invocation_count: 7,
                candidate_raw_invocation_count: 1,
            },
            definitions: DefinitionCounts {
                local_definition_count: 4,
                invoked_local_definition_count: 2,
                unused_local_definition_count: 2,
            },
            raw_invocations: RawInvocationCounts {
                total: 9,
                resolved: 7,
                local: 1,
                candidate: 1,
                unresolved: 1,
            },
            adoption: AdoptionCounts {
                eligible_invocation_count: 8,
                adopted_invocation_count: 7,
                non_adopted_invocation_count: 1,
            },
            parent_scopes: ParentScopeCounts {
                total: 2,
                with_resolved_invocations: 2,
                with_local_invocations: 0,
                with_unresolved_invocations: 1,
            },
            tokens: wax_contract::TokenCounts {
                configured_token_count: 2,
                used_token_count: 1,
                token_reference_site_count: 3,
                hardcoded_style_candidate_count: 1,
                token_references_by_category: wax_contract::TokenCategoryCounts {
                    color: 2,
                    spacing: 1,
                    ..Default::default()
                },
                hardcoded_by_category: wax_contract::TokenCategoryCounts {
                    spacing: 1,
                    ..Default::default()
                },
                parent_scopes_with_token_references: 1,
                parent_scopes_with_hardcoded_candidates: 1,
            },
        }
    }

    fn facts_with_status(
        status: ScanStatus,
        invocation_adoption_ratio: Option<f64>,
        diagnostics: Vec<Diagnostic>,
    ) -> ScanFacts {
        ScanFacts {
            schema_version: SCHEMA_VERSION,
            language: LanguageMetadata {
                id: LanguageId::from_str("compose").unwrap(),
                version: "0.1.0".to_owned(),
                ecosystem: "test".to_owned(),
                parser_name: "fixture".to_owned(),
                parser_version: "1.0.0".to_owned(),
            },
            snapshot_id: "snap-1".to_owned(),
            scanned_at: OffsetDateTime::UNIX_EPOCH,
            status,
            design_system_components: Vec::new(),
            local_components: Vec::new(),
            usage_sites: Vec::new(),
            diagnostics,
            metrics: Metrics {
                invocation_adoption_ratio,
                registry_resolution_ratio: None,
                token_reference_ratio: None,
                parse_extract_ms: 1,
                files_scanned: 1,
            },
            counts: CountSummary::default(),
            symbol_usage_summary: vec![],
            design_system_tokens: vec![],
            token_sites: vec![],
            hardcoded_style_sites: vec![],
            token_usage_summary: vec![],
        }
    }

    fn diagnostic(severity: DiagnosticSeverity, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            severity,
            code: code.to_owned(),
            message: message.to_owned(),
            location: None,
        }
    }
}
