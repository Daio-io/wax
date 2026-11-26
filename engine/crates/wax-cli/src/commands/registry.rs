//! Registry discovery command implementations for `wax discover` and `wax registry discover`.

use super::diagnostic_output::format_diagnostic_line;
use super::state_path::resolve_state_path;
use crate::presentation::Presentation;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use wax_contract::{CatalogFindResponse, Diagnostic, LanguageId};
use wax_core::paths::PathsError;
use wax_core::registry_cache::RegistrySourceSpec;
use wax_core::registry_discovery::{
    RegistryDiscoverError, RegistryDiscoverOptions, discover_registry,
};
use wax_core::registry_memory::{
    RegistryMemoryError, add_remembered_registry, delete_remembered_design_system,
    list_remembered_design_systems, refresh_remembered_registry, show_remembered_design_system,
    update_remembered_design_system_repo_root,
};
use wax_core::registry_merge::{
    RegistryMergeChange, RegistryMergeChangeKind, RegistryMergeEntryKind,
};

/// Options for `wax discover` and `wax registry discover`.
#[derive(Debug, Clone)]
pub struct RegistryDiscoverCommandOptions {
    /// Repository root where the registry should be written.
    pub repo_root: PathBuf,
    /// Language pack identifier to discover.
    pub language_id: String,
    /// Source roots inspected by language-specific discovery.
    pub roots: Vec<PathBuf>,
    /// When true, print registry JSON to stdout without writing a file.
    pub dry_run: bool,
    /// Replace the current discovered component set, pruning symbols no longer found.
    pub force: bool,
    /// Design-system id to remember after discovery.
    pub design_system_id: Option<String>,
    /// Display name for the remembered design system.
    pub design_system_name: Option<String>,
}

/// Options for registry memory management commands.
#[derive(Debug, Clone)]
pub struct RegistryMemoryCommandOptions {
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Options for `wax registry update`.
#[derive(Debug, Clone)]
pub struct RegistryUpdateCommandOptions {
    /// Design-system id to update.
    pub design_system_id: String,
    /// New repository root for the remembered design system.
    pub repo_root: PathBuf,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Options for `wax registry add`.
#[derive(Debug, Clone)]
pub struct RegistryAddCommandOptions {
    /// Design-system id used to remember the source.
    pub design_system_id: String,
    /// Display name for list and show output.
    pub name: String,
    /// User-facing local, HTTP, or Git source.
    pub source: String,
    /// Optional language identity.
    pub language: Option<LanguageId>,
    /// Optional Git tag or commit.
    pub tag: Option<String>,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Errors returned by registry discovery commands.
#[derive(Debug, Error)]
pub enum RegistryDiscoverCommandError {
    /// Registry discovery orchestration failed.
    #[error(transparent)]
    Discover(#[from] RegistryDiscoverError),
    /// Output writing failed.
    #[error("failed to write registry discover output: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Errors returned by registry memory management commands.
#[derive(Debug, Error)]
pub enum RegistryMemoryCommandError {
    /// Remembered design-system state could not be read or updated.
    #[error(transparent)]
    Memory(#[from] RegistryMemoryError),
    /// Global path resolution failed.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Output writing failed.
    #[error("failed to write registry memory output: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax registry add`.
///
/// # Errors
///
/// Returns a typed memory/cache error when the source is invalid, cannot be
/// fetched, conflicts with an existing name, or cannot be persisted.
pub fn run_registry_add(
    options: RegistryAddCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let mut source = RegistrySourceSpec::parse(&options.source, options.language)
        .map_err(RegistryMemoryError::from)?;
    if let Some(tag) = options.tag {
        source = source.with_tag(tag).map_err(RegistryMemoryError::from)?;
    }
    let snapshots = add_remembered_registry(
        &state_path,
        &options.design_system_id,
        &options.name,
        &source,
    )?;
    writeln!(
        writer,
        "Remembered design system `{}` with {} registry snapshot(s).",
        options.design_system_id,
        snapshots.len()
    )
    .map_err(write_memory_error)?;
    Ok(())
}

/// Runs `wax registry refresh`.
///
/// # Errors
///
/// Returns a typed memory/cache error when the id is absent, local-only, cannot
/// be fetched, or cannot be persisted.
pub fn run_registry_refresh(
    design_system_id: &str,
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let snapshots = refresh_remembered_registry(&state_path, design_system_id)?;
    writeln!(
        writer,
        "Refreshed design system `{design_system_id}` with {} registry snapshot(s).",
        snapshots.len()
    )
    .map_err(write_memory_error)?;
    Ok(())
}

/// Runs `wax discover` or `wax registry discover`.
///
/// # Errors
///
/// Returns [`RegistryDiscoverCommandError::Discover`] when registry discovery or
/// persistence fails, or [`RegistryDiscoverCommandError::Io`] when dry-run JSON
/// cannot be serialized into command output or output cannot be written.
pub fn run_registry_discover(
    options: RegistryDiscoverCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryDiscoverCommandError> {
    let language_id = options.language_id.clone();
    let dry_run = options.dry_run;
    let repo_root = options.repo_root.clone();
    let roots = resolve_cli_roots(&repo_root, options.roots);

    let result = discover_registry(RegistryDiscoverOptions {
        wax_home: None,
        repo_root: &repo_root,
        language_id: &language_id,
        roots,
        dry_run: options.dry_run,
        force: options.force,
        design_system_id: options.design_system_id.as_deref(),
        design_system_name: options.design_system_name.as_deref(),
    })?;

    let root_count = result.root_count;

    let component_count = result
        .registry
        .get("components")
        .and_then(|components| components.as_array())
        .map(|components| components.len())
        .unwrap_or(0);

    if dry_run {
        let json = serde_json::to_string_pretty(&result.registry).map_err(|source| {
            RegistryDiscoverCommandError::Io {
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            }
        })?;
        writeln!(writer, "{json}").map_err(|source| RegistryDiscoverCommandError::Io { source })?;
        write_diagnostics(
            component_count,
            &language_id,
            root_count,
            true,
            result.used_config_roots,
            &result.diagnostics,
        );
        write_dry_run_changes(&result.changes);
        write_merge_diagnostics(&result.merge_diagnostics);
        return Ok(());
    }

    let root_label = if root_count == 1 { "root" } else { "roots" };
    writeln!(
        writer,
        "Discovered {component_count} {language_id} registry components from {root_count} {root_label}."
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    writeln!(
        writer,
        "Wrote {}.",
        display_output_path(&repo_root, &result.output_path)
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    writeln!(
        writer,
        "Review before committing: deterministic discovery may include false positives."
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    if result.remembered_design_system {
        writeln!(
            writer,
            "Apps can use this registry with `wax init` or refresh existing setups with `wax sync`."
        )
        .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    } else if result.lockfile_present {
        writeln!(
            writer,
            "Run `wax language update` to refresh registry locks."
        )
        .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    }
    if result.wax_config_present && !result.remembered_design_system {
        writeln!(
            writer,
            "Run `wax validate` to verify repository configuration."
        )
        .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    }
    write_config_roots_warning(result.used_config_roots);
    write_pack_diagnostics(&result.diagnostics);

    Ok(())
}

/// Runs `wax registry list`.
///
/// # Errors
///
/// Returns [`RegistryMemoryCommandError::Paths`] when no global state path can be
/// resolved, [`RegistryMemoryCommandError::Memory`] when remembered state cannot
/// be loaded, or [`RegistryMemoryCommandError::Io`] when output cannot be written.
pub fn run_registry_list(
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let entries = list_remembered_design_systems(&state_path)?;

    writeln!(
        writer,
        "id\tname\trepo_root\tsource\tcommit\trevision\tlanguages"
    )
    .map_err(write_memory_error)?;
    for entry in entries {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            entry.id,
            entry.name,
            entry.repo_root.display(),
            entry
                .source
                .as_ref()
                .map_or_else(String::new, RegistrySourceSpec::identity),
            entry
                .bundle_pin
                .as_ref()
                .map_or("", |pin| pin.commit.as_str()),
            entry
                .id
                .strip_prefix("public-")
                .and_then(|id| id.rsplit_once("-r"))
                .map_or("", |(_, revision)| revision),
            entry
                .snapshots
                .iter()
                .map(|s| s.language.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
        .map_err(write_memory_error)?;
    }

    Ok(())
}

/// Runs `wax registry show`.
///
/// # Errors
///
/// Returns [`RegistryMemoryCommandError::Paths`] when no global state path can be
/// resolved, [`RegistryMemoryCommandError::Memory`] when the id is invalid or
/// unknown or state cannot be loaded, or [`RegistryMemoryCommandError::Io`] when
/// output cannot be written.
pub fn run_registry_show(
    design_system_id: &str,
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    let entry = show_remembered_design_system(&state_path, design_system_id)?;

    writeln!(writer, "id: {}", entry.id).map_err(write_memory_error)?;
    writeln!(writer, "name: {}", entry.name).map_err(write_memory_error)?;
    if let Some(pin) = &entry.bundle_pin {
        writeln!(writer, "commit: {}", pin.commit).map_err(write_memory_error)?;
        if let Some((_, revision)) = entry
            .id
            .rsplit_once("-r")
            .filter(|_| entry.id.starts_with("public-"))
        {
            writeln!(writer, "revision: {revision}").map_err(write_memory_error)?;
        }
        writeln!(
            writer,
            "languages: {}",
            pin.registries
                .keys()
                .map(LanguageId::as_str)
                .collect::<Vec<_>>()
                .join(",")
        )
        .map_err(write_memory_error)?;
    }

    if let Some(source) = &entry.source {
        writeln!(writer, "source: {}", source.identity()).map_err(write_memory_error)?;
        writeln!(writer, "snapshots: {}", entry.snapshots.len()).map_err(write_memory_error)?;
    } else {
        writeln!(writer, "repo_root: {}", entry.repo_root.display()).map_err(write_memory_error)?;
        writeln!(
            writer,
            "last_seen_config: {}",
            entry.last_seen_config.display()
        )
        .map_err(write_memory_error)?;
    }

    Ok(())
}

/// Runs `wax registry update`.
///
/// # Errors
///
/// Returns [`RegistryMemoryCommandError::Paths`] when no global state path can be
/// resolved, [`RegistryMemoryCommandError::Memory`] when the id is invalid or
/// unknown, the new repository root cannot be resolved, or state cannot be
/// saved, and [`RegistryMemoryCommandError::Io`] when output cannot be written.
pub fn run_registry_update(
    options: RegistryUpdateCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    update_remembered_design_system_repo_root(
        &state_path,
        &options.design_system_id,
        &options.repo_root,
    )?;

    writeln!(
        writer,
        "Updated design system `{}` repo root to {}.",
        options.design_system_id,
        options.repo_root.display()
    )
    .map_err(write_memory_error)?;

    Ok(())
}

/// Runs `wax registry delete`.
///
/// # Errors
///
/// Returns [`RegistryMemoryCommandError::Paths`] when no global state path can be
/// resolved, [`RegistryMemoryCommandError::Memory`] when the id is invalid or
/// unknown or state cannot be saved, and [`RegistryMemoryCommandError::Io`] when
/// output cannot be written.
pub fn run_registry_delete(
    design_system_id: &str,
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path.as_deref())?;
    delete_remembered_design_system(&state_path, design_system_id)?;

    writeln!(
        writer,
        "Deleted remembered design system `{}`.",
        design_system_id
    )
    .map_err(write_memory_error)?;

    Ok(())
}

fn write_memory_error(source: io::Error) -> RegistryMemoryCommandError {
    RegistryMemoryCommandError::Io { source }
}

fn resolve_cli_roots(repo_root: &Path, roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots
        .into_iter()
        .map(|root| {
            if root.is_absolute() {
                root
            } else {
                repo_root.join(root)
            }
        })
        .collect()
}

fn write_diagnostics(
    component_count: usize,
    language_id: &str,
    root_count: usize,
    dry_run: bool,
    used_config_roots: bool,
    diagnostics: &[Diagnostic],
) {
    let root_label = if root_count == 1 { "root" } else { "roots" };
    eprintln!(
        "Discovered {component_count} {language_id} registry components from {root_count} {root_label}."
    );
    if dry_run {
        eprintln!("Dry run: no registry file was written.");
    }
    write_config_roots_warning(used_config_roots);
    write_pack_diagnostics(diagnostics);
    eprintln!("warning: deterministic discovery may include false positives.");
}

fn write_pack_diagnostics(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }

    eprintln!("discovery diagnostics ({}):", diagnostics.len());
    for diagnostic in diagnostics {
        eprintln!("  {}", format_diagnostic_line(diagnostic));
    }
}

fn write_dry_run_changes(changes: &[RegistryMergeChange]) {
    if changes.is_empty() {
        eprintln!("Dry-run changes: none (rediscovery is unchanged).");
        return;
    }

    eprintln!("Dry-run changes ({}):", changes.len());
    for change in changes {
        let entry_kind = match change.entry_kind {
            RegistryMergeEntryKind::Component => "component",
            RegistryMergeEntryKind::Token => "token",
        };
        let kind = match change.kind {
            RegistryMergeChangeKind::Added => "added",
            RegistryMergeChangeKind::Updated => "updated",
            RegistryMergeChangeKind::Removed => "removed",
        };
        eprintln!("  {kind} {entry_kind} {}", change.id);
    }
    eprintln!("Dry run keeps the existing registry file intact; no destructive regeneration.");
}

fn write_merge_diagnostics(diagnostics: &[wax_core::registry_merge::RegistryMergeDiagnostic]) {
    if diagnostics.is_empty() {
        return;
    }

    eprintln!("merge diagnostics ({}):", diagnostics.len());
    for diagnostic in diagnostics {
        eprintln!("  {}: {}", diagnostic.discovered_id, diagnostic.message);
    }
}

fn write_config_roots_warning(used_config_roots: bool) {
    if used_config_roots {
        eprintln!(
            "warning: using configured language roots; prefer --root path/to/design-system when scanning a design-system package."
        );
    }
}

fn display_output_path(repo_root: &Path, output_path: &Path) -> String {
    output_path
        .strip_prefix(repo_root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| output_path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_find_preserves_order_and_only_suggests_available_public_revisions() {
        let mut configured = find_row("configured", "acme", None, true);
        configured.name = "Acme Local".into();
        configured.languages.clear();
        let mut response = find_response(vec![
            configured,
            find_row("public", "acme", Some(2), true),
            find_row("public", "acme", Some(1), false),
            find_row("remembered", "public-acme-r2", Some(2), true),
        ]);
        response.public_status.commit = Some("0123456789abcdef0123456789abcdef01234567".into());

        assert_eq!(
            human_find(&response, false),
            concat!(
                "Registry results (4)\n",
                "--------------------\n\n",
                "Acme Local\n  configured | acme\n  Languages: none listed\n\n",
                "Acme\n  public | acme @ 2 | Installed\n  Languages: compose, react\n\n",
                "Acme\n  public | acme @ 1 | Available\n  Languages: compose, react\n",
                "  Install:\n    wax registry install acme --version 1\n\n",
                "Acme\n  remembered | public-acme-r2 @ 2\n  Languages: compose, react\n\n",
                "Public catalogue: complete\n",
                "  Commit: 0123456789abcdef0123456789abcdef01234567\n",
            )
        );
    }

    #[test]
    fn human_find_offline_explains_bundle_cache_once_when_hints_are_present() {
        let mut response = find_response(vec![
            find_row("public", "acme", Some(1), false),
            find_row("public", "acme", Some(2), false),
        ]);
        response.public_status.cached = true;
        let output = human_find(&response, true);
        assert!(output.contains("Public catalogue: complete (cached)\n"));
        assert!(output.contains("    wax registry install acme --version 1 --offline\n"));
        assert!(output.contains("    wax registry install acme --version 2 --offline\n"));
        let cache_note = "Offline installs require cached bundle objects.";
        assert_eq!(output.matches(cache_note).count(), 1);

        for row in &mut response.results {
            row.installed = true;
        }
        let output = human_find(&response, true);
        assert!(!output.contains("wax registry install"));
        assert!(!output.contains(cache_note));
    }

    #[test]
    fn human_find_distinguishes_empty_complete_and_cold_offline_results() {
        let mut response = find_response(vec![]);
        assert_eq!(
            human_find(&response, false),
            "No matching design systems.\n\nPublic catalogue: complete\n"
        );
        response.public_status.complete = false;
        response.public_status.diagnostic = Some(wax_contract::CatalogDiagnostic {
            code: "offline_cache_miss".into(),
            message: "public catalogue is not available in the offline cache".into(),
        });
        assert_eq!(
            human_find(&response, true),
            concat!(
                "No matching design systems in available sources.\n\n",
                "Public catalogue: incomplete\n",
                "  offline_cache_miss: public catalogue is not available in the offline cache\n"
            )
        );
    }

    #[test]
    fn human_find_returns_writer_errors() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let error =
            write_registry_find_human(&find_response(vec![]), false, &mut Broken).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn human_find_escapes_terminal_controls_and_preserves_printable_unicode() {
        let mut row = find_row("configured", "a\tcme", None, true);
        row.name = "Élan\n\x1b[31m".into();
        let mut response = find_response(vec![row]);
        response.public_status.complete = false;
        response.public_status.diagnostic = Some(wax_contract::CatalogDiagnostic {
            code: "git\rerror".into(),
            message: "Échec\x1b[0m\nretry".into(),
        });

        let output = human_find(&response, false);
        assert!(output.contains("Élan\\n\\u{1b}[31m\n  configured | a\\tcme\n"));
        assert!(output.contains("  git\\rerror: Échec\\u{1b}[0m\\nretry\n"));
        assert!(!output.contains(['\x1b', '\r', '\t']));
    }

    fn find_row(
        origin: &str,
        id: &str,
        revision: Option<u64>,
        installed: bool,
    ) -> wax_contract::CatalogFindResult {
        wax_contract::CatalogFindResult {
            origin: origin.into(),
            id: id.into(),
            name: "Acme".into(),
            revision,
            languages: vec!["compose".parse().unwrap(), "react".parse().unwrap()],
            upstream: vec![],
            coverage: vec![],
            gaps: vec![],
            installed,
            catalogue_commit: None,
        }
    }

    fn find_response(results: Vec<wax_contract::CatalogFindResult>) -> CatalogFindResponse {
        CatalogFindResponse {
            schema_version: 1,
            results,
            public_status: wax_contract::CatalogPublicStatus {
                complete: true,
                cached: false,
                commit: None,
                diagnostic: None,
            },
        }
    }

    fn human_find(response: &CatalogFindResponse, offline: bool) -> String {
        let mut bytes = Vec::new();
        write_registry_find_human(response, offline, &mut bytes).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn relative_roots_are_resolved_against_repo_root() {
        let repo_root = PathBuf::from("/tmp/repo");
        let roots = resolve_cli_roots(&repo_root, vec![PathBuf::from("src/main/kotlin")]);

        assert_eq!(roots, vec![PathBuf::from("/tmp/repo/src/main/kotlin")]);
    }

    #[test]
    fn absolute_roots_are_left_unchanged() {
        let repo_root = PathBuf::from("/tmp/repo");
        let absolute = PathBuf::from("/abs/design-system/src/main/kotlin");
        let roots = resolve_cli_roots(&repo_root, vec![absolute.clone()]);

        assert_eq!(roots, vec![absolute]);
    }
}

/// Failure from public catalogue commands.
#[derive(Debug, Error)]
pub enum RegistryCatalogCommandError {
    /// Catalogue operation failed.
    #[error(transparent)]
    Catalog(#[from] wax_core::registry_catalog::CatalogError),
    /// Global paths are unavailable.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Output could not be written.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// JSON output encoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
/// Runs public/local catalogue discovery, keeping partial failure observable in stdout.
///
/// # Errors
/// Returns a typed error for invalid local state/config or output failure.
pub fn run_registry_find(
    args: crate::cli::RegistryFindArgs,
    repo_root: PathBuf,
    writer: &mut impl Write,
) -> Result<(), RegistryCatalogCommandError> {
    run_registry_find_with_presentation(args, repo_root, Presentation::Plain, writer)
}

/// Runs catalogue discovery using the requested presentation for text output.
///
/// JSON always uses the catalogue response envelope.
///
/// # Errors
/// Returns a typed error for invalid local state/config or output failure.
pub fn run_registry_find_with_presentation(
    args: crate::cli::RegistryFindArgs,
    repo_root: PathBuf,
    presentation: Presentation,
    writer: &mut impl Write,
) -> Result<(), RegistryCatalogCommandError> {
    let mut options =
        wax_core::registry_catalog::CatalogOptions::new(repo_root, resolve_state_path(None)?);
    options.query = args.query;
    options.language = args.language;
    options.offline = args.offline;
    let response = wax_core::registry_catalog::find_design_systems(&options)?;
    match args.format {
        crate::cli::LookupStdoutFormat::Json => {
            serde_json::to_writer(&mut *writer, &response)?;
            writeln!(writer)?;
        }
        crate::cli::LookupStdoutFormat::Text if presentation == Presentation::Human => {
            write_registry_find_human(&response, args.offline, writer)?;
        }
        crate::cli::LookupStdoutFormat::Text => {
            writeln!(writer, "origin\tid\trevision\tname\tlanguages\tinstalled")?;
            for row in response.results {
                writeln!(
                    writer,
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    row.origin,
                    row.id,
                    row.revision.map_or_else(String::new, |r| r.to_string()),
                    row.name,
                    row.languages
                        .iter()
                        .map(LanguageId::as_str)
                        .collect::<Vec<_>>()
                        .join(","),
                    row.installed
                )?;
            }
            writeln!(
                writer,
                "public: complete={} cached={} commit={}",
                response.public_status.complete,
                response.public_status.cached,
                response.public_status.commit.as_deref().unwrap_or("none")
            )?;
            if let Some(diagnostic) = response.public_status.diagnostic {
                writeln!(writer, "{}: {}", diagnostic.code, diagnostic.message)?;
            }
        }
    }
    Ok(())
}

fn write_registry_find_human(
    response: &CatalogFindResponse,
    offline: bool,
    writer: &mut impl Write,
) -> io::Result<()> {
    let status = &response.public_status;
    let mut has_offline_hint = false;
    if response.results.is_empty() {
        let message = if status.complete {
            "No matching design systems."
        } else {
            "No matching design systems in available sources."
        };
        writeln!(writer, "{message}\n")?;
    } else {
        let heading = format!("Registry results ({})", response.results.len());
        writeln!(writer, "{heading}\n{}\n", "-".repeat(heading.len()))?;
        for row in &response.results {
            writeln!(writer, "{}", escape_terminal_controls(&row.name))?;
            write!(
                writer,
                "  {} | {}",
                row.origin,
                escape_terminal_controls(&row.id)
            )?;
            if let Some(revision) = row.revision {
                write!(writer, " @ {revision}")?;
            }
            if row.origin == "public" {
                let installed = if row.installed {
                    "Installed"
                } else {
                    "Available"
                };
                write!(writer, " | {installed}")?;
            }
            writeln!(writer)?;
            let languages = row
                .languages
                .iter()
                .map(LanguageId::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let languages = if languages.is_empty() {
                "none listed"
            } else {
                &languages
            };
            writeln!(writer, "  Languages: {languages}")?;
            if row.origin == "public"
                && !row.installed
                && let Some(revision) = row.revision.filter(|revision| *revision > 0)
            {
                let offline_flag = if offline { " --offline" } else { "" };
                writeln!(
                    writer,
                    "  Install:\n    wax registry install {} --version {revision}{offline_flag}",
                    row.id
                )?;
                has_offline_hint = offline;
            }
            writeln!(writer)?;
        }
    }
    let completeness = match (status.complete, status.cached) {
        (true, false) => "complete",
        (true, true) => "complete (cached)",
        (false, _) => "incomplete",
    };
    writeln!(writer, "Public catalogue: {completeness}")?;
    if let Some(commit) = &status.commit {
        writeln!(writer, "  Commit: {commit}")?;
    }
    if let Some(diagnostic) = &status.diagnostic {
        writeln!(
            writer,
            "  {}: {}",
            escape_terminal_controls(&diagnostic.code),
            escape_terminal_controls(&diagnostic.message)
        )?;
    }
    if has_offline_hint {
        writeln!(writer, "  Offline installs require cached bundle objects.")?;
    }
    Ok(())
}

fn escape_terminal_controls(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

/// Installs a public revision without modifying application files.
///
/// # Errors
/// Returns typed catalogue, storage, or output errors.
pub fn run_registry_install(
    args: crate::cli::RegistryInstallArgs,
    writer: &mut impl Write,
) -> Result<(), RegistryCatalogCommandError> {
    let mut options = wax_core::registry_catalog::CatalogOptions::new(
        PathBuf::from("."),
        resolve_state_path(None)?,
    );
    options.offline = args.offline;
    let pin =
        wax_core::registry_catalog::install_design_system(&options, &args.public_id, args.version)?;
    writeln!(
        writer,
        "Installed public-{}-r{} at {} (languages: {}).",
        args.public_id,
        args.version,
        pin.commit,
        pin.registries
            .keys()
            .map(LanguageId::as_str)
            .collect::<Vec<_>>()
            .join(",")
    )?;
    Ok(())
}
