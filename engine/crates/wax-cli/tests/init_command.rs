use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

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
        serde_json::from_str(&fs::read_to_string(repo.join("wax.lock.json")).unwrap()).unwrap();
    let expected_version = option_env!("WAX_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    assert_eq!(lockfile["wax_version"], expected_version);
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
        "alpha index should not publish react"
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

    assert_eq!(ids, ["compose", "basic"]);
}
