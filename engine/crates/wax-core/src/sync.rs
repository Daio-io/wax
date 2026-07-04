//! App registry sync from remembered design systems.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;
use wax_contract::LanguageId;

use crate::config::lockfile::{LockedRegistry, WAX_LOCK_SCHEMA_VERSION, WaxLock, load_lockfile};
use crate::config::repo_files::discover_repo_files;
use crate::config::waxrc::{LanguageEntry, WaxRc, WaxRcError, load_waxrc};
use crate::registry_memory::{
    RegistryMemoryError, copy_design_system_registry_to_app, resolve_remembered_registry,
    show_remembered_design_system,
};
use crate::registry_source::{RegistrySourceInput, resolve_registry_source};

/// Options for syncing app registries from remembered design systems.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Repository root containing `.wax/wax.config.json` and `.wax/wax.lock.json`.
    pub repo_root: PathBuf,
    /// Global wax state path containing remembered design systems.
    pub state_path: PathBuf,
}

/// One language registry refreshed during sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncUpdate {
    /// Language id whose registry inputs were refreshed.
    pub language_id: LanguageId,
    /// Upstream reference in `<design-system-id>/<language-id>` form.
    pub upstream: String,
    /// Registry source written to app config after sync.
    pub source: String,
}

type BestEffortSyncResult = Result<(Vec<SyncUpdate>, Vec<(String, SyncError)>), SyncError>;

/// Errors returned while syncing app registries.
#[derive(Debug, Error)]
pub enum SyncError {
    /// Wax config could not be loaded.
    #[error(transparent)]
    Config(#[from] WaxRcError),
    /// Lockfile could not be loaded or saved.
    #[error(transparent)]
    Lockfile(#[from] crate::config::lockfile::LockfileError),
    /// Remembered design-system resolution failed.
    #[error(transparent)]
    RegistryMemory(#[from] RegistryMemoryError),
    /// Registry source resolution failed.
    #[error(transparent)]
    RegistrySource(#[from] crate::registry_source::RegistrySourceError),
    /// Wax config is missing from the repository.
    #[error("wax config not found at {path}")]
    MissingConfig {
        /// Expected config path.
        path: PathBuf,
    },
    /// Wax lockfile is missing from the repository.
    #[error("wax lockfile not found at {path}")]
    MissingLockfile {
        /// Expected lockfile path.
        path: PathBuf,
    },
    /// Upstream metadata could not be parsed for a language entry.
    #[error(
        "invalid registry upstream `{upstream}` for language `{language_id}`; expected `<design-system-id>/<language-id>`"
    )]
    InvalidUpstream {
        /// Upstream string from config.
        upstream: String,
        /// Language id from config.
        language_id: LanguageId,
    },
    /// Wax config could not be read or updated on disk.
    #[error("failed to update wax config at {path}: {source}")]
    ConfigUpdate {
        /// Config path that failed to update.
        path: String,
        /// Underlying failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Lockfile could not be written to disk.
    #[error("failed to write wax lockfile at {path}: {source}")]
    LockfileWrite {
        /// Lockfile path that failed to write.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
}

/// Refreshes app registry inputs for every configured upstream reference.
pub fn sync_app_registries(options: &SyncOptions) -> Result<Vec<SyncUpdate>, SyncError> {
    let repo_files = discover_repo_files(&options.repo_root);
    ensure_repo_files_exist(&repo_files)?;

    let waxrc = load_waxrc(&repo_files.config_path)?;
    let mut lockfile = load_lockfile(&repo_files.lockfile_path)?;
    let config_path_display = repo_files.config_path.display().to_string();
    let mut config_json = read_config_json(&repo_files.config_path, &config_path_display)?;
    let mut updates = Vec::new();
    let mut config_changed = false;

    for entry in upstream_language_entries(&waxrc) {
        let update = sync_language_upstream(options, entry, &mut config_json, &mut config_changed)?;
        updates.push(update);
    }

    if config_changed {
        write_config_json(&repo_files.config_path, &config_path_display, &config_json)?;
    }

    let waxrc = load_waxrc(&repo_files.config_path)?;
    refresh_registry_locks(&mut lockfile, &options.repo_root, &waxrc)?;
    save_lockfile(&repo_files.lockfile_path, &lockfile)?;

    Ok(updates)
}

/// Attempts sync for each configured upstream, applying successful updates.
pub fn best_effort_sync_app_registries(options: &SyncOptions) -> BestEffortSyncResult {
    let repo_files = discover_repo_files(&options.repo_root);
    ensure_repo_files_exist(&repo_files)?;

    let waxrc = load_waxrc(&repo_files.config_path)?;
    let mut lockfile = load_lockfile(&repo_files.lockfile_path)?;
    let config_path_display = repo_files.config_path.display().to_string();
    let mut config_json = read_config_json(&repo_files.config_path, &config_path_display)?;

    let mut updates = Vec::new();
    let mut failures = Vec::new();
    let mut config_changed = false;

    for entry in upstream_language_entries(&waxrc) {
        let upstream = entry
            .registry_source
            .as_ref()
            .and_then(|registry| registry.upstream.as_deref())
            .expect("upstream language entries always have upstream metadata");
        match sync_language_upstream(options, entry, &mut config_json, &mut config_changed) {
            Ok(update) => updates.push(update),
            Err(error) => failures.push((upstream.to_owned(), error)),
        }
    }

    if config_changed {
        write_config_json(&repo_files.config_path, &config_path_display, &config_json)?;
    }

    if !updates.is_empty() {
        let waxrc = load_waxrc(&repo_files.config_path)?;
        refresh_registry_locks(&mut lockfile, &options.repo_root, &waxrc)?;
        save_lockfile(&repo_files.lockfile_path, &lockfile)?;
    }

    Ok((updates, failures))
}

fn ensure_repo_files_exist(
    repo_files: &crate::config::repo_files::RepoFileSet,
) -> Result<(), SyncError> {
    if !repo_files.config_path.is_file() {
        return Err(SyncError::MissingConfig {
            path: repo_files.config_path.clone(),
        });
    }
    if !repo_files.lockfile_path.is_file() {
        return Err(SyncError::MissingLockfile {
            path: repo_files.lockfile_path.clone(),
        });
    }
    Ok(())
}

fn upstream_language_entries(waxrc: &WaxRc) -> impl Iterator<Item = &LanguageEntry> {
    waxrc.languages.iter().filter(|entry| {
        entry
            .registry_source
            .as_ref()
            .and_then(|registry| registry.upstream.as_ref())
            .is_some_and(|upstream| !upstream.trim().is_empty())
    })
}

fn sync_language_upstream(
    options: &SyncOptions,
    entry: &LanguageEntry,
    config_json: &mut Value,
    config_changed: &mut bool,
) -> Result<SyncUpdate, SyncError> {
    let upstream = entry
        .registry_source
        .as_ref()
        .and_then(|registry| registry.upstream.as_deref())
        .expect("upstream language entries always have upstream metadata");
    let design_system_id = parse_upstream_design_system_id(upstream, &entry.id)?;
    let remembered = show_remembered_design_system(&options.state_path, design_system_id)?;
    let resolved = resolve_remembered_registry(&remembered, &entry.id)?;
    if let Some(local_source) = resolved.design_system_local_source.as_deref() {
        copy_design_system_registry_to_app(
            &remembered,
            local_source,
            &options.repo_root,
            &resolved.config_source,
        )?;
    }
    if update_config_registry_source(config_json, &entry.id, &resolved.config_source)? {
        *config_changed = true;
    }
    Ok(SyncUpdate {
        language_id: entry.id.clone(),
        upstream: resolved.upstream,
        source: resolved.config_source,
    })
}

fn parse_upstream_design_system_id<'a>(
    upstream: &'a str,
    language_id: &LanguageId,
) -> Result<&'a str, SyncError> {
    let (design_system_id, upstream_language) =
        upstream
            .split_once('/')
            .ok_or_else(|| SyncError::InvalidUpstream {
                upstream: upstream.to_owned(),
                language_id: language_id.clone(),
            })?;
    if design_system_id.is_empty()
        || upstream_language.is_empty()
        || upstream_language != language_id.as_str()
    {
        return Err(SyncError::InvalidUpstream {
            upstream: upstream.to_owned(),
            language_id: language_id.clone(),
        });
    }
    Ok(design_system_id)
}

fn read_config_json(path: &Path, path_display: &str) -> Result<Value, SyncError> {
    let contents = fs::read_to_string(path).map_err(|source| SyncError::ConfigUpdate {
        path: path_display.to_owned(),
        source: Box::new(source),
    })?;
    serde_json::from_str(&contents).map_err(|source| SyncError::ConfigUpdate {
        path: path_display.to_owned(),
        source: Box::new(source),
    })
}

fn write_config_json(path: &Path, path_display: &str, config: &Value) -> Result<(), SyncError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| SyncError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        })?;
    }
    let serialized =
        serde_json::to_string_pretty(config).map_err(|source| SyncError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        })?;
    fs::write(path, format!("{serialized}\n")).map_err(|source| SyncError::ConfigUpdate {
        path: path_display.to_owned(),
        source: Box::new(source),
    })
}

fn update_config_registry_source(
    config: &mut Value,
    language_id: &LanguageId,
    source: &str,
) -> Result<bool, SyncError> {
    let Some(language) = config
        .get_mut("languages")
        .and_then(Value::as_object_mut)
        .and_then(|languages| languages.get_mut(language_id.as_str()))
        .and_then(Value::as_object_mut)
    else {
        return Ok(false);
    };
    let Some(registry) = language.get_mut("registry").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    let current = registry
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if current == source {
        return Ok(false);
    }
    registry.insert("source".to_owned(), Value::String(source.to_owned()));
    Ok(true)
}

fn refresh_registry_locks(
    lockfile: &mut WaxLock,
    repo_root: &Path,
    waxrc: &WaxRc,
) -> Result<(), SyncError> {
    for entry in &waxrc.languages {
        let resolved = resolve_registry_source(RegistrySourceInput {
            repo_root,
            language_id: entry.id.as_str(),
            source: entry
                .registry_source
                .as_ref()
                .map(|setting| setting.source.as_str()),
        })?;
        lockfile.registries.insert(
            entry.id.clone(),
            LockedRegistry {
                source: resolved.source,
                sha256: resolved.sha256,
            },
        );
    }
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    Ok(())
}

fn save_lockfile(path: &Path, lockfile: &WaxLock) -> Result<(), SyncError> {
    let mut lockfile = lockfile.clone();
    lockfile.schema_version = WAX_LOCK_SCHEMA_VERSION;
    let contents =
        serde_json::to_string_pretty(&lockfile).map_err(|source| SyncError::LockfileWrite {
            path: path.display().to_string(),
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;
    fs::write(path, format!("{contents}\n")).map_err(|source| SyncError::LockfileWrite {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::registry_memory::remember_design_system;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("wax-core-sync-{name}-{nonce}"));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_app_repo(app_repo: &Path, upstream: &str, source: &str) {
        fs::create_dir_all(app_repo.join(".wax/registries/acme")).expect("create registries dir");
        fs::write(
            app_repo.join(".wax/registries/acme/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
        )
        .expect("write app registry");
        fs::write(
            app_repo.join(".wax/wax.config.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "languages": {{
    "react": {{
      "roots": ["src"],
      "registry": {{
        "source": "{source}",
        "upstream": "{upstream}"
      }}
    }}
  }}
}}
"#
            ),
        )
        .expect("write app config");
        fs::write(
            app_repo.join(".wax/wax.lock.json"),
            r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0-test",
  "locked_at": null,
  "registries": {
    "react": {
      "source": ".wax/registries/acme/react.json",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  },
  "languages": {}
}
"#,
        )
        .expect("write app lockfile");
    }

    fn setup_remembered_local_ds(root: &Path) -> (PathBuf, PathBuf) {
        let ds_repo = root.join("acme-ds");
        fs::create_dir_all(ds_repo.join(".wax/registries")).expect("create ds registries dir");
        fs::write(
            ds_repo.join(".wax/registries/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"}]}"#,
        )
        .expect("write ds registry");
        fs::write(
            ds_repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json"
        }
      }
    }
  }
}
"#,
        )
        .expect("write ds config");

        let wax_home = root.join("wax-home");
        fs::create_dir_all(&wax_home).expect("create wax home");
        let state_path = wax_home.join("state.json");
        remember_design_system(&state_path, "acme", "Acme Design System", &ds_repo)
            .expect("remember design system");
        (ds_repo, state_path)
    }

    #[test]
    fn sync_copies_local_design_system_registry_changes_into_app_repo() {
        let _guard = env_lock();
        let root = TestDir::new("copy-local");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let (ds_repo, state_path) = setup_remembered_local_ds(&root.path);

        fs::write(
            ds_repo.join(".wax/registries/react.json"),
            r#"{"schema_version":1,"components":[{"name":"Button"},{"name":"Card"}]}"#,
        )
        .expect("update ds registry");

        let updates = sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path,
        })
        .expect("sync app registries");

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].upstream, "acme/react");
        let copied = fs::read_to_string(app_repo.join(".wax/registries/acme/react.json"))
            .expect("read copied registry");
        assert!(copied.contains("Card"));
    }

    #[test]
    fn sync_switches_app_registry_source_to_published_source() {
        let _guard = env_lock();
        let root = TestDir::new("published-source");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let (ds_repo, state_path) = setup_remembered_local_ds(&root.path);
        let published_registry = ds_repo.join("published-react.registry.json");
        fs::write(
            &published_registry,
            r#"{"schema_version":1,"components":[{"name":"PublishedButton"}]}"#,
        )
        .expect("write published registry");
        let published_source = format!("file://{}", published_registry.display());
        fs::write(
            ds_repo.join(".wax/wax.config.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "design_systems": {{
    "acme": {{
      "name": "Acme Design System",
      "registries": {{
        "react": {{
          "source": ".wax/registries/react.json",
          "published_source": "{published_source}"
        }}
      }}
    }}
  }}
}}
"#
            ),
        )
        .expect("write ds config with published source");

        let updates = sync_app_registries(&SyncOptions {
            repo_root: app_repo.clone(),
            state_path,
        })
        .expect("sync app registries");

        assert_eq!(updates[0].source, published_source);
        let app_config =
            fs::read_to_string(app_repo.join(".wax/wax.config.json")).expect("read app config");
        assert!(app_config.contains(&published_source));
    }

    #[test]
    fn sync_fails_when_upstream_design_system_is_not_remembered() {
        let _guard = env_lock();
        let root = TestDir::new("missing-memory");
        let app_repo = root.path.join("app");
        write_app_repo(&app_repo, "acme/react", ".wax/registries/acme/react.json");
        let wax_home = root.path.join("wax-home");
        fs::create_dir_all(&wax_home).expect("create wax home");
        let state_path = wax_home.join("state.json");
        fs::write(
            &state_path,
            r#"{"installed_languages":{},"design_systems":{}}"#,
        )
        .expect("write empty state");

        let error = sync_app_registries(&SyncOptions {
            repo_root: app_repo,
            state_path,
        })
        .expect_err("sync should fail for missing remembered design system");

        assert!(error.to_string().contains("acme"));
    }
}
