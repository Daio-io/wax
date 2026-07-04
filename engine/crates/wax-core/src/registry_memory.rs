//! Remembered design-system locations in global wax state.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use thiserror::Error;
use wax_contract::LanguageId;

use crate::config::repo_files::PREFERRED_CONFIG_RELATIVE_PATH;
use crate::global_state::{
    GlobalStateError, RememberedDesignSystem, load_global_state, save_global_state,
};
use crate::registry_source::{
    RegistrySourceError, is_external_registry_source, reject_repo_relative_registry_path_escape,
    validate_repo_relative_registry_path_within_repo,
};

/// Repo-relative path recorded for remembered design systems.
pub const LAST_SEEN_CONFIG_RELATIVE_PATH: &str = PREFERRED_CONFIG_RELATIVE_PATH;

/// Relative registry path for a design-system-authored language registry.
pub fn design_system_registry_relative_path(language_id: &LanguageId) -> String {
    format!(".wax/registries/{}.json", language_id.as_str())
}

/// App-local registry path when copying from a remembered design system.
pub fn app_registry_relative_path(design_system_id: &str, language_id: &LanguageId) -> String {
    format!(
        ".wax/registries/{}/{}.json",
        design_system_id,
        language_id.as_str()
    )
}

/// Resolved registry inputs from a remembered design system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRememberedRegistry {
    /// Source written to app config `registry.source`.
    pub config_source: String,
    /// Upstream id in `<design-system-id>/<language-id>` form.
    pub upstream: String,
    /// Local registry path relative to the design-system repo when copying is required.
    pub design_system_local_source: Option<String>,
}

/// Errors returned while reading or updating remembered design systems.
#[derive(Debug, Error)]
pub enum RegistryMemoryError {
    /// Global wax state could not be loaded or saved.
    #[error(transparent)]
    GlobalState(#[from] GlobalStateError),
    /// The design-system id is not a valid lowercase ASCII slug.
    #[error(
        "design system id `{design_system_id}` is not valid; expected lowercase ASCII slug [a-z][a-z0-9-]*"
    )]
    InvalidDesignSystemId {
        /// Invalid design-system id string.
        design_system_id: String,
    },
    /// The remembered design system does not exist in global state.
    #[error("design system `{design_system_id}` is not remembered")]
    NotFound {
        /// Requested design-system id.
        design_system_id: String,
    },
    /// The repository root could not be canonicalized.
    #[error("failed to resolve repository root {path}: {source}")]
    ResolveRepoRoot {
        /// Repository root passed by the caller.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Wax config could not be read or updated.
    #[error("failed to update wax config at {path}: {source}")]
    ConfigUpdate {
        /// Config path that failed to update.
        path: String,
        /// Underlying I/O or parse failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Remembered registry source path is invalid or escapes the design-system repo.
    #[error(transparent)]
    RegistrySource(#[from] RegistrySourceError),
}

/// Summary of one remembered design system for list/show output.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RememberedDesignSystemSummary {
    /// Design-system id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Canonical repository root.
    pub repo_root: PathBuf,
    /// Repo-relative path to the last seen config file.
    pub last_seen_config: PathBuf,
}

/// Stores or refreshes a remembered design system in global state.
pub fn remember_design_system(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
    name: &str,
    repo_root: impl AsRef<Path>,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let repo_root = canonicalize_repo_root(repo_root.as_ref())?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    state.design_systems.insert(
        design_system_id.to_owned(),
        RememberedDesignSystem {
            name: name.to_owned(),
            repo_root,
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
        },
    );
    save_global_state(state_path, &state)?;
    Ok(())
}

/// Lists remembered design systems sorted by id.
pub fn list_remembered_design_systems(
    state_path: impl AsRef<Path>,
) -> Result<Vec<RememberedDesignSystemSummary>, RegistryMemoryError> {
    let state = load_global_state(state_path.as_ref())?;
    Ok(state
        .design_systems
        .iter()
        .map(|(id, entry)| RememberedDesignSystemSummary {
            id: id.clone(),
            name: entry.name.clone(),
            repo_root: entry.repo_root.clone(),
            last_seen_config: entry.last_seen_config.clone(),
        })
        .collect())
}

/// Returns one remembered design system by id.
pub fn show_remembered_design_system(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
) -> Result<RememberedDesignSystemSummary, RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state = load_global_state(state_path.as_ref())?;
    let Some(entry) = state.design_systems.get(design_system_id) else {
        return Err(RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        });
    };
    Ok(RememberedDesignSystemSummary {
        id: design_system_id.to_owned(),
        name: entry.name.clone(),
        repo_root: entry.repo_root.clone(),
        last_seen_config: entry.last_seen_config.clone(),
    })
}

/// Updates the remembered repository root for a design system.
pub fn update_remembered_design_system_repo_root(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
    repo_root: impl AsRef<Path>,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let repo_root = canonicalize_repo_root(repo_root.as_ref())?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    let Some(entry) = state.design_systems.get_mut(design_system_id) else {
        return Err(RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        });
    };
    entry.repo_root = repo_root;
    save_global_state(state_path, &state)?;
    Ok(())
}

/// Removes a remembered design system from global state.
pub fn delete_remembered_design_system(
    state_path: impl AsRef<Path>,
    design_system_id: &str,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let state_path = state_path.as_ref();
    let mut state = load_global_state(state_path)?;
    if state.design_systems.remove(design_system_id).is_none() {
        return Err(RegistryMemoryError::NotFound {
            design_system_id: design_system_id.to_owned(),
        });
    };
    save_global_state(state_path, &state)?;
    Ok(())
}

/// Resolves registry source and upstream metadata for one remembered design-system language.
pub fn resolve_remembered_registry(
    remembered: &RememberedDesignSystemSummary,
    language_id: &LanguageId,
) -> Result<ResolvedRememberedRegistry, RegistryMemoryError> {
    validate_design_system_id(&remembered.id)?;
    let config_path = remembered.repo_root.join(&remembered.last_seen_config);
    let path_display = config_path.display().to_string();
    let config = load_or_create_config(&config_path, &path_display)?;
    let design_systems = config
        .get("design_systems")
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "wax config missing design_systems object",
            )),
        })?;
    let entry =
        design_systems
            .get(&remembered.id)
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("design system `{}` missing from wax config", remembered.id),
                )),
            })?;
    let registries = entry
        .get("registries")
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "design system `{}` missing registries object",
                    remembered.id
                ),
            )),
        })?;
    let registry = registries
        .get(language_id.as_str())
        .and_then(Value::as_object)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "design system `{}` missing registry for language `{}`",
                    remembered.id,
                    language_id.as_str()
                ),
            )),
        })?;
    let local_source = registry
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "design system `{}` registry for `{}` missing source",
                    remembered.id,
                    language_id.as_str()
                ),
            )),
        })?
        .to_owned();
    let published_source = registry
        .get("published_source")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let upstream = format!("{}/{}", remembered.id, language_id.as_str());
    if let Some(published_source) = published_source.filter(|value| !value.trim().is_empty()) {
        Ok(ResolvedRememberedRegistry {
            config_source: published_source,
            upstream,
            design_system_local_source: None,
        })
    } else if is_external_registry_source(&local_source) {
        Ok(ResolvedRememberedRegistry {
            config_source: local_source,
            upstream,
            design_system_local_source: None,
        })
    } else {
        reject_repo_relative_registry_path_escape(&local_source)?;
        Ok(ResolvedRememberedRegistry {
            config_source: app_registry_relative_path(&remembered.id, language_id),
            upstream,
            design_system_local_source: Some(local_source),
        })
    }
}

/// Copies a design-system registry artifact into an app repository.
pub fn copy_design_system_registry_to_app(
    remembered: &RememberedDesignSystemSummary,
    design_system_local_source: &str,
    app_repo_root: &Path,
    app_registry_relative: &str,
) -> Result<(), RegistryMemoryError> {
    let source_path = validate_repo_relative_registry_path_within_repo(
        &remembered.repo_root,
        design_system_local_source,
    )?;
    let destination_path = app_repo_root.join(app_registry_relative);
    let destination_display = destination_path.display().to_string();
    let contents =
        fs::read_to_string(&source_path).map_err(|source| RegistryMemoryError::ConfigUpdate {
            path: source_path.display().to_string(),
            source: Box::new(source),
        })?;
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent).map_err(|source| RegistryMemoryError::ConfigUpdate {
            path: destination_display.clone(),
            source: Box::new(source),
        })?;
    }
    fs::write(&destination_path, format!("{contents}\n")).map_err(|source| {
        RegistryMemoryError::ConfigUpdate {
            path: destination_display,
            source: Box::new(source),
        }
    })?;
    Ok(())
}

/// Ensures a design-system registry source exists in `.wax/wax.config.json`.
///
/// Creates the config file when missing and preserves unrelated fields.
pub fn ensure_design_system_registry_source(
    repo_root: impl AsRef<Path>,
    design_system_id: &str,
    design_system_name: &str,
    language_id: &LanguageId,
    registry_source: &str,
) -> Result<(), RegistryMemoryError> {
    validate_design_system_id(design_system_id)?;
    let repo_root = repo_root.as_ref();
    let config_path = repo_root.join(LAST_SEEN_CONFIG_RELATIVE_PATH);
    let path_display = config_path.display().to_string();

    let mut config = load_or_create_config(&config_path, &path_display)?;
    let root_object = config
        .as_object_mut()
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "wax config root must be an object",
            )),
        })?;
    if !root_object.contains_key("schema_version") {
        root_object.insert("schema_version".to_owned(), Value::Number(2.into()));
    }
    let design_systems = root_object
        .entry("design_systems".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let design_systems =
        design_systems
            .as_object_mut()
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "wax config design_systems must be an object",
                )),
            })?;

    let entry = design_systems
        .entry(design_system_id.to_owned())
        .or_insert_with(|| {
            Value::Object(Map::from_iter([
                (
                    "name".to_owned(),
                    Value::String(design_system_name.to_owned()),
                ),
                ("registries".to_owned(), Value::Object(Map::new())),
            ]))
        });
    let entry_object = entry
        .as_object_mut()
        .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
            path: path_display.clone(),
            source: Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "design_systems entry is not an object",
            )),
        })?;
    entry_object.insert(
        "name".to_owned(),
        Value::String(design_system_name.to_owned()),
    );

    let registries = entry_object
        .entry("registries".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let registries_object =
        registries
            .as_object_mut()
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "design_systems registries is not an object",
                )),
            })?;
    let language_key = language_id.as_str().to_owned();
    let registry_entry = registries_object
        .entry(language_key)
        .or_insert_with(|| Value::Object(Map::new()));
    let registry_object =
        registry_entry
            .as_object_mut()
            .ok_or_else(|| RegistryMemoryError::ConfigUpdate {
                path: path_display.clone(),
                source: Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "design_systems registry entry is not an object",
                )),
            })?;
    registry_object.insert(
        "source".to_owned(),
        Value::String(registry_source.to_owned()),
    );

    write_config(&config_path, &path_display, &config)?;
    Ok(())
}

fn validate_design_system_id(design_system_id: &str) -> Result<(), RegistryMemoryError> {
    if LanguageId::try_from(design_system_id).is_err() {
        return Err(RegistryMemoryError::InvalidDesignSystemId {
            design_system_id: design_system_id.to_owned(),
        });
    }
    Ok(())
}

fn canonicalize_repo_root(repo_root: &Path) -> Result<PathBuf, RegistryMemoryError> {
    fs::canonicalize(repo_root).map_err(|source| RegistryMemoryError::ResolveRepoRoot {
        path: repo_root.display().to_string(),
        source,
    })
}

fn load_or_create_config(
    config_path: &Path,
    path_display: &str,
) -> Result<Value, RegistryMemoryError> {
    match fs::read_to_string(config_path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|source| RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Ok(Value::Object(Map::from_iter([
                ("schema_version".to_owned(), Value::Number(2.into())),
                ("design_systems".to_owned(), Value::Object(Map::new())),
            ])))
        }
        Err(source) => Err(RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }),
    }
}

fn write_config(
    config_path: &Path,
    path_display: &str,
    config: &Value,
) -> Result<(), RegistryMemoryError> {
    if let Some(parent) = config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        })?;
    }

    let serialized = serde_json::to_string_pretty(config).map_err(|source| {
        RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }
    })?;
    fs::write(config_path, format!("{serialized}\n")).map_err(|source| {
        RegistryMemoryError::ConfigUpdate {
            path: path_display.to_owned(),
            source: Box::new(source),
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod registry_memory_tests {
    use super::*;
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
            let path = std::env::temp_dir().join(format!(
                "wax-core-registry-memory-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn remember_list_show_update_delete_roundtrip() {
        let _guard = env_lock();
        let dir = TestDir::new("roundtrip");
        let state_path = dir.path.join("state.json");
        let repo = dir.path.join("acme-ds");
        std::fs::create_dir_all(&repo).unwrap();

        remember_design_system(&state_path, "acme", "Acme Design System", &repo).unwrap();

        let listed = list_remembered_design_systems(&state_path).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "acme");
        assert_eq!(listed[0].name, "Acme Design System");
        assert_eq!(
            listed[0].last_seen_config,
            PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH)
        );

        let shown = show_remembered_design_system(&state_path, "acme").unwrap();
        assert_eq!(shown.name, "Acme Design System");
        assert!(shown.repo_root.ends_with("acme-ds"));

        let updated_repo = dir.path.join("acme-ds-moved");
        std::fs::create_dir_all(&updated_repo).unwrap();
        update_remembered_design_system_repo_root(&state_path, "acme", &updated_repo).unwrap();
        let updated = show_remembered_design_system(&state_path, "acme").unwrap();
        assert!(updated.repo_root.ends_with("acme-ds-moved"));

        delete_remembered_design_system(&state_path, "acme").unwrap();
        assert!(
            list_remembered_design_systems(&state_path)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_design_system_ids() {
        let _guard = env_lock();
        let dir = TestDir::new("invalid-id");
        let state_path = dir.path.join("state.json");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let err = remember_design_system(&state_path, "Acme", "Acme", &repo).unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::InvalidDesignSystemId { .. }
        ));
    }

    #[test]
    fn show_and_delete_report_missing_entries() {
        let _guard = env_lock();
        let dir = TestDir::new("missing");
        let state_path = dir.path.join("state.json");

        let err = show_remembered_design_system(&state_path, "acme").unwrap_err();
        assert!(matches!(err, RegistryMemoryError::NotFound { .. }));

        let err = delete_remembered_design_system(&state_path, "acme").unwrap_err();
        assert!(matches!(err, RegistryMemoryError::NotFound { .. }));
    }

    #[test]
    fn ensure_design_system_registry_source_creates_config() {
        let _guard = env_lock();
        let dir = TestDir::new("config-create");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let language_id = LanguageId::try_from("react").unwrap();

        ensure_design_system_registry_source(
            &repo,
            "acme",
            "Acme Design System",
            &language_id,
            ".wax/registries/react.json",
        )
        .unwrap();

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(config["schema_version"], 2);
        assert_eq!(
            config["design_systems"]["acme"]["name"],
            "Acme Design System"
        );
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["source"],
            ".wax/registries/react.json"
        );
    }

    #[test]
    fn ensure_design_system_registry_source_merges_existing_config() {
        let _guard = env_lock();
        let dir = TestDir::new("config-merge");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "engine": { "scan_concurrency": 4 },
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "compose": {
          "source": ".wax/registries/compose.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
        let react_id = LanguageId::try_from("react").unwrap();

        ensure_design_system_registry_source(
            &repo,
            "acme",
            "Acme Design System",
            &react_id,
            ".wax/registries/react.json",
        )
        .unwrap();

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(config["engine"]["scan_concurrency"], 4);
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["compose"]["source"],
            ".wax/registries/compose.json"
        );
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["source"],
            ".wax/registries/react.json"
        );
    }

    #[test]
    fn ensure_design_system_registry_source_preserves_published_source() {
        let _guard = env_lock();
        let dir = TestDir::new("preserve-published-source");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react-old.json",
          "published_source": "https://cdn.example.com/acme/react.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
        let react_id = LanguageId::try_from("react").unwrap();

        ensure_design_system_registry_source(
            &repo,
            "acme",
            "Acme Design System",
            &react_id,
            ".wax/registries/react.json",
        )
        .unwrap();

        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["source"],
            ".wax/registries/react.json"
        );
        assert_eq!(
            config["design_systems"]["acme"]["registries"]["react"]["published_source"],
            "https://cdn.example.com/acme/react.registry.json"
        );
    }

    #[test]
    fn design_system_registry_relative_path_matches_spec() {
        let language_id = LanguageId::try_from("react").unwrap();
        assert_eq!(
            design_system_registry_relative_path(&language_id),
            ".wax/registries/react.json"
        );
    }

    #[test]
    fn resolve_remembered_registry_uses_external_source_directly() {
        let _guard = env_lock();
        let dir = TestDir::new("remembered-external-source");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": "file:///tmp/acme-react.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();

        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme Design System".to_owned(),
            repo_root: repo.clone(),
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
        };
        let language_id = LanguageId::try_from("react").unwrap();

        let resolved = resolve_remembered_registry(&remembered, &language_id).unwrap();
        assert_eq!(
            resolved.config_source,
            "file:///tmp/acme-react.registry.json"
        );
        assert_eq!(resolved.upstream, "acme/react");
        assert!(resolved.design_system_local_source.is_none());
    }

    #[test]
    fn resolve_remembered_registry_rejects_path_that_escapes_design_system_repo() {
        let _guard = env_lock();
        let dir = TestDir::new("remembered-path-escape");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(repo.join(".wax")).unwrap();
        std::fs::write(
            repo.join(".wax/wax.config.json"),
            r#"{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": "../outside.registry.json"
        }
      }
    }
  }
}
"#,
        )
        .unwrap();

        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme Design System".to_owned(),
            repo_root: repo.clone(),
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
        };
        let language_id = LanguageId::try_from("react").unwrap();

        let err = resolve_remembered_registry(&remembered, &language_id).unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::RegistrySource(RegistrySourceError::PathEscapesRepo { .. })
        ));
    }

    #[test]
    fn copy_design_system_registry_rejects_path_that_escapes_design_system_repo() {
        let _guard = env_lock();
        let dir = TestDir::new("copy-path-escape");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let outside = dir.path.join("outside.registry.json");
        std::fs::write(&outside, r#"{"schema_version":1,"components":[]}"#).unwrap();

        let remembered = RememberedDesignSystemSummary {
            id: "acme".to_owned(),
            name: "Acme Design System".to_owned(),
            repo_root: repo.clone(),
            last_seen_config: PathBuf::from(LAST_SEEN_CONFIG_RELATIVE_PATH),
        };
        let language_id = LanguageId::try_from("react").unwrap();

        let err = copy_design_system_registry_to_app(
            &remembered,
            "../outside.registry.json",
            dir.path.join("app").as_path(),
            &app_registry_relative_path("acme", &language_id),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            RegistryMemoryError::RegistrySource(RegistrySourceError::PathEscapesRepo { .. })
        ));
    }

    #[test]
    fn remembered_entries_do_not_copy_registry_json() {
        let _guard = env_lock();
        let dir = TestDir::new("no-registry-copy");
        let state_path = dir.path.join("state.json");
        let repo = dir.path.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        remember_design_system(&state_path, "acme", "Acme", &repo).unwrap();

        let raw = std::fs::read_to_string(&state_path).unwrap();
        assert!(!raw.contains("components"));
        assert!(!raw.contains("registries/react.json"));

        let state = load_global_state(&state_path).unwrap();
        assert_eq!(state.design_systems.len(), 1);
        assert!(state.installed_languages.is_empty());
        assert!(state.design_systems.contains_key("acme"));
    }
}
