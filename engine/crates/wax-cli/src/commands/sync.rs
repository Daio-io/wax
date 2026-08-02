//! `wax sync` command implementation.

use std::io::Write;
use std::path::PathBuf;

use thiserror::Error;
use wax_core::sync::{SyncError, SyncOptions, sync_app_registries};

/// Options for `wax sync`.
#[derive(Debug, Clone)]
pub struct SyncCommandOptions {
    /// Repository root containing wax config and lockfile.
    pub repo_root: PathBuf,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
    /// Refresh Git-backed registry tags instead of using their locked commits.
    pub upgrade: bool,
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
///
/// # Errors
///
/// Returns [`SyncCommandError::Sync`] when registry sync fails, or
/// [`SyncCommandError::Io`] when output cannot be written.
pub fn run_sync_cli(
    options: SyncCommandOptions,
    writer: &mut impl Write,
) -> Result<(), SyncCommandError> {
    let updates = sync_app_registries(&SyncOptions {
        repo_root: options.repo_root,
        state_path: options.state_path,
        upgrade: options.upgrade,
    })?;

    if updates.is_empty() {
        writeln!(writer, "Registry pins are already up to date.")
            .map_err(|source| SyncCommandError::Io { source })?;
        return Ok(());
    }

    for update in updates {
        if let (Some(_git), Some(tag), Some(new_commit)) =
            (update.git, update.tag, update.new_commit)
        {
            let old = update.old_commit.as_deref().unwrap_or("<none>");
            writeln!(
                writer,
                "updated {} git registry tag {}: {} -> {}",
                update.language_id, tag, old, new_commit
            )
        } else {
            writeln!(
                writer,
                "updated {} registry from {} -> {}",
                update.language_id, update.upstream, update.source
            )
        }
        .map_err(|source| SyncCommandError::Io { source })?;
    }
    Ok(())
}
