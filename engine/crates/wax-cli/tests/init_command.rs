use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_cli::commands::init::{InitOptions, InitSelections, RegistrySetup, run_init};
use wax_contract::LanguageId;
use wax_core::config::lockfile::load_lockfile;
use wax_core::config::waxrc::load_waxrc;
use wax_core::registry_cache::RegistrySourceSpec;
use wax_core::registry_memory::add_remembered_registry;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    #[expect(
        unsafe_code,
        reason = "these tests hold ENV_LOCK while mutating process environment variables, which keeps env access serialized inside this test binary"
    )]
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(name);
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    #[expect(
        unsafe_code,
        reason = "these tests hold ENV_LOCK while restoring process environment variables, which keeps env access serialized inside this test binary"
    )]
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-cli-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn init_loads_file_copy_of_alpha_pack_index() {
    let _guard = env_lock();
    let root = TestDir::new("init-alpha-index");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax home fixture");

    let alpha_index = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("registry")
        .join("alpha-index.json");
    let registry_copy = root.path.join("alpha-index.json");
    fs::copy(&alpha_index, &registry_copy).expect("copy alpha index fixture");
    assert_alpha_index_ids(&registry_copy);
    let registry_url = format!("file://{}", registry_copy.display());

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "init",
            "--non-interactive",
            "--language",
            "compose",
            "--no-install",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pack-index",
        ])
        .arg(&registry_url)
        .args(["--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax init");

    assert!(
        output.status.success(),
        "wax init exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let lockfile: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo.join(".wax/wax.lock.json")).unwrap())
            .unwrap();
    assert_eq!(lockfile["schema_version"], 2);
    let expected_version = option_env!("WAX_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    assert_eq!(lockfile["wax_version"], expected_version);
    assert!(
        lockfile["registries"].as_object().is_some(),
        "init should write an explicit registries object for schema v2"
    );
    assert_eq!(
        lockfile["registries"]["compose"]["source"],
        ".wax/compose.registry.json"
    );
    assert!(
        lockfile["registries"]["compose"]["sha256"]
            .as_str()
            .is_some_and(|sha256| !sha256.is_empty())
    );
    let compose = &lockfile["languages"]["compose"];
    assert_eq!(compose["version"], "0.1.0-alpha.0");
    assert_eq!(compose["api_version"], 1);
    assert_eq!(compose["source"], registry_url);
    assert_eq!(compose["resolved"]["target"], "x86_64-unknown-linux-gnu");
    assert_eq!(
        compose["resolved"]["url"],
        "https://github.com/Daio-io/wax/releases/latest/download/wax-lang-compose-0.1.0-alpha.0-x86_64-unknown-linux-gnu.tar.gz"
    );
    assert_eq!(
        compose["resolved"]["sha256"],
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert!(
        lockfile["languages"].get("react").is_none(),
        "init with compose should not write react lockfile entry"
    );
}

fn assert_alpha_index_ids(index_path: &Path) {
    let index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(index_path).unwrap()).unwrap();
    let ids = index
        .as_array()
        .expect("alpha index should be an array")
        .iter()
        .map(|entry| {
            entry["id"]
                .as_str()
                .expect("alpha index entries should have ids")
        })
        .collect::<Vec<_>>();

    assert_eq!(ids, ["compose", "basic", "react", "swift"]);
}

#[test]
fn init_writes_centralized_wax_layout_and_gitignore() {
    let _guard = env_lock();
    let root = TestDir::new("init-centralized-layout");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax home fixture");

    let alpha_index = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("registry")
        .join("alpha-index.json");
    let registry_copy = root.path.join("alpha-index.json");
    fs::copy(&alpha_index, &registry_copy).expect("copy alpha index fixture");
    let registry_url = format!("file://{}", registry_copy.display());

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "init",
            "--non-interactive",
            "--language",
            "compose",
            "--no-install",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pack-index",
        ])
        .arg(&registry_url)
        .args(["--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax init");

    assert!(
        output.status.success(),
        "wax init exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(repo.join(".wax/wax.config.json").is_file());
    assert!(repo.join(".wax/wax.lock.json").is_file());
    assert!(repo.join(".wax/compose.registry.json").is_file());
    assert!(!repo.join(".wax/wax.registry.json").exists());
    assert!(!repo.join(".waxrc").exists());
    assert!(!repo.join("wax.lock.json").exists());
    assert!(!repo.join("design-system/registry.json").exists());

    let gitignore = fs::read_to_string(repo.join(".gitignore")).expect("read .gitignore");
    assert!(gitignore.contains("/.wax/cache/"));
    assert!(gitignore.contains("/.wax/out/"));
}

#[test]
fn init_does_not_duplicate_gitignore_entries() {
    let _guard = env_lock();
    let root = TestDir::new("init-gitignore-dedupe");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax home fixture");
    fs::write(repo.join(".gitignore"), "/.wax/cache/\n/.wax/out/\n")
        .expect("write existing gitignore");

    let alpha_index = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("registry")
        .join("alpha-index.json");
    let registry_copy = root.path.join("alpha-index.json");
    fs::copy(&alpha_index, &registry_copy).expect("copy alpha index fixture");
    let registry_url = format!("file://{}", registry_copy.display());

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "init",
            "--non-interactive",
            "--language",
            "compose",
            "--no-install",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pack-index",
        ])
        .arg(&registry_url)
        .args(["--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax init");

    assert!(
        output.status.success(),
        "wax init exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let gitignore = fs::read_to_string(repo.join(".gitignore")).expect("read .gitignore");
    assert_eq!(gitignore.matches("/.wax/cache/").count(), 1);
    assert_eq!(gitignore.matches("/.wax/out/").count(), 1);
}

#[test]
fn init_scaffolds_per_language_registry_for_single_language() {
    let _guard = env_lock();
    let root = TestDir::new("init-per-language-registry");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax home fixture");

    let alpha_index = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("registry")
        .join("alpha-index.json");
    let registry_copy = root.path.join("alpha-index.json");
    fs::copy(&alpha_index, &registry_copy).expect("copy alpha index fixture");
    let registry_url = format!("file://{}", registry_copy.display());

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "init",
            "--non-interactive",
            "--language",
            "compose",
            "--no-install",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pack-index",
        ])
        .arg(&registry_url)
        .args(["--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax init");

    assert!(
        output.status.success(),
        "wax init exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(repo.join(".wax/compose.registry.json").is_file());
    assert!(!repo.join(".wax/wax.registry.json").exists());
    assert!(!repo.join("design-system/registry.json").exists());
}

fn lang(id: &str) -> LanguageId {
    LanguageId::try_from(id).expect("language id")
}

#[test]
fn init_scaffolds_per_language_registry_files_for_multi_language_repo() {
    let _guard = env_lock();
    let root = TestDir::new("init-per-language-registries");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax home fixture");

    let alpha_index = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("registry")
        .join("alpha-index.json");
    let registry_copy = root.path.join("alpha-index.json");
    fs::copy(&alpha_index, &registry_copy).expect("copy alpha index fixture");
    let registry_url = format!("file://{}", registry_copy.display());

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "init",
            "--non-interactive",
            "--language",
            "compose",
            "--language",
            "react",
            "--no-install",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pack-index",
        ])
        .arg(&registry_url)
        .args(["--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax init");

    assert!(
        output.status.success(),
        "wax init exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(repo.join(".wax/compose.registry.json").is_file());
    assert!(repo.join(".wax/react.registry.json").is_file());
    assert!(!repo.join(".wax/wax.registry.json").exists());

    let waxrc = load_waxrc(repo.join(".wax/wax.config.json")).unwrap();
    let compose = waxrc
        .languages
        .iter()
        .find(|language| language.id.as_str() == "compose")
        .unwrap();
    let react = waxrc
        .languages
        .iter()
        .find(|language| language.id.as_str() == "react")
        .unwrap();
    assert_eq!(
        compose
            .registry_source
            .as_ref()
            .and_then(|source| source.path_or_url_parts().map(|(source, _)| source)),
        Some(".wax/compose.registry.json")
    );
    assert_eq!(
        react
            .registry_source
            .as_ref()
            .and_then(|source| source.path_or_url_parts().map(|(source, _)| source)),
        Some(".wax/react.registry.json")
    );

    let lock = load_lockfile(repo.join(".wax/wax.lock.json")).unwrap();
    assert_eq!(
        lock.registries.get(&lang("compose")).unwrap().source,
        ".wax/compose.registry.json"
    );
    assert_eq!(
        lock.registries.get(&lang("react")).unwrap().source,
        ".wax/react.registry.json"
    );
}

#[test]
fn init_scaffolds_swift_per_language_registry_and_lock_entry() {
    let _guard = env_lock();
    let root = TestDir::new("init-swift-per-language-registry");
    let repo = root.path.join("repo");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&repo).expect("create repo fixture");
    fs::create_dir_all(&wax_home).expect("create wax home fixture");

    let alpha_index = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("registry")
        .join("alpha-index.json");
    let registry_copy = root.path.join("alpha-index.json");
    fs::copy(&alpha_index, &registry_copy).expect("copy alpha index fixture");
    let registry_url = format!("file://{}", registry_copy.display());

    let _wax_home = EnvVarGuard::set("WAX_HOME", &wax_home);
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "init",
            "--non-interactive",
            "--language",
            "swift",
            "--no-install",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--pack-index",
        ])
        .arg(&registry_url)
        .args(["--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax init");

    assert!(
        output.status.success(),
        "wax init exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(repo.join(".wax/swift.registry.json").is_file());
    assert!(!repo.join(".wax/compose.registry.json").exists());

    let waxrc = load_waxrc(repo.join(".wax/wax.config.json")).unwrap();
    let swift = waxrc
        .languages
        .iter()
        .find(|language| language.id.as_str() == "swift")
        .expect("swift language entry");
    assert_eq!(
        swift
            .registry_source
            .as_ref()
            .and_then(|source| source.path_or_url_parts().map(|(source, _)| source)),
        Some(".wax/swift.registry.json")
    );
    assert_eq!(swift.roots, ["App/Sources"]);

    let lock = load_lockfile(repo.join(".wax/wax.lock.json")).unwrap();
    let swift_registry = lock.registries.get(&lang("swift")).unwrap();
    assert_eq!(swift_registry.source, ".wax/swift.registry.json");
    assert!(!swift_registry.sha256.is_empty());
}

fn write_pack_artifact(path: &Path, pack_name: &str) -> String {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use sha2::{Digest, Sha256};
    use tar::Builder;

    let mut archive = Vec::new();
    {
        let encoder = GzEncoder::new(&mut archive, Compression::default());
        let mut builder = Builder::new(encoder);
        let manifest = format!(
            r#"{{
  "id": "compose",
  "version": "0.4.2",
  "api_version": 1,
  "command": ["{pack_name}"]
}}"#
        );
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "manifest.json", manifest.as_bytes())
            .expect("append manifest");
        builder.into_inner().expect("finish archive");
    }
    fs::write(path, archive).expect("write pack artifact");
    let digest = Sha256::digest(fs::read(path).expect("read artifact"));
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[test]
fn init_non_interactive_source_backed_remembered_writes_explicit_url() {
    let _guard = env_lock();
    let root = TestDir::new("init-noninteractive-source-backed");
    let source = root.path.join("compose-registry.json");
    let registry_bytes = br#"{"schema_version":1,"language":"compose","components":[{"id":"button","symbol":"Button"}]}"#;
    fs::write(&source, registry_bytes).expect("write remote registry");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    let state_path = wax_home.join("state.json");
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::LocalFile {
            path: source.clone(),
            language: None,
        },
    )
    .expect("remember source-backed registry");

    let artifact_path = root.path.join("compose.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
    let pack_index = root.path.join("pack-index.json");
    fs::write(
        &pack_index,
        format!(
            r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path),
            digest
        ),
    )
    .expect("write pack index");

    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create repo");

    run_init(
        InitOptions {
            non_interactive: true,
            languages: Vec::new(),
            no_install: true,
            registry_url: Some(file_url(&pack_index)),
            repo_root: repo.clone(),
            target_triple: Some("test-target".to_owned()),
            state_path: Some(state_path),
            scaffold_registries: false,
            interactive: Some(InitSelections {
                languages: vec![lang("compose")],
                scan_roots: BTreeMap::from([(lang("compose"), vec![PathBuf::from("src")])]),
                registry_setup: RegistrySetup::RememberedDesignSystem {
                    design_system_id: "acme".to_owned(),
                },
            }),
        },
        &mut Vec::new(),
    )
    .expect("non-interactive init with source-backed remembered registry");

    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".wax/wax.config.json")).expect("wax config"),
    )
    .unwrap();
    assert_eq!(
        config["languages"]["compose"]["registry"]["source"],
        file_url(&source)
    );
    assert_eq!(
        config["languages"]["compose"]["registry"]["upstream"],
        "acme/compose"
    );
    let lock = load_lockfile(repo.join(".wax/wax.lock.json")).unwrap();
    assert_eq!(lock.registries[&lang("compose")].source, file_url(&source));
    assert_eq!(
        lock.registries[&lang("compose")].sha256,
        wax_core::digest::sha256_hex(registry_bytes)
    );
}

#[test]
fn init_non_interactive_git_remembered_writes_git_tag_and_pins_commit() {
    use std::process::Output;
    use wax_core::digest::sha256_hex;

    let _guard = env_lock();
    let root = TestDir::new("init-noninteractive-git-remembered");
    let work = root.path.join("git-work");
    let remote = root.path.join("remote.git");
    fs::create_dir_all(work.join(".wax/registries")).expect("registry directory");
    let git = |args: &[&str], cwd: &Path| -> Output {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Wax Test",
                "-c",
                "user.email=wax-test@example.com",
            ])
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    git(&["init", "-q", "-b", "main"], &work);
    git(
        &[
            "init",
            "--bare",
            "-q",
            remote.to_str().expect("remote path"),
        ],
        &root.path,
    );
    git(&["symbolic-ref", "HEAD", "refs/heads/main"], &remote);
    let registry_bytes = br#"{"schema_version":1,"language":"compose","components":[{"id":"button","symbol":"Button"}]}"#;
    fs::write(work.join(".wax/registries/compose.json"), registry_bytes)
        .expect("write compose registry");
    git(&["add", "."], &work);
    git(&["commit", "-q", "-m", "first"], &work);
    let first_commit = String::from_utf8(git(&["rev-parse", "HEAD"], &work).stdout)
        .expect("utf8")
        .trim()
        .to_owned();
    git(&["tag", "v1"], &work);
    git(
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
        &work,
    );
    git(&["push", "-q", "origin", "main", "--tags"], &work);

    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax home");
    let state_path = wax_home.join("state.json");
    let remote_url = remote.display().to_string();
    add_remembered_registry(
        &state_path,
        "acme",
        "Acme Design System",
        &RegistrySourceSpec::Git {
            url: remote_url.clone(),
            tag: None,
            language: lang("compose"),
        },
    )
    .expect("remember git registry following default branch");

    let artifact_path = root.path.join("compose.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
    let pack_index = root.path.join("pack-index.json");
    fs::write(
        &pack_index,
        format!(
            r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path),
            digest
        ),
    )
    .expect("write pack index");

    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create repo");
    run_init(
        InitOptions {
            non_interactive: true,
            languages: Vec::new(),
            no_install: true,
            registry_url: Some(file_url(&pack_index)),
            repo_root: repo.clone(),
            target_triple: Some("test-target".to_owned()),
            state_path: Some(state_path),
            scaffold_registries: false,
            interactive: Some(InitSelections {
                languages: vec![lang("compose")],
                scan_roots: BTreeMap::from([(lang("compose"), vec![PathBuf::from("src")])]),
                registry_setup: RegistrySetup::RememberedDesignSystem {
                    design_system_id: "acme".to_owned(),
                },
            }),
        },
        &mut Vec::new(),
    )
    .expect("non-interactive init with git remembered registry");

    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".wax/wax.config.json")).expect("wax config"),
    )
    .unwrap();
    assert_eq!(
        config["languages"]["compose"]["registry"]["git"],
        remote_url
    );
    assert_eq!(
        config["languages"]["compose"]["registry"]["tag"],
        first_commit
    );
    let lock = load_lockfile(repo.join(".wax/wax.lock.json")).unwrap();
    let locked = &lock.registries[&lang("compose")];
    assert_eq!(locked.sha256, sha256_hex(registry_bytes));
    assert_eq!(locked.commit.as_deref(), Some(first_commit.as_str()));
    assert_eq!(locked.git.as_deref(), Some(remote_url.as_str()));
}
