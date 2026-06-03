//! `wax init` repository onboarding.

use super::language::{
    LanguageCommandError, default_target_triple, install_pinned_manifest, manifest_for_language,
    resolve_registry_url, save_lockfile, update_lockfile_entry,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use wax_contract::LanguageId;
use wax_core::config::lockfile::{LockedRegistry, WAX_LOCK_SCHEMA_VERSION, WaxLock};
use wax_core::config::repo_files::{
    DEFAULT_REGISTRY_RELATIVE_PATH, PREFERRED_CONFIG_RELATIVE_PATH,
    PREFERRED_LOCKFILE_RELATIVE_PATH,
};
use wax_core::config::waxrc::WAXRC_SCHEMA_VERSION;
use wax_core::registry::{
    RegistryArtifact, RegistryError, RegistryManifest, fetch_pack_index, select_target_artifact,
};
use wax_core::registry_source::{
    RegistrySourceError, RegistrySourceInput, resolve_registry_source,
};
use wax_lang_api::build_version;

const EXAMPLE_WAXRC: &str = include_str!("../../../../fixtures/config/example.waxrc");
const EXAMPLE_DESIGN_SYSTEM_REGISTRY: &str =
    include_str!("../../../../fixtures/config/example-registry.json");

/// Engine API version recorded in new `wax.lock.json` files.
const ENGINE_API_VERSION: u32 = 1;

/// Resolved pack index metadata used for lockfile pinning and install.
struct ResolvedInitLanguage {
    manifest: RegistryManifest,
    artifact: RegistryArtifact,
}

/// Options for `wax init`.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Run scriptable onboarding without prompts.
    pub non_interactive: bool,
    /// Language pack ids to enable in the new repository configuration.
    pub languages: Vec<LanguageId>,
    /// Write config and lockfile without downloading language packs.
    pub no_install: bool,
    /// Pack index URL. Resolution precedence: `--registry` > `WAX_LANG_INDEX` > built-in default.
    pub registry_url: Option<String>,
    /// Repository root that will receive `.wax/wax.config.json` and `.wax/wax.lock.json`.
    pub repo_root: PathBuf,
    /// Target triple override for tests.
    pub target_triple: Option<String>,
    /// Global state path override for tests.
    pub state_path: Option<PathBuf>,
    /// Copy example design-system registry files when paths are missing.
    pub scaffold_registries: bool,
}

/// Errors returned by `wax init`.
#[derive(Debug, Error)]
pub enum InitCommandError {
    /// Scriptable init requires `--non-interactive`.
    #[error(
        "wax init requires --non-interactive for scriptable onboarding; interactive prompts are not implemented yet"
    )]
    RequiresNonInteractiveFlag,
    /// No language ids were provided for onboarding.
    #[error("wax init requires at least one --language <id>")]
    MissingLanguageSelection,
    /// Repository configuration already exists.
    #[error("wax config already exists at {path}; remove it or run init in a fresh directory")]
    WaxConfigAlreadyExists {
        /// Existing configuration path.
        path: PathBuf,
    },
    /// Example onboarding template is malformed.
    #[error("invalid example .waxrc template: {source}")]
    InvalidExampleWaxRc {
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Example onboarding template is missing a `languages` array.
    #[error("example .waxrc template is missing languages array")]
    MissingExampleLanguages,
    /// Requested language id is not present in the example template.
    #[error("language {language_id} is not supported by the example .waxrc template")]
    UnknownExampleLanguage {
        /// Requested language id.
        language_id: LanguageId,
    },
    /// Language lifecycle command failed during init.
    #[error(transparent)]
    Language(#[from] LanguageCommandError),
    /// Pack index fetch or resolution failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// Registry source resolution failed while creating initial registry locks.
    #[error(transparent)]
    RegistrySource(#[from] RegistrySourceError),
    /// Filesystem operation failed.
    #[error("{context}: {source}")]
    Io {
        /// Human-readable context.
        context: String,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Runs `wax init`.
pub fn run_init(options: InitOptions, writer: &mut impl Write) -> Result<(), InitCommandError> {
    if !options.non_interactive {
        return Err(InitCommandError::RequiresNonInteractiveFlag);
    }

    let languages = dedupe_languages(&options.languages);
    if languages.is_empty() {
        return Err(InitCommandError::MissingLanguageSelection);
    }

    let wax_dir = options.repo_root.join(".wax");
    let config_path = options.repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    let lockfile_path = options.repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH);
    if config_path.exists() {
        return Err(InitCommandError::WaxConfigAlreadyExists { path: config_path });
    }

    let waxrc_contents = build_waxrc_contents(&languages)?;
    let registry_url = resolve_registry_url(options.registry_url)?;
    let manifests = fetch_pack_index(&registry_url)?;
    let target = options
        .target_triple
        .clone()
        .unwrap_or_else(default_target_triple);

    let mut resolved_languages = Vec::with_capacity(languages.len());
    for language_id in &languages {
        let manifest = manifest_for_language(&manifests, language_id, None)?;
        let artifact = select_target_artifact(&manifest, &target)?.clone();
        resolved_languages.push(ResolvedInitLanguage { manifest, artifact });
    }

    fs::create_dir_all(&wax_dir).map_err(|source| InitCommandError::Io {
        context: format!("create {}", wax_dir.display()),
        source,
    })?;

    write_file_atomically(&config_path, &waxrc_contents)?;

    if options.scaffold_registries {
        write_file_atomically(
            &options.repo_root.join(DEFAULT_REGISTRY_RELATIVE_PATH),
            EXAMPLE_DESIGN_SYSTEM_REGISTRY,
        )?;
    }

    let mut lockfile = WaxLock {
        schema_version: WAX_LOCK_SCHEMA_VERSION,
        engine_api_version: ENGINE_API_VERSION,
        wax_version: build_version().to_owned(),
        locked_at: None,
        registries: BTreeMap::new(),
        languages: BTreeMap::new(),
    };
    for resolved in &resolved_languages {
        update_lockfile_entry(
            &mut lockfile,
            &resolved.manifest,
            &registry_url,
            &target,
            &resolved.artifact,
        );
        let registry_source = resolve_registry_source(RegistrySourceInput {
            repo_root: &options.repo_root,
            language_id: resolved.manifest.id.as_str(),
            source: None,
        })?;
        lockfile.registries.insert(
            resolved.manifest.id.clone(),
            LockedRegistry {
                source: registry_source.source,
                sha256: registry_source.sha256,
            },
        );
    }

    save_lockfile(&lockfile_path, &lockfile)?;
    update_gitignore(&options.repo_root)?;

    if !options.no_install {
        for resolved in &resolved_languages {
            install_pinned_manifest(
                &resolved.manifest,
                &target,
                &resolved.artifact,
                options.state_path.clone(),
                writer,
            )?;
        }
    }

    writeln!(writer, "initialized wax in {}", options.repo_root.display()).map_err(|source| {
        InitCommandError::Io {
            context: "write init output".to_owned(),
            source,
        }
    })?;
    Ok(())
}

fn dedupe_languages(languages: &[LanguageId]) -> Vec<LanguageId> {
    let mut deduped = Vec::with_capacity(languages.len());
    for language_id in languages {
        if !deduped.iter().any(|seen| seen == language_id) {
            deduped.push(language_id.clone());
        }
    }
    deduped
}

fn build_waxrc_contents(selected: &[LanguageId]) -> Result<String, InitCommandError> {
    let mut template: serde_json::Value = serde_json::from_str(EXAMPLE_WAXRC)
        .map_err(|source| InitCommandError::InvalidExampleWaxRc { source })?;
    let Some(languages) = template
        .get_mut("languages")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Err(InitCommandError::MissingExampleLanguages);
    };

    let selected: BTreeMap<_, _> = selected.iter().map(|id| (id.as_str(), id)).collect();
    let mut filtered = Vec::new();
    for entry in languages.drain(..) {
        let Some(id) = entry.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if selected.contains_key(id) {
            filtered.push(entry);
        }
    }

    if filtered.len() != selected.len() {
        for language_id in selected.values() {
            if !filtered.iter().any(|entry| {
                entry
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id == language_id.as_str())
            }) {
                return Err(InitCommandError::UnknownExampleLanguage {
                    language_id: (*language_id).clone(),
                });
            }
        }
    }

    for entry in &mut filtered {
        if let Some(object) = entry.as_object_mut() {
            object.remove("design_system_registry");
            object.remove("registry");
        }
    }

    *languages = filtered;
    template["schema_version"] = serde_json::json!(WAXRC_SCHEMA_VERSION);

    serde_json::to_string_pretty(&template).map_err(|source| InitCommandError::Io {
        context: "serialize wax config".to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })
}

fn update_gitignore(repo_root: &Path) -> Result<(), InitCommandError> {
    let path = repo_root.join(".gitignore");
    let mut contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(InitCommandError::Io {
                context: format!("read {}", path.display()),
                source,
            });
        }
    };

    for entry in ["/.wax/cache/", "/.wax/out/"] {
        if !contents.lines().any(|line| line.trim() == entry) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(entry);
            contents.push('\n');
        }
    }

    fs::write(&path, contents).map_err(|source| InitCommandError::Io {
        context: format!("write {}", path.display()),
        source,
    })
}

fn write_file_atomically(path: &Path, contents: &str) -> Result<(), InitCommandError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| InitCommandError::Io {
            context: format!("create parent directory for {}", path.display()),
            source,
        })?;
    }
    fs::write(path, format!("{contents}\n")).map_err(|source| InitCommandError::Io {
        context: format!("write {}", path.display()),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::env_lock;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use wax_contract::LanguageId;
    use wax_core::config::lockfile::load_lockfile;
    use wax_core::config::waxrc::load_waxrc;
    use wax_core::global_state::{GlobalState, load_global_state};

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value.as_ref());
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("wax-init-{name}-{}", std::process::id()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn lang(id: &str) -> LanguageId {
        LanguageId::try_from(id).expect("language id")
    }

    fn file_url(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    fn write_default_registry(repo_root: &Path) {
        let registry_path = repo_root.join(DEFAULT_REGISTRY_RELATIVE_PATH);
        if let Some(parent) = registry_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            &registry_path,
            format!("{EXAMPLE_DESIGN_SYSTEM_REGISTRY}\n"),
        )
        .unwrap();
    }

    fn write_pack_artifact(path: &Path, binary: &str) -> String {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use sha2::{Digest, Sha256};

        let mut bytes = Vec::new();
        {
            let gz = GzEncoder::new(&mut bytes, Compression::default());
            let mut tar = tar::Builder::new(gz);
            let body = b"#!/bin/sh\nexit 0\n";
            let mut header = tar::Header::new_gnu();
            header.set_path(binary).unwrap();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append(&header, body.as_slice()).unwrap();
            tar.finish().unwrap();
        }
        fs::write(path, &bytes).unwrap();
        Sha256::digest(&bytes)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }

    #[test]
    fn requires_non_interactive_flag_for_scriptable_init() {
        let temp = TestDir::new("requires-non-interactive");
        let err = run_init(
            InitOptions {
                non_interactive: false,
                languages: vec![lang("compose")],
                no_install: true,
                registry_url: Some("file:///tmp/registry.json".to_owned()),
                repo_root: temp.path.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: None,
                scaffold_registries: false,
            },
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(matches!(err, InitCommandError::RequiresNonInteractiveFlag));
    }

    #[test]
    fn init_writes_waxrc_lockfile_and_installs_selected_language() {
        let _guard = env_lock();
        let temp = TestDir::new("happy-path");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path.join("home"));

        let artifact_path = temp.path.join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path.join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        let repo_root = temp.path.join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let mut output = Vec::new();
        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: false,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: true,
            },
            &mut output,
        )
        .unwrap();

        let waxrc = load_waxrc(repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH)).unwrap();
        assert_eq!(waxrc.languages.len(), 1);
        assert_eq!(waxrc.languages[0].id, lang("compose"));
        assert!(waxrc.languages[0].enabled);
        assert!(waxrc.languages[0].registry_source().is_none());

        let lock = load_lockfile(repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        let compose = lock.languages.get(&lang("compose")).unwrap();
        assert_eq!(compose.version, "0.4.2");
        assert_eq!(compose.resolved.target, "test-target");
        let registry = lock.registries.get(&lang("compose")).unwrap();
        assert_eq!(registry.source, DEFAULT_REGISTRY_RELATIVE_PATH);

        assert!(repo_root.join(DEFAULT_REGISTRY_RELATIVE_PATH).is_file());

        let state = load_global_state(temp.path.join("home/state.json")).unwrap();
        assert!(state.installed_languages[&lang("compose")].contains_key("0.4.2"));

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("initialized wax"));
        assert!(output.contains("installed compose 0.4.2"));
    }

    #[test]
    fn init_no_install_skips_global_install() {
        let _guard = env_lock();
        let temp = TestDir::new("no-install");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path.join("home"));

        let artifact_path = temp.path.join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path.join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        let repo_root = temp.path.join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        write_default_registry(&repo_root);

        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: false,
            },
            &mut Vec::new(),
        )
        .unwrap();

        let state =
            load_global_state(temp.path.join("home/state.json")).unwrap_or_else(|_| GlobalState {
                installed_languages: BTreeMap::new(),
            });
        assert!(state.installed_languages.is_empty());
        assert!(repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH).is_file());
    }

    #[test]
    fn init_reuses_already_installed_language_for_second_repo() {
        let _guard = env_lock();
        let temp = TestDir::new("second-repo");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path.join("home"));

        let artifact_path = temp.path.join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path.join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        let first_repo = temp.path.join("first-repo");
        let second_repo = temp.path.join("second-repo");
        fs::create_dir_all(&first_repo).unwrap();
        fs::create_dir_all(&second_repo).unwrap();
        write_default_registry(&first_repo);
        write_default_registry(&second_repo);
        let state_path = temp.path.join("home/state.json");

        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: false,
                registry_url: Some(file_url(&registry_path)),
                repo_root: first_repo.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(state_path.clone()),
                scaffold_registries: false,
            },
            &mut Vec::new(),
        )
        .unwrap();

        let mut output = Vec::new();
        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: false,
                registry_url: Some(file_url(&registry_path)),
                repo_root: second_repo.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(state_path.clone()),
                scaffold_registries: false,
            },
            &mut output,
        )
        .unwrap();

        assert!(first_repo.join(PREFERRED_CONFIG_RELATIVE_PATH).is_file());
        assert!(second_repo.join(PREFERRED_CONFIG_RELATIVE_PATH).is_file());
        assert!(second_repo.join(PREFERRED_LOCKFILE_RELATIVE_PATH).is_file());
        let state = load_global_state(state_path).unwrap();
        assert!(state.installed_languages[&lang("compose")].contains_key("0.4.2"));
        let first_lock = load_lockfile(first_repo.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        let second_lock =
            load_lockfile(second_repo.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        assert_eq!(
            first_lock.languages[&lang("compose")],
            second_lock.languages[&lang("compose")]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("initialized wax"));
        assert!(output.contains("installed compose 0.4.2"));
    }

    #[test]
    fn init_refuses_existing_wax_config() {
        let temp = TestDir::new("existing");
        let repo_root = temp.path.join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        let config_path = repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, "{}\n").unwrap();

        let err = run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: true,
                registry_url: Some("file:///tmp/registry.json".to_owned()),
                repo_root,
                target_triple: Some("test-target".to_owned()),
                state_path: None,
                scaffold_registries: false,
            },
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            InitCommandError::WaxConfigAlreadyExists { .. }
        ));
    }

    #[test]
    fn init_leaves_repo_clean_when_registry_resolution_fails() {
        let temp = TestDir::new("registry-failure");
        let registry_path = temp.path.join("registry.json");
        fs::write(
            &registry_path,
            r#"[{"id":"react","version":"1.0.0","api_version":1,"targets":{"test-target":{"url":"file:///tmp/react.tgz","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}}]"#,
        )
        .unwrap();

        let repo_root = temp.path.join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let err = run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: None,
                scaffold_registries: false,
            },
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            InitCommandError::Language(LanguageCommandError::LanguageNotFound { .. })
        ));
        assert!(!repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH).exists());
        assert!(!repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH).exists());
        assert!(!repo_root.join(DEFAULT_REGISTRY_RELATIVE_PATH).exists());
    }

    #[test]
    fn init_dedupes_duplicate_language_flags() {
        let _guard = env_lock();
        let temp = TestDir::new("dedupe");
        let _wax_home = EnvVarGuard::set("WAX_HOME", temp.path.join("home"));

        let artifact_path = temp.path.join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let registry_path = temp.path.join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest
            ),
        )
        .unwrap();

        let repo_root = temp.path.join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        write_default_registry(&repo_root);

        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose"), lang("compose")],
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: false,
            },
            &mut Vec::new(),
        )
        .unwrap();

        let waxrc = load_waxrc(repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH)).unwrap();
        assert_eq!(waxrc.languages.len(), 1);
        let lock = load_lockfile(repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        assert_eq!(lock.languages.len(), 1);
    }
}
