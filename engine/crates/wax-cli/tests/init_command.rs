use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use wax_contract::LanguageId;
use wax_core::config::lockfile::load_lockfile;
use wax_core::config::waxrc::load_waxrc;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
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
            "--registry",
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
            "--registry",
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
            "--registry",
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
            "--registry",
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
            "--registry",
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
        compose.registry_source().map(|source| source.source),
        Some(".wax/compose.registry.json".to_owned())
    );
    assert_eq!(
        react.registry_source().map(|source| source.source),
        Some(".wax/react.registry.json".to_owned())
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
            "--registry",
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
    assert!(!repo.join(".wax/wax.registry.json").exists());

    let waxrc = load_waxrc(repo.join(".wax/wax.config.json")).unwrap();
    let swift = waxrc
        .languages
        .iter()
        .find(|language| language.id.as_str() == "swift")
        .expect("swift language entry");
    assert_eq!(
        swift.registry_source().map(|source| source.source),
        Some(".wax/swift.registry.json".to_owned())
    );
    assert_eq!(swift.extra["roots"], serde_json::json!(["App/Sources"]));

    let lock = load_lockfile(repo.join(".wax/wax.lock.json")).unwrap();
    assert_eq!(
        lock.registries.get(&lang("swift")).unwrap().source,
        ".wax/swift.registry.json"
    );
}
