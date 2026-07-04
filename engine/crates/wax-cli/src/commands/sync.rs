//! `wax sync` command implementation.

use std::io::Write;
use std::path::PathBuf;

use thiserror::Error;
use wax_core::paths::state_file;
use wax_core::sync::{SyncError, SyncOptions, sync_app_registries};

/// Options for `wax sync`.
#[derive(Debug, Clone)]
pub struct SyncCommandOptions {
    /// Repository root containing wax config and lockfile.
    pub repo_root: PathBuf,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
}

/// Errors returned by `wax sync`.
#[derive(Debug, Error)]
pub enum SyncCommandError {
    /// Registry sync orchestration failed.
    #[error(transparent)]
    Sync(#[from] SyncError),
    /// Summary writing failed.
    #[error("failed to write sync summary: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: std::io::Error,
    },
}

/// Runs `wax sync` for the current repository.
pub fn run_sync_cli(
    options: SyncCommandOptions,
    writer: &mut impl Write,
) -> Result<(), SyncCommandError> {
    let state_path = options
        .state_path
        .unwrap_or_else(|| state_file().expect("resolve global state path"));
    let updates = sync_app_registries(&SyncOptions {
        repo_root: options.repo_root,
        state_path,
    })?;

    if updates.is_empty() {
        writeln!(writer, "No registry upstreams configured; nothing to sync.")
            .map_err(|source| SyncCommandError::Io { source })?;
        return Ok(());
    }

    for update in updates {
        writeln!(
            writer,
            "updated {} registry from {} -> {}",
            update.language_id, update.upstream, update.source
        )
        .map_err(|source| SyncCommandError::Io { source })?;
    }
    Ok(())
}
