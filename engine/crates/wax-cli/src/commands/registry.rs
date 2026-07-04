//! Registry discovery command implementations for `wax discover` and `wax registry discover`.

use super::diagnostic_output::format_diagnostic_line;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use wax_contract::Diagnostic;
use wax_core::paths::state_file;
use wax_core::registry_discovery::{
    RegistryDiscoverError, RegistryDiscoverOptions, discover_registry,
};
use wax_core::registry_memory::{
    RegistryMemoryError, delete_remembered_design_system, list_remembered_design_systems,
    show_remembered_design_system, update_remembered_design_system_repo_root,
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
    /// When true, replace an existing registry file.
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
    Paths(#[from] wax_core::paths::PathsError),
    /// Output writing failed.
    #[error("failed to write registry memory output: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax discover` or `wax registry discover`.
pub fn run_registry_discover(
    options: RegistryDiscoverCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryDiscoverCommandError> {
    let language_id = options.language_id.clone();
    let dry_run = options.dry_run;
    let repo_root = options.repo_root.clone();
    let roots = resolve_cli_roots(&repo_root, options.roots);

    let result = discover_registry(RegistryDiscoverOptions {
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
pub fn run_registry_list(
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path)?;
    let entries = list_remembered_design_systems(&state_path)?;

    writeln!(writer, "id\tname\trepo_root").map_err(write_memory_error)?;
    for entry in entries {
        writeln!(
            writer,
            "{}\t{}\t{}",
            entry.id,
            entry.name,
            entry.repo_root.display()
        )
        .map_err(write_memory_error)?;
    }

    Ok(())
}

/// Runs `wax registry show`.
pub fn run_registry_show(
    design_system_id: &str,
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path)?;
    let entry = show_remembered_design_system(&state_path, design_system_id)?;

    writeln!(writer, "id: {}", entry.id).map_err(write_memory_error)?;
    writeln!(writer, "name: {}", entry.name).map_err(write_memory_error)?;
    writeln!(writer, "repo_root: {}", entry.repo_root.display()).map_err(write_memory_error)?;
    writeln!(
        writer,
        "last_seen_config: {}",
        entry.last_seen_config.display()
    )
    .map_err(write_memory_error)?;

    Ok(())
}

/// Runs `wax registry update`.
pub fn run_registry_update(
    options: RegistryUpdateCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path)?;
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
pub fn run_registry_delete(
    design_system_id: &str,
    options: RegistryMemoryCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryMemoryCommandError> {
    let state_path = resolve_state_path(options.state_path)?;
    delete_remembered_design_system(&state_path, design_system_id)?;

    writeln!(
        writer,
        "Deleted remembered design system `{}`.",
        design_system_id
    )
    .map_err(write_memory_error)?;

    Ok(())
}

fn resolve_state_path(state_path: Option<PathBuf>) -> Result<PathBuf, RegistryMemoryCommandError> {
    match state_path {
        Some(path) => Ok(path),
        None => Ok(state_file()?),
    }
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
