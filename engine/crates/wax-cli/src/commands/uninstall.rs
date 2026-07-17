//! `wax uninstall` command implementation.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;
use wax_core::paths::{PathsError, wax_home};

/// Options for `wax uninstall`.
#[derive(Debug, Clone)]
pub struct UninstallCliOptions {
    /// Remove all global wax state and binaries.
    pub full: bool,
}

/// Errors returned by `wax uninstall`.
#[derive(Debug, Error)]
pub enum UninstallCliError {
    /// `--full` was not provided.
    #[error("`wax uninstall` requires `--full`")]
    FullFlagRequired,
    /// The global wax home path could not be resolved.
    #[error(transparent)]
    Paths(#[from] PathsError),
    /// Removing global state failed.
    #[error("failed to remove global wax state at {path}: {source}")]
    RemoveWaxHome {
        /// Path that could not be removed.
        path: String,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
    /// Writing CLI output failed.
    #[error("failed to write uninstall summary: {source}")]
    Io {
        /// Underlying write error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax uninstall`.
///
/// # Errors
///
/// Returns [`UninstallCliError::FullFlagRequired`] unless `--full` was supplied,
/// [`UninstallCliError::Paths`] when the global wax home cannot be resolved,
/// [`UninstallCliError::RemoveWaxHome`] when that directory cannot be removed, or
/// [`UninstallCliError::Io`] when the removal report cannot be written.
pub fn run_uninstall_cli(
    options: UninstallCliOptions,
    writer: &mut impl Write,
) -> Result<(), UninstallCliError> {
    if !options.full {
        return Err(UninstallCliError::FullFlagRequired);
    }

    let wax_home = wax_home()?;
    let mut removed_wax_home = false;
    if wax_home.exists() {
        fs::remove_dir_all(&wax_home).map_err(|source| UninstallCliError::RemoveWaxHome {
            path: wax_home.display().to_string(),
            source,
        })?;
        removed_wax_home = true;
    }

    let (removed_bins, failed_bins) = remove_binary_candidates(binary_candidates());

    writeln!(
        writer,
        "removed global state: {} ({})",
        wax_home.display(),
        if removed_wax_home {
            "deleted"
        } else {
            "not present"
        }
    )
    .map_err(write_error)?;

    if removed_bins.is_empty() {
        writeln!(writer, "removed binaries: none").map_err(write_error)?;
    } else {
        writeln!(writer, "removed binaries:").map_err(write_error)?;
        for path in &removed_bins {
            writeln!(writer, "  {}", path.display()).map_err(write_error)?;
        }
    }

    if failed_bins.is_empty() {
        writeln!(writer, "binary removal errors: none").map_err(write_error)?;
    } else {
        writeln!(writer, "binary removal errors:").map_err(write_error)?;
        for (path, err) in &failed_bins {
            writeln!(writer, "  {}: {}", path.display(), err).map_err(write_error)?;
        }
    }

    Ok(())
}

fn write_error(source: io::Error) -> UninstallCliError {
    UninstallCliError::Io { source }
}

fn binary_candidates() -> Vec<PathBuf> {
    let mut candidates = BTreeSet::new();
    candidates.insert(PathBuf::from("/usr/local/bin/wax"));

    if let Some(home) = std::env::var_os("HOME") {
        candidates.insert(PathBuf::from(home).join(".wax").join("bin").join("wax"));
    }

    if let Ok(current_exe) = std::env::current_exe()
        && current_exe.file_name().is_some_and(|name| name == "wax")
    {
        candidates.insert(current_exe);
    }

    candidates.into_iter().collect()
}

fn remove_binary_candidates(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<(PathBuf, io::Error)>) {
    let mut removed = Vec::new();
    let mut failed = Vec::new();

    for path in paths {
        if !path.exists() {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => failed.push((path, err)),
        }
    }

    (removed, failed)
}

#[cfg(test)]
mod tests {
    use super::{UninstallCliError, UninstallCliOptions, run_uninstall_cli};
    use crate::testing::env_lock;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn requires_full_flag() {
        let mut output = Vec::new();
        let err = run_uninstall_cli(UninstallCliOptions { full: false }, &mut output).unwrap_err();
        assert!(matches!(err, UninstallCliError::FullFlagRequired));
    }

    #[test]
    fn removes_wax_home_and_reports_binary_cleanup() {
        let _guard = env_lock();
        let temp_home = unique_temp_dir("uninstall-full");
        fs::create_dir_all(temp_home.join(".wax/bin")).unwrap();
        fs::create_dir_all(temp_home.join(".wax/langs/compose/0.1.0")).unwrap();
        fs::write(temp_home.join(".wax/state.json"), b"{}\n").unwrap();
        fs::write(temp_home.join(".wax/bin/wax"), b"binary\n").unwrap();

        let _home_guard = EnvVarGuard::set("HOME", &temp_home);

        let mut output = Vec::new();
        run_uninstall_cli(UninstallCliOptions { full: true }, &mut output).unwrap();

        assert!(!temp_home.join(".wax").exists());
        let stdout = String::from_utf8(output).unwrap();
        assert!(stdout.contains("removed global state:"));
        assert!(stdout.contains("deleted"));
        assert!(stdout.contains("removed binaries:"));

        let _ = fs::remove_dir_all(temp_home);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wax-cli-{prefix}-{nanos}"))
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(name);
            unsafe {
                std::env::set_var(name, value);
            }
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.name, value),
                    None => std::env::remove_var(self.name),
                }
            }
        }
    }
}
