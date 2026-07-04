//! Repository-local wax file discovery.

use std::path::{Path, PathBuf};

use wax_contract::LanguageId;

/// Preferred repo-local wax config path.
pub const PREFERRED_CONFIG_RELATIVE_PATH: &str = ".wax/wax.config.json";
/// Preferred repo-local wax lockfile path.
pub const PREFERRED_LOCKFILE_RELATIVE_PATH: &str = ".wax/wax.lock.json";
/// Registry cache directory used for materialized external sources.
pub const REGISTRY_CACHE_RELATIVE_DIR: &str = ".wax/cache/registries";
/// Generated scan output directory.
pub const SCAN_OUTPUT_RELATIVE_DIR: &str = ".wax/out";

/// Default per-language registry path when a language entry omits `registry`.
pub fn default_registry_path_for_language_id(language_id: &str) -> String {
    format!(".wax/{language_id}.registry.json")
}

/// Default per-language registry path when a language entry omits `registry`.
pub fn default_registry_path_for_language(language_id: &LanguageId) -> String {
    default_registry_path_for_language_id(language_id.as_str())
}

/// Repo-local wax files selected for a command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFileSet {
    /// Selected config path.
    pub config_path: PathBuf,
    /// Selected lockfile path.
    pub lockfile_path: PathBuf,
}

/// Discovers wax repo files under `repo_root`.
///
/// Only `.wax/wax.config.json` and `.wax/wax.lock.json` are supported.
pub fn discover_repo_files(repo_root: impl AsRef<Path>) -> RepoFileSet {
    let repo_root = repo_root.as_ref();
    RepoFileSet {
        config_path: repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH),
        lockfile_path: repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH),
    }
}
