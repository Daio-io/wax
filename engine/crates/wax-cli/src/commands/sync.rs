//! `wax sync` command implementation.

use std::io::Write;
use std::path::PathBuf;

use thiserror::Error;
use wax_core::paths::WaxHome;
use wax_core::sync::{SyncError, SyncOptions, sync_app_registries};

/// Options for `wax sync`.
#[derive(Debug, Clone)]
pub struct SyncCommandOptions {
    /// Repository root containing wax config and lockfile.
    pub repo_root: PathBuf,
    /// Wax home to use for remembered registry state.
    pub wax_home: Option<WaxHome>,
    /// Refresh Git-backed registry tags instead of using their locked commits.
    pub upgrade: bool,
    /// Explicit remembered bundle revision to adopt for the same bound system.
    pub design_system: Option<String>,
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
        design_system: options.design_system,
        repo_root: options.repo_root,
        // Core resolves an omitted state path only for syncs that need remembered state.
        state_path: options.wax_home.as_ref().map(WaxHome::state_file),
        upgrade: options.upgrade,
    })?;

    if updates.is_empty() {
        writeln!(writer, "Registry pins are already up to date.")
            .map_err(|source| SyncCommandError::Io { source })?;
        return Ok(());
    }

    for update in updates {
        if let (Some(old), Some(new)) = (&update.old_subdir, &update.new_subdir) {
            writeln!(writer, "{} revision {} -> {}", update.language_id, old, new)
                .map_err(|source| SyncCommandError::Io { source })?;
        }
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
