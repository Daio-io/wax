use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-cli-registry-memory-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct MemoryHarness {
    wax_home: PathBuf,
    _wax_home_guard: EnvVarGuard,
}

fn setup_memory_state(wax_home: &Path, repo_root: &Path) {
    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{}},
  "design_systems": {{
    "acme": {{
      "name": "Acme Design System",
      "repo_root": "{}",
      "last_seen_config": ".wax/wax.config.json"
    }}
  }}
}}"#,
            repo_root.display()
        ),
    )
    .expect("write state");
}

fn setup_harness(repo_root: &Path) -> MemoryHarness {
    let wax_home = repo_root.parent().expect("repo parent").join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax-home");
    setup_memory_state(&wax_home, repo_root);
    let wax_home_guard = EnvVarGuard::set("WAX_HOME", &wax_home);
    MemoryHarness {
        wax_home,
        _wax_home_guard: wax_home_guard,
    }
}

fn run_registry(command: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["registry"])
        .args(command)
        .output()
        .expect("spawn wax registry")
}

#[test]
fn registry_memory_commands_list_and_show() {
    let _guard = env_lock();
    let root = TestDir::new("list-show");
    let repo = root.path.join("acme-ds");
    fs::create_dir_all(&repo).expect("create repo");
    let harness = setup_harness(&repo);

    let list = run_registry(&["list"]);
    assert!(
        list.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let list_stdout = String::from_utf8(list.stdout).unwrap();
    assert!(list_stdout.contains("id\tname\trepo_root"));
    assert!(list_stdout.contains("acme\tAcme Design System\t"));

    let show = run_registry(&["show", "acme"]);
    assert!(
        show.status.success(),
        "show failed: {}",
        String::from_utf8_lossy(&show.stderr)
    );
    let show_stdout = String::from_utf8(show.stdout).unwrap();
    assert!(show_stdout.contains("id: acme"));
    assert!(show_stdout.contains("name: Acme Design System"));
    assert!(show_stdout.contains("last_seen_config: .wax/wax.config.json"));
    assert!(show_stdout.contains(&repo.display().to_string()));

    drop(harness);
}

#[test]
fn registry_memory_commands_update_repo_root() {
    let _guard = env_lock();
    let root = TestDir::new("update");
    let original_repo = root.path.join("acme-ds");
    let updated_repo = root.path.join("acme-ds-moved");
    fs::create_dir_all(&original_repo).expect("create original repo");
    fs::create_dir_all(&updated_repo).expect("create updated repo");
    let harness = setup_harness(&original_repo);

    let output = run_registry(&[
        "update",
        "acme",
        "--repo-root",
        updated_repo.to_str().expect("utf8 path"),
    ]);
    assert!(
        output.status.success(),
        "update failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Updated design system `acme`"));

    let show = run_registry(&["show", "acme"]);
    assert!(show.status.success());
    let show_stdout = String::from_utf8(show.stdout).unwrap();
    assert!(show_stdout.contains(&updated_repo.display().to_string()));

    let state = fs::read_to_string(harness.wax_home.join("state.json")).unwrap();
    assert!(state.contains("acme-ds-moved"));

    drop(harness);
}

#[test]
fn registry_memory_commands_delete() {
    let _guard = env_lock();
    let root = TestDir::new("delete");
    let repo = root.path.join("acme-ds");
    fs::create_dir_all(&repo).expect("create repo");
    let harness = setup_harness(&repo);

    let output = run_registry(&["delete", "acme"]);
    assert!(
        output.status.success(),
        "delete failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Deleted remembered design system `acme`"));

    let list = run_registry(&["list"]);
    assert!(list.status.success());
    let list_stdout = String::from_utf8(list.stdout).unwrap();
    assert_eq!(list_stdout.lines().count(), 1, "expected header only");

    let show = run_registry(&["show", "acme"]);
    assert!(!show.status.success());
    assert!(
        String::from_utf8(show.stderr)
            .unwrap()
            .contains("not remembered")
    );

    drop(harness);
}

#[test]
fn registry_memory_commands_show_missing_reports_error() {
    let _guard = env_lock();
    let root = TestDir::new("missing");
    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax-home");
    fs::write(
        wax_home.join("state.json"),
        r#"{"installed_languages":{},"design_systems":{}}"#,
    )
    .expect("write empty state");
    let _wax_home_guard = EnvVarGuard::set("WAX_HOME", &wax_home);

    let output = run_registry(&["show", "missing"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("not remembered")
    );
}
