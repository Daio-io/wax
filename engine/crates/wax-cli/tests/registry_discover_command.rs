use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
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

fn compose_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin")
}

fn compose_fixture_design_system_dir() -> PathBuf {
    compose_fixture_root()
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("compose design-system fixture directory")
        .to_path_buf()
}

fn link_compose_fixture_into_repo(repo: &Path) {
    fn copy_dir_all(source: &Path, destination: &Path) -> std::io::Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir_all(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    copy_dir_all(
        &compose_fixture_design_system_dir(),
        &repo.join("design-system"),
    )
    .expect("copy compose fixture");
}

fn write_compose_config_with_roots(repo: &Path, roots: &[&str]) {
    let wax_dir = repo.join(".wax");
    fs::create_dir_all(&wax_dir).expect("create .wax directory");
    let roots_json: Vec<String> = roots.iter().map(|root| format!("\"{root}\"")).collect();
    let config = format!(
        r#"{{
  "schema_version": 1,
  "languages": [
    {{
      "id": "compose",
      "enabled": true,
      "roots": [{roots}]
    }}
  ]
}}
"#,
        roots = roots_json.join(", ")
    );
    fs::write(wax_dir.join("wax.config.json"), config).expect("write wax config");
}

fn run_discover(repo: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wax"));
    command.args([
        "registry",
        "discover",
        "--language",
        "compose",
        "--repo-root",
    ]);
    command.arg(repo);
    command.arg("--root");
    command.arg(compose_fixture_root());
    command.args(extra_args);
    command.output().expect("spawn wax registry discover")
}

#[test]
fn dry_run_prints_valid_json_to_stdout() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-dry-run-json");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = run_discover(&repo, &["--dry-run"]);

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let registry: Value = serde_json::from_str(stdout.trim()).expect("stdout should be valid json");

    assert_eq!(registry["schema_version"], 1);
    assert!(registry["components"].is_array());
    assert!(!stdout.is_empty());
    assert!(stderr.contains("Discovered"));
    assert!(stderr.contains("false positives"));
    assert!(!stdout.contains("Discovered"));
    assert!(!repo.join(".wax/wax.registry.json").exists());
}

#[test]
fn dry_run_writes_summaries_and_warnings_to_stderr_not_stdout() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-dry-run-streams");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = run_discover(&repo, &["--dry-run"]);

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("Discovered"));
    assert!(!stdout.contains("warning:"));
    assert!(!stdout.contains("false positives"));
}

#[test]
fn default_write_creates_centralized_registry_path() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-write");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = run_discover(&repo, &[]);

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let registry_path = repo.join(".wax/wax.registry.json");
    assert!(registry_path.is_file());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("Wrote .wax/wax.registry.json"));
    assert!(stdout.contains("Review before committing"));
    assert!(stdout.contains("wax validate"));
    assert!(stdout.contains("wax language update"));
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("false positives"));
    assert!(stdout.contains("false positives"));

    let written: Value =
        serde_json::from_str(&fs::read_to_string(&registry_path).unwrap()).unwrap();
    assert_eq!(written["schema_version"], 1);
    assert!(written["components"].as_array().unwrap().len() >= 2);
}

#[test]
fn second_write_fails_without_force() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-refuse-overwrite");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let first = run_discover(&repo, &[]);
    assert!(first.status.success(), "first write should succeed");

    let registry_path = repo.join(".wax/wax.registry.json");
    let original = fs::read_to_string(&registry_path).unwrap();

    let second = run_discover(&repo, &[]);
    assert!(!second.status.success(), "second write should fail");

    let stderr = String::from_utf8(second.stderr).unwrap();
    assert!(stderr.contains("--force"));
    assert!(stderr.contains("--dry-run"));
    assert_eq!(fs::read_to_string(&registry_path).unwrap(), original);
}

#[test]
fn force_replaces_existing_registry() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-force");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let first = run_discover(&repo, &[]);
    assert!(first.status.success());

    let registry_path = repo.join(".wax/wax.registry.json");
    fs::write(&registry_path, "{\"schema_version\":1,\"components\":[]}\n").unwrap();

    let forced = run_discover(&repo, &["--force"]);
    assert!(
        forced.status.success(),
        "forced write should succeed, stderr: {}",
        String::from_utf8_lossy(&forced.stderr)
    );

    let written: Value =
        serde_json::from_str(&fs::read_to_string(&registry_path).unwrap()).unwrap();
    assert!(written["components"].as_array().unwrap().len() >= 2);
}

#[test]
fn relative_root_is_resolved_against_repo_root_when_cwd_differs() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-relative-root");
    let repo = root.path.join("repo");
    let kotlin_root = repo.join("src/main/kotlin");
    fs::create_dir_all(&kotlin_root).unwrap();
    fs::write(
        kotlin_root.join("Components.kt"),
        r#"import androidx.compose.runtime.Composable

@Composable
fun PrimaryButton() {}
"#,
    )
    .unwrap();

    let outside_cwd = root.path.join("outside");
    fs::create_dir_all(&outside_cwd).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .current_dir(&outside_cwd)
        .args([
            "registry",
            "discover",
            "--language",
            "compose",
            "--repo-root",
        ])
        .arg(&repo)
        .args(["--root", "src/main/kotlin", "--dry-run"])
        .output()
        .expect("spawn wax registry discover");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let registry: Value = serde_json::from_str(stdout.trim()).expect("stdout should be valid json");
    assert_eq!(registry["components"][0]["symbol"], "PrimaryButton");
}

#[test]
fn dry_run_uses_config_roots_when_root_omitted() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-config-roots");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();
    link_compose_fixture_into_repo(&repo);
    write_compose_config_with_roots(&repo, &["design-system/src/main/kotlin"]);

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "registry",
            "discover",
            "--language",
            "compose",
            "--repo-root",
        ])
        .arg(&repo)
        .arg("--dry-run")
        .output()
        .expect("spawn wax registry discover");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let registry: Value = serde_json::from_str(stdout.trim()).expect("stdout should be valid json");

    assert_eq!(registry["schema_version"], 1);
    assert!(registry["components"].as_array().unwrap().len() >= 2);
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("--root path/to/design-system"));
    assert!(!repo.join(".wax/wax.registry.json").exists());
}

#[test]
fn missing_root_fails_with_guidance() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-missing-root");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args([
            "registry",
            "discover",
            "--language",
            "compose",
            "--repo-root",
        ])
        .arg(&repo)
        .arg("--dry-run")
        .output()
        .expect("spawn wax registry discover");

    assert!(!output.status.success(), "expected missing root to fail");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("pass --root path/to/design-system"));
    assert!(!repo.join(".wax/wax.registry.json").exists());
}
