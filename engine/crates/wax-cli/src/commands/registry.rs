//! `wax registry` command implementations.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
    writeln!(
        writer,
        "Run `wax language update` to refresh registry locks."
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    writeln!(
        writer,
        "Run `wax validate` to verify repository configuration."
    )
    .map_err(|source| RegistryDiscoverCommandError::Io { source })?;
    write_config_roots_warning(result.used_config_roots);

    Ok(())
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
) {
    let root_label = if root_count == 1 { "root" } else { "roots" };
    eprintln!(
        "Discovered {component_count} {language_id} registry components from {root_count} {root_label}."
    );
    if dry_run {
        eprintln!("Dry run: no registry file was written.");
    }
    write_config_roots_warning(used_config_roots);
    eprintln!("warning: deterministic discovery may include false positives.");
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
