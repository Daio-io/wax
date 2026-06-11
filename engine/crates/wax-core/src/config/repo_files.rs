//! Repository-local wax file discovery.

use std::path::{Path, PathBuf};

use wax_contract::LanguageId;

/// Preferred repo-local wax config path.
pub const PREFERRED_CONFIG_RELATIVE_PATH: &str = ".wax/wax.config.json";
/// Legacy repo-local wax config path.
pub const LEGACY_CONFIG_RELATIVE_PATH: &str = ".waxrc";
/// Preferred repo-local wax lockfile path.
pub const PREFERRED_LOCKFILE_RELATIVE_PATH: &str = ".wax/wax.lock.json";
/// Legacy repo-local wax lockfile path.
pub const LEGACY_LOCKFILE_RELATIVE_PATH: &str = "wax.lock.json";
/// Default local registry path used when language config omits `registry`.
pub const DEFAULT_REGISTRY_RELATIVE_PATH: &str = ".wax/wax.registry.json";

/// Default per-language registry path when a language entry omits `registry`.
pub fn default_registry_path_for_language(language_id: &LanguageId) -> String {
    format!(".wax/{}.registry.json", language_id.as_str())
}
/// Registry cache directory used for materialized external sources.
pub const REGISTRY_CACHE_RELATIVE_DIR: &str = ".wax/cache/registries";
/// Generated scan output directory.
pub const SCAN_OUTPUT_RELATIVE_DIR: &str = ".wax/out";

/// Repo-local wax files selected for a command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFileSet {
    /// Selected config path.
    pub config_path: PathBuf,
    /// Selected lockfile path.
    pub lockfile_path: PathBuf,
    /// Non-fatal discovery warnings.
    pub warnings: Vec<RepoFileWarning>,
}

/// Warnings emitted when legacy files are present but ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoFileWarning {
    /// A legacy `.waxrc` exists but the preferred config exists too.
    IgnoredLegacyConfig {
        /// Preferred config path.
        preferred: PathBuf,
        /// Ignored legacy config path.
        legacy: PathBuf,
    },
    /// A legacy top-level lockfile exists but the preferred lockfile exists too.
    IgnoredLegacyLockfile {
        /// Preferred lockfile path.
        preferred: PathBuf,
        /// Ignored legacy lockfile path.
        legacy: PathBuf,
    },
    /// Centralized config exists but only the legacy lockfile is present.
    PreferredConfigWithLegacyLockfile {
        /// Preferred config path.
        preferred_config: PathBuf,
        /// Legacy lockfile path selected for use.
        legacy_lockfile: PathBuf,
    },
}

/// Discovers preferred or legacy wax repo files under `repo_root`.
pub fn discover_repo_files(repo_root: impl AsRef<Path>) -> RepoFileSet {
    let repo_root = repo_root.as_ref();
    let preferred_config = repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    let legacy_config = repo_root.join(LEGACY_CONFIG_RELATIVE_PATH);
    let preferred_lockfile = repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH);
    let legacy_lockfile = repo_root.join(LEGACY_LOCKFILE_RELATIVE_PATH);

    let mut warnings = Vec::new();
    let uses_preferred_config = preferred_config.is_file();

    let config_path = if uses_preferred_config {
        if legacy_config.is_file() {
            warnings.push(RepoFileWarning::IgnoredLegacyConfig {
                preferred: preferred_config.clone(),
                legacy: legacy_config,
            });
        }
        preferred_config
    } else if legacy_config.is_file() {
        legacy_config
    } else {
        preferred_config
    };

    let lockfile_path = if preferred_lockfile.is_file() {
        if legacy_lockfile.is_file() {
            warnings.push(RepoFileWarning::IgnoredLegacyLockfile {
                preferred: preferred_lockfile.clone(),
                legacy: legacy_lockfile,
            });
        }
        preferred_lockfile
    } else if legacy_lockfile.is_file() {
        if uses_preferred_config {
            warnings.push(RepoFileWarning::PreferredConfigWithLegacyLockfile {
                preferred_config: repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH),
                legacy_lockfile: legacy_lockfile.clone(),
            });
        }
        legacy_lockfile
    } else {
        preferred_lockfile
    };

    RepoFileSet {
        config_path,
        lockfile_path,
        warnings,
    }
}
