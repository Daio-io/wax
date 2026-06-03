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
fn validate_command_exits_zero_and_prints_warning_for_empty_components() {
    let _guard = env_lock();
    let root = TestDir::new("validate-command-warning");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    write_repo(&repo, "design-system/registry.json", "[]");
    let _wax_home = EnvVarGuard::set("WAX_HOME", root.path.join("wax-home"));

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax validate");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warning:"));
    assert!(stdout.contains("validation passed"));
}

#[test]
fn validate_command_exits_non_zero_on_invalid_repo() {
    let _guard = env_lock();
    let root = TestDir::new("validate-command-fail");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    fs::write(
        repo.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    { "id": "compose", "enabled": true }
  ]
}"#,
    )
    .unwrap();
    fs::write(
        repo.join("wax.lock.json"),
        r#"{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "languages": {
    "compose": {
      "version": "0.1.0",
      "api_version": 1,
      "source": "file:///tmp/registry.json",
      "resolved": {
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }
    }
  }
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax validate");

    assert!(!output.status.success(), "expected validation failure");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("invalid .wax config field languages[0].registry"));
}

#[test]
fn validate_command_prints_deprecated_registry_warning() {
    let _guard = env_lock();
    let root = TestDir::new("validate-command-deprecated-registry-warning");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    let registry_path = "design-system/registry.json";
    let registry_abs = repo.join(registry_path);
    fs::create_dir_all(registry_abs.parent().unwrap()).unwrap();
    fs::write(
        &registry_abs,
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    fs::write(
        repo.join(".waxrc"),
        format!(
            "{{\n  \"schema_version\": 1,\n  \"languages\": [{{\"id\":\"compose\",\"enabled\":true,\"design_system_registry\":\"{registry_path}\"}}]\n}}\n"
        ),
    )
    .unwrap();
    write_legacy_lockfile_with_registry(&repo, registry_path);
    let _wax_home = EnvVarGuard::set("WAX_HOME", root.path.join("wax-home"));

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax validate");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(
        "warning: language compose uses deprecated languages[0].design_system_registry; use registry instead"
    ));
}

#[test]
fn validate_command_prints_ignored_legacy_file_warnings() {
    let _guard = env_lock();
    let root = TestDir::new("validate-command-ignored-legacy-warnings");
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join(".wax")).unwrap();

    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true}]}"#,
    )
    .unwrap();
    fs::write(
        repo.join(".wax/wax.registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    write_lockfile_with_registry(&repo, ".wax/wax.registry.json");
    fs::write(
        repo.join(".waxrc"),
        r#"{"schema_version":1,"languages":[]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("wax.lock.json"),
        r#"{"schema_version":1,"engine_api_version":1,"wax_version":"0.0.0","languages":{}}"#,
    )
    .unwrap();
    let _wax_home = EnvVarGuard::set("WAX_HOME", root.path.join("wax-home"));

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax validate");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warning: ignored legacy config"));
    assert!(stderr.contains(".waxrc"));
    assert!(stderr.contains("warning: ignored legacy lockfile"));
    assert!(stderr.contains("wax.lock.json"));
}

#[test]
fn validate_command_prints_partial_layout_migration_warning() {
    let _guard = env_lock();
    let root = TestDir::new("validate-command-partial-layout");
    let repo = root.path.join("repo");
    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true,"registry":"design-system/registry.json"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(repo.join("design-system")).unwrap();
    fs::write(
        repo.join("design-system/registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    write_legacy_lockfile_with_registry(&repo, "design-system/registry.json");
    let _wax_home = EnvVarGuard::set("WAX_HOME", root.path.join("wax-home"));

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax validate");

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warning: using legacy lockfile"));
    assert!(stderr.contains("wax.lock.json"));
    assert!(stderr.contains(".wax/wax.config.json"));
}

fn write_repo(repo: &Path, registry_path: &str, components: &str) {
    let registry_abs = repo.join(registry_path);
    if let Some(parent) = registry_abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        &registry_abs,
        format!("{{\n  \"schema_version\": 1,\n  \"components\": {components}\n}}\n"),
    )
    .unwrap();

    fs::write(
        repo.join(".waxrc"),
        format!(
            "{{\n  \"schema_version\": 1,\n  \"languages\": [{{\"id\":\"compose\",\"enabled\":true,\"registry\":\"{registry_path}\"}}]\n}}\n"
        ),
    )
    .unwrap();

    write_legacy_lockfile_with_registry(repo, registry_path);
}

fn write_lockfile_with_registry(repo_root: &Path, source: &str) {
    fs::write(
        repo_root.join(".wax/wax.lock.json"),
        lockfile_json(repo_root, source),
    )
    .unwrap();
}

fn write_legacy_lockfile_with_registry(repo_root: &Path, source: &str) {
    fs::write(
        repo_root.join("wax.lock.json"),
        lockfile_json(repo_root, source),
    )
    .unwrap();
}

fn lockfile_json(repo_root: &Path, source: &str) -> String {
    let resolved = wax_core::registry_source::resolve_registry_source(
        wax_core::registry_source::RegistrySourceInput {
            repo_root,
            language_id: "compose",
            source: Some(source),
        },
    )
    .unwrap();
    format!(
        r#"{{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "locked_at": null,
  "registries": {{
    "compose": {{
      "source": "{source}",
      "sha256": "{}"
    }}
  }},
  "languages": {{
    "compose": {{
      "version": "0.1.0",
      "api_version": 1,
      "source": "file:///tmp/registry.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }}
    }}
  }}
}}
"#,
        resolved.sha256
    )
}
