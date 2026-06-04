//! `wax registry` command implementations.

use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;
use wax_core::registry_discovery::{
    RegistryDiscoverError, RegistryDiscoverOptions, discover_registry,
};

/// Options for `wax registry discover`.
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
}

/// Errors returned by `wax registry discover`.
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

/// Runs `wax registry discover`.
pub fn run_registry_discover(
    options: RegistryDiscoverCommandOptions,
    writer: &mut impl Write,
) -> Result<(), RegistryDiscoverCommandError> {
    let root_count = options.roots.len();
    let language_id = options.language_id.clone();
    let dry_run = options.dry_run;

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: &options.repo_root,
        language_id: &options.language_id,
        roots: options.roots,
        dry_run: options.dry_run,
        force: options.force,
    })?;

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
        write_diagnostics(component_count, &language_id, root_count, true);
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
        display_output_path(&options.repo_root, &result.output_path)
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    writeln!(
        writer,
        "Review before committing: deterministic discovery may include false positives."
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    writeln!(
        writer,
        "Run `wax validate` to verify repository configuration."
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;

    Ok(())
}

fn write_diagnostics(component_count: usize, language_id: &str, root_count: usize, dry_run: bool) {
    let root_label = if root_count == 1 { "root" } else { "roots" };
    eprintln!(
        "Discovered {component_count} {language_id} registry components from {root_count} {root_label}."
    );
    if dry_run {
        eprintln!("Dry run: no registry file was written.");
    }
    eprintln!("warning: deterministic discovery may include false positives.");
}

fn display_output_path(repo_root: &std::path::Path, output_path: &std::path::Path) -> String {
    output_path
        .strip_prefix(repo_root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| output_path.display().to_string())
}
