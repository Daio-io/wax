//! `wax init` repository onboarding.

use super::language::{
    LanguageCommandError, default_target_triple, install_pinned_manifest, manifest_for_language,
    resolve_registry_url, save_lockfile, update_lockfile_entry,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use wax_contract::LanguageId;
use wax_core::config::lockfile::{LockedRegistry, WAX_LOCK_SCHEMA_VERSION, WaxLock};
use wax_core::config::repo_files::{
    PREFERRED_CONFIG_RELATIVE_PATH, PREFERRED_LOCKFILE_RELATIVE_PATH,
    default_registry_path_for_language,
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

struct PendingRegistryScaffold {
    language_id: LanguageId,
    path: PathBuf,
    sha256: String,
    contents: String,
}

/// Answers collected by the interactive init wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitSelections {
    /// Language pack ids selected by the user.
    pub languages: Vec<LanguageId>,
    /// Scan roots to persist in `.wax/wax.config.json`, keyed by language id.
    pub scan_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    /// Registry setup mode selected by the user.
    pub registry_setup: RegistrySetup,
}

/// Registry setup answer collected during interactive init.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // constructed by interactive prompts in Task 3
pub enum RegistrySetup {
    /// Registry definitions are managed outside this repository.
    External,
    /// Registry source is in this repository. Roots are used only for printed follow-up commands.
    InRepository {
        /// Source roots for `wax registry discover`, keyed by language id.
        roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    },
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
    /// Interactive selections. When present, init uses these answers instead of CLI language flags.
    pub interactive: Option<InitSelections>,
}

/// Errors returned by `wax init`.
#[derive(Debug, Error)]
pub enum InitCommandError {
    /// Interactive init requires a terminal unless scriptable flags are used.
    #[error(
        "wax init needs an interactive terminal. For CI or scripts, run: wax init --non-interactive --language <language-id>"
    )]
    RequiresInteractiveTerminal,
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
    let selections = options.interactive.clone();
    if !options.non_interactive && selections.is_none() {
        return Err(InitCommandError::RequiresInteractiveTerminal);
    }

    let languages = match &selections {
        Some(selections) => dedupe_languages(&selections.languages),
        None => dedupe_languages(&options.languages),
    };
    if languages.is_empty() {
        return Err(InitCommandError::MissingLanguageSelection);
    }

    let wax_dir = options.repo_root.join(".wax");
    let config_path = options.repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    let lockfile_path = options.repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH);
    if config_path.exists() {
        return Err(InitCommandError::WaxConfigAlreadyExists { path: config_path });
    }

    let waxrc_contents =
        build_waxrc_contents(&languages, options.scaffold_registries, selections.as_ref())?;
    let registry_url = resolve_registry_url(options.registry_url)?;
    let manifests = fetch_pack_index(&registry_url)?;
    let pending_registry_scaffolds =
        pending_registry_scaffolds(&options.repo_root, &languages, options.scaffold_registries);
    let scaffold_by_language = pending_registry_scaffolds
        .iter()
        .map(|scaffold| (scaffold.language_id.clone(), scaffold))
        .collect::<BTreeMap<_, _>>();
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
        let registry_source =
            if let Some(scaffold) = scaffold_by_language.get(&resolved.manifest.id) {
                LockedRegistry {
                    source: default_registry_path_for_language(&resolved.manifest.id),
                    sha256: scaffold.sha256.clone(),
                }
            } else {
                let repo_relative = default_registry_path_for_language(&resolved.manifest.id);
                let registry_source = resolve_registry_source(RegistrySourceInput {
                    repo_root: &options.repo_root,
                    language_id: resolved.manifest.id.as_str(),
                    source: Some(&repo_relative),
                })?;
                LockedRegistry {
                    source: registry_source.source,
                    sha256: registry_source.sha256,
                }
            };
        lockfile
            .registries
            .insert(resolved.manifest.id.clone(), registry_source);
    }

    fs::create_dir_all(&wax_dir).map_err(|source| InitCommandError::Io {
        context: format!("create {}", wax_dir.display()),
        source,
    })?;
    write_file_atomically(&config_path, &waxrc_contents)?;
    for scaffold in &pending_registry_scaffolds {
        write_file_atomically(&scaffold.path, &scaffold.contents)?;
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
    write_next_steps(selections.as_ref(), writer)?;
    Ok(())
}

fn write_next_steps(
    selections: Option<&InitSelections>,
    writer: &mut impl Write,
) -> Result<(), InitCommandError> {
    let Some(selections) = selections else {
        return Ok(());
    };

    (|| -> io::Result<()> {
        match &selections.registry_setup {
            RegistrySetup::External => {
                writeln!(writer)?;
                writeln!(writer, "Registry setup:")?;
                for language_id in &selections.languages {
                    writeln!(
                        writer,
                        "- Populate .wax/{}.registry.json or update that language's registry source before scanning.",
                        language_id.as_str()
                    )?;
                }
                writeln!(writer, "Then run `wax scan`.")?;
            }
            RegistrySetup::InRepository { roots } => {
                writeln!(writer)?;
                writeln!(
                    writer,
                    "Next, populate registries from your design-system source:"
                )?;
                for language_id in &selections.languages {
                    if let Some(language_roots) = roots.get(language_id) {
                        if language_roots.is_empty() {
                            writeln!(
                                writer,
                                "No registry source roots were provided for {}; populate .wax/{}.registry.json or rerun wax registry discover with a source root.",
                                language_id.as_str(),
                                language_id.as_str()
                            )?;
                        } else {
                            for root in language_roots {
                                writeln!(
                                    writer,
                                    "wax registry discover --language {} --root {}",
                                    language_id.as_str(),
                                    root.display()
                                )?;
                            }
                        }
                    }
                }
                writeln!(writer, "Then run `wax scan`.")?;
            }
        }

        Ok(())
    })()
    .map_err(|source| InitCommandError::Io {
        context: "write init guidance".to_owned(),
        source,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_roots(input: &str) -> Vec<PathBuf> {
    input
        .split(',')
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[allow(dead_code)] // used by DialoguerInitPrompts; wired in Task 3
fn sorted_language_manifests(manifests: &[RegistryManifest]) -> Vec<&RegistryManifest> {
    let mut sorted = manifests.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| match (left.id.as_str(), right.id.as_str()) {
        ("compose", "compose") => std::cmp::Ordering::Equal,
        ("compose", _) => std::cmp::Ordering::Less,
        (_, "compose") => std::cmp::Ordering::Greater,
        _ => left.id.as_str().cmp(right.id.as_str()),
    });
    sorted
}

trait InitPrompts {
    fn select_languages(
        &mut self,
        manifests: &[RegistryManifest],
    ) -> Result<Vec<LanguageId>, InitCommandError>;

    fn scan_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, InitCommandError>;

    fn registry_in_repo(&mut self) -> Result<bool, InitCommandError>;

    fn registry_roots(
        &mut self,
        language_id: &LanguageId,
    ) -> Result<Vec<PathBuf>, InitCommandError>;
}

#[allow(dead_code)] // wired in Task 3 via run_init_cli
struct DialoguerInitPrompts;

impl InitPrompts for DialoguerInitPrompts {
    fn select_languages(
        &mut self,
        manifests: &[RegistryManifest],
    ) -> Result<Vec<LanguageId>, InitCommandError> {
        use dialoguer::MultiSelect;

        let sorted = sorted_language_manifests(manifests);
        let labels: Vec<String> = sorted
            .iter()
            .map(|manifest| format!("{} ({})", manifest.id.as_str(), manifest.version))
            .collect();
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();

        let selected = MultiSelect::new()
            .with_prompt("Select language packs to enable")
            .items(&label_refs)
            .interact()
            .map_err(|source| InitCommandError::Io {
                context: "select language packs".to_owned(),
                source: io::Error::other(source),
            })?;

        Ok(selected
            .into_iter()
            .map(|index| sorted[index].id.clone())
            .collect())
    }

    fn scan_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, InitCommandError> {
        use dialoguer::Input;

        let input: String = Input::new()
            .with_prompt(format!(
                "Scan roots for {} (comma-separated)",
                language_id.as_str()
            ))
            .interact_text()
            .map_err(|source| InitCommandError::Io {
                context: format!("scan roots for {}", language_id.as_str()),
                source: io::Error::other(source),
            })?;

        Ok(parse_roots(&input))
    }

    fn registry_in_repo(&mut self) -> Result<bool, InitCommandError> {
        use dialoguer::Confirm;

        Confirm::new()
            .with_prompt("Is your design-system registry source in this repository?")
            .default(true)
            .interact()
            .map_err(|source| InitCommandError::Io {
                context: "registry source location".to_owned(),
                source: io::Error::other(source),
            })
    }

    fn registry_roots(
        &mut self,
        language_id: &LanguageId,
    ) -> Result<Vec<PathBuf>, InitCommandError> {
        use dialoguer::Input;

        let input: String = Input::new()
            .with_prompt(format!(
                "Registry source roots for {} (comma-separated)",
                language_id.as_str()
            ))
            .interact_text()
            .map_err(|source| InitCommandError::Io {
                context: format!("registry source roots for {}", language_id.as_str()),
                source: io::Error::other(source),
            })?;

        Ok(parse_roots(&input))
    }
}

#[allow(dead_code)] // wired in Task 3 via run_init_cli
fn collect_interactive_selections(
    manifests: &[RegistryManifest],
    prompts: &mut impl InitPrompts,
) -> Result<InitSelections, InitCommandError> {
    let languages = dedupe_languages(&prompts.select_languages(manifests)?);
    if languages.is_empty() {
        return Err(InitCommandError::MissingLanguageSelection);
    }

    let mut scan_roots = BTreeMap::new();
    for language_id in &languages {
        scan_roots.insert(language_id.clone(), prompts.scan_roots(language_id)?);
    }

    let registry_setup = if prompts.registry_in_repo()? {
        let mut roots = BTreeMap::new();
        for language_id in &languages {
            roots.insert(language_id.clone(), prompts.registry_roots(language_id)?);
        }
        RegistrySetup::InRepository { roots }
    } else {
        RegistrySetup::External
    };

    Ok(InitSelections {
        languages,
        scan_roots,
        registry_setup,
    })
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

fn build_waxrc_contents(
    selected: &[LanguageId],
    scaffold_registries: bool,
    selections: Option<&InitSelections>,
) -> Result<String, InitCommandError> {
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
            if scaffold_registries {
                if let Some(id) = object.get("id").and_then(serde_json::Value::as_str) {
                    let language_id = LanguageId::try_from(id)
                        .expect("example template language ids are validated");
                    object.insert(
                        "registry".to_owned(),
                        serde_json::json!(default_registry_path_for_language(&language_id)),
                    );
                }
            } else {
                object.remove("registry");
            }
            if let Some(selections) = selections
                && let Some(id) = object.get("id").and_then(serde_json::Value::as_str)
            {
                let language_id =
                    LanguageId::try_from(id).expect("example template language ids are validated");
                if let Some(roots) = selections.scan_roots.get(&language_id) {
                    object.insert(
                        "roots".to_owned(),
                        serde_json::Value::Array(
                            roots
                                .iter()
                                .map(|root| {
                                    serde_json::Value::String(root.to_string_lossy().to_string())
                                })
                                .collect(),
                        ),
                    );
                }
            }
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

fn pending_registry_scaffolds(
    repo_root: &Path,
    languages: &[LanguageId],
    scaffold_registries: bool,
) -> Vec<PendingRegistryScaffold> {
    if !scaffold_registries {
        return Vec::new();
    }

    languages
        .iter()
        .filter_map(|language_id| {
            let repo_relative = default_registry_path_for_language(language_id);
            let path = repo_root.join(&repo_relative);
            if path.exists() {
                return None;
            }
            let contents = rendered_file_contents(EXAMPLE_DESIGN_SYSTEM_REGISTRY);
            Some(PendingRegistryScaffold {
                language_id: language_id.clone(),
                path,
                sha256: sha256_hex(contents.as_bytes()),
                contents,
            })
        })
        .collect()
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
    fs::write(path, rendered_file_contents(contents)).map_err(|source| InitCommandError::Io {
        context: format!("write {}", path.display()),
        source,
    })
}

fn rendered_file_contents(contents: &str) -> String {
    if contents.ends_with('\n') {
        contents.to_owned()
    } else {
        format!("{contents}\n")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
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
    use wax_core::config::repo_files::{
        PREFERRED_CONFIG_RELATIVE_PATH, PREFERRED_LOCKFILE_RELATIVE_PATH,
        default_registry_path_for_language,
    };
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

    fn write_default_registry(repo_root: &Path, language_id: &LanguageId) {
        let registry_path = repo_root.join(default_registry_path_for_language(language_id));
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
    fn init_without_interactive_answers_requires_terminal() {
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
                interactive: None,
            },
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(matches!(err, InitCommandError::RequiresInteractiveTerminal));
    }

    #[test]
    fn init_writes_interactive_scan_roots() {
        let temp = TestDir::new("init-interactive-scan-roots");
        let artifact_path = temp.path.join("compose.tgz");
        let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
        let react_artifact_path = temp.path.join("react.tgz");
        let react_digest = write_pack_artifact(&react_artifact_path, "wax-lang-react");
        let registry_path = temp.path.join("registry.json");
        fs::write(
            &registry_path,
            format!(
                r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}},{{"id":"react","version":"0.2.0","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
                file_url(&artifact_path),
                digest,
                file_url(&react_artifact_path),
                react_digest
            ),
        )
        .unwrap();
        let repo_root = temp.path.join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        let mut output = Vec::new();

        run_init(
            InitOptions {
                non_interactive: false,
                languages: Vec::new(),
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: true,
                interactive: Some(InitSelections {
                    languages: vec![
                        LanguageId::try_from("compose").unwrap(),
                        LanguageId::try_from("react").unwrap(),
                    ],
                    scan_roots: BTreeMap::from([
                        (
                            LanguageId::try_from("compose").unwrap(),
                            vec![PathBuf::from("android/app/src/main/kotlin")],
                        ),
                        (
                            LanguageId::try_from("react").unwrap(),
                            vec![
                                PathBuf::from("apps/web/src"),
                                PathBuf::from("packages/ui/src"),
                            ],
                        ),
                    ]),
                    registry_setup: RegistrySetup::External,
                }),
            },
            &mut output,
        )
        .expect("interactive init");

        let config: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(repo_root.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        let languages = config["languages"].as_array().unwrap();
        assert_eq!(languages[0]["id"], "compose");
        assert_eq!(
            languages[0]["roots"],
            serde_json::json!(["android/app/src/main/kotlin"])
        );
        assert_eq!(languages[1]["id"], "react");
        assert_eq!(
            languages[1]["roots"],
            serde_json::json!(["apps/web/src", "packages/ui/src"])
        );
    }

    #[test]
    fn init_does_not_persist_registry_source_roots() {
        let temp = TestDir::new("init-registry-roots-not-persisted");
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

        run_init(
            InitOptions {
                non_interactive: false,
                languages: Vec::new(),
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: true,
                interactive: Some(InitSelections {
                    languages: vec![LanguageId::try_from("compose").unwrap()],
                    scan_roots: BTreeMap::from([(
                        LanguageId::try_from("compose").unwrap(),
                        vec![PathBuf::from("app/src/main/kotlin")],
                    )]),
                    registry_setup: RegistrySetup::InRepository {
                        roots: BTreeMap::from([(
                            LanguageId::try_from("compose").unwrap(),
                            vec![PathBuf::from("design-system/src/main/kotlin")],
                        )]),
                    },
                }),
            },
            &mut Vec::new(),
        )
        .expect("interactive init");

        let config: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(repo_root.join(".wax/wax.config.json")).unwrap(),
        )
        .unwrap();
        let language = &config["languages"][0];
        assert_eq!(
            language["roots"],
            serde_json::json!(["app/src/main/kotlin"])
        );
        let serialized = serde_json::to_string(language).unwrap();
        assert!(
            !serialized.contains("design-system/src/main/kotlin"),
            "registry source roots must only appear in next-step output"
        );
    }

    struct MockInitPrompts {
        languages: Vec<LanguageId>,
        scan_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
        registry_in_repo: bool,
        registry_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    }

    impl InitPrompts for MockInitPrompts {
        fn select_languages(
            &mut self,
            _manifests: &[RegistryManifest],
        ) -> Result<Vec<LanguageId>, InitCommandError> {
            Ok(self.languages.clone())
        }

        fn scan_roots(
            &mut self,
            language_id: &LanguageId,
        ) -> Result<Vec<PathBuf>, InitCommandError> {
            Ok(self
                .scan_roots
                .get(language_id)
                .cloned()
                .unwrap_or_default())
        }

        fn registry_in_repo(&mut self) -> Result<bool, InitCommandError> {
            Ok(self.registry_in_repo)
        }

        fn registry_roots(
            &mut self,
            language_id: &LanguageId,
        ) -> Result<Vec<PathBuf>, InitCommandError> {
            Ok(self
                .registry_roots
                .get(language_id)
                .cloned()
                .unwrap_or_default())
        }
    }

    #[test]
    fn registry_discover_guidance_uses_interactive_roots() {
        let selections = InitSelections {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            scan_roots: BTreeMap::from([(
                LanguageId::try_from("compose").unwrap(),
                vec![PathBuf::from("app/src/main/kotlin")],
            )]),
            registry_setup: RegistrySetup::InRepository {
                roots: BTreeMap::from([(
                    LanguageId::try_from("compose").unwrap(),
                    vec![PathBuf::from("design-system/src/main/kotlin")],
                )]),
            },
        };

        let mut output = Vec::new();
        write_next_steps(Some(&selections), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains(
            "wax registry discover --language compose --root design-system/src/main/kotlin"
        ));
        assert!(output.contains("wax scan"));
    }

    #[test]
    fn external_registry_guidance_explains_registry_setup() {
        let selections = InitSelections {
            languages: vec![LanguageId::try_from("react").unwrap()],
            scan_roots: BTreeMap::from([(
                LanguageId::try_from("react").unwrap(),
                vec![PathBuf::from("apps/web/src")],
            )]),
            registry_setup: RegistrySetup::External,
        };

        let mut output = Vec::new();
        write_next_steps(Some(&selections), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains(".wax/react.registry.json"));
        assert!(output.contains("wax scan"));
        assert!(!output.contains("wax registry discover"));
    }

    #[test]
    fn empty_registry_source_roots_print_guidance_without_root_args() {
        let selections = InitSelections {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            scan_roots: BTreeMap::from([(
                LanguageId::try_from("compose").unwrap(),
                vec![PathBuf::from("app/src/main/kotlin")],
            )]),
            registry_setup: RegistrySetup::InRepository {
                roots: BTreeMap::from([(LanguageId::try_from("compose").unwrap(), Vec::new())]),
            },
        };

        let mut output = Vec::new();
        write_next_steps(Some(&selections), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("No registry source roots were provided for compose"));
        assert!(!output.contains("--root"));
        assert!(output.contains("wax scan"));
    }

    #[test]
    fn collect_interactive_selections_uses_mocked_prompt_answers() {
        let manifests = vec![
            RegistryManifest {
                id: LanguageId::try_from("compose").unwrap(),
                version: "0.4.2".to_owned(),
                api_version: 1,
                targets: BTreeMap::new(),
            },
            RegistryManifest {
                id: LanguageId::try_from("react").unwrap(),
                version: "0.2.0".to_owned(),
                api_version: 1,
                targets: BTreeMap::new(),
            },
        ];
        let mut prompts = MockInitPrompts {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            scan_roots: BTreeMap::from([(
                LanguageId::try_from("compose").unwrap(),
                vec![PathBuf::from("app/src/main/kotlin")],
            )]),
            registry_in_repo: true,
            registry_roots: BTreeMap::from([(
                LanguageId::try_from("compose").unwrap(),
                vec![PathBuf::from("design-system/src/main/kotlin")],
            )]),
        };

        let selections = collect_interactive_selections(&manifests, &mut prompts).unwrap();

        assert_eq!(
            selections.languages,
            vec![LanguageId::try_from("compose").unwrap()]
        );
        assert_eq!(
            selections.scan_roots[&LanguageId::try_from("compose").unwrap()],
            vec![PathBuf::from("app/src/main/kotlin")]
        );
        assert!(matches!(
            selections.registry_setup,
            RegistrySetup::InRepository { .. }
        ));
    }

    #[test]
    fn parse_roots_splits_comma_separated_paths() {
        assert_eq!(
            parse_roots(" apps/web/src , packages/ui/src , "),
            vec![
                PathBuf::from("apps/web/src"),
                PathBuf::from("packages/ui/src"),
            ]
        );
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
                interactive: None,
            },
            &mut output,
        )
        .unwrap();

        let waxrc = load_waxrc(repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH)).unwrap();
        assert_eq!(waxrc.languages.len(), 1);
        assert_eq!(waxrc.languages[0].id, lang("compose"));
        assert!(waxrc.languages[0].enabled);
        assert_eq!(
            waxrc.languages[0]
                .registry_source()
                .map(|source| source.source),
            Some(default_registry_path_for_language(&lang("compose")))
        );

        let lock = load_lockfile(repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        let compose = lock.languages.get(&lang("compose")).unwrap();
        assert_eq!(compose.version, "0.4.2");
        assert_eq!(compose.resolved.target, "test-target");
        let registry = lock.registries.get(&lang("compose")).unwrap();
        assert_eq!(
            registry.source,
            default_registry_path_for_language(&lang("compose"))
        );

        assert!(
            repo_root
                .join(default_registry_path_for_language(&lang("compose")))
                .is_file()
        );

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
        write_default_registry(&repo_root, &lang("compose"));

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
                interactive: None,
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
        write_default_registry(&first_repo, &lang("compose"));
        write_default_registry(&second_repo, &lang("compose"));
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
                interactive: None,
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
                interactive: None,
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
                interactive: None,
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
                interactive: None,
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
        assert!(
            !repo_root
                .join(default_registry_path_for_language(&lang("compose")))
                .exists()
        );
    }

    #[test]
    fn init_leaves_repo_clean_when_default_registry_is_missing_and_scaffold_is_disabled() {
        let temp = TestDir::new("missing-default-registry");
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
                interactive: None,
            },
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(matches!(err, InitCommandError::RegistrySource(_)));
        assert!(!repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH).exists());
        assert!(!repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH).exists());
        assert!(
            !repo_root
                .join(default_registry_path_for_language(&lang("compose")))
                .exists()
        );
    }

    #[test]
    fn init_does_not_overwrite_existing_per_language_registry_when_scaffolding() {
        let _guard = env_lock();
        let temp = TestDir::new("preserve-existing-registry");
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
        let existing_registry =
            repo_root.join(default_registry_path_for_language(&lang("compose")));
        fs::create_dir_all(existing_registry.parent().unwrap()).unwrap();
        fs::write(
            &existing_registry,
            "{\n  \"schema_version\": 1,\n  \"components\": [{\"id\": \"ds.keep\"}]\n}\n",
        )
        .unwrap();

        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: true,
                interactive: None,
            },
            &mut Vec::new(),
        )
        .unwrap();

        let registry_contents = fs::read_to_string(existing_registry).unwrap();
        assert!(registry_contents.contains("\"ds.keep\""));
        let lock = load_lockfile(repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        assert_eq!(
            lock.registries[&lang("compose")].sha256,
            sha256_hex(registry_contents.as_bytes())
        );
    }

    #[test]
    fn init_scaffolded_registry_digest_matches_written_file() {
        let _guard = env_lock();
        let temp = TestDir::new("scaffolded-registry-digest");
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

        run_init(
            InitOptions {
                non_interactive: true,
                languages: vec![lang("compose")],
                no_install: true,
                registry_url: Some(file_url(&registry_path)),
                repo_root: repo_root.clone(),
                target_triple: Some("test-target".to_owned()),
                state_path: Some(temp.path.join("home/state.json")),
                scaffold_registries: true,
                interactive: None,
            },
            &mut Vec::new(),
        )
        .unwrap();

        let lock = load_lockfile(repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH)).unwrap();
        let registry_bytes =
            fs::read(repo_root.join(default_registry_path_for_language(&lang("compose")))).unwrap();
        assert_eq!(
            lock.registries[&lang("compose")].sha256,
            sha256_hex(&registry_bytes)
        );
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
        write_default_registry(&repo_root, &lang("compose"));

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
                interactive: None,
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
