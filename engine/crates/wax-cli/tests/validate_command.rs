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
    fs::create_dir_all(repo.join(".wax")).unwrap();

    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {"compose": {}}
}"#,
    )
    .unwrap();
    fs::write(
        repo.join(".wax/wax.lock.json"),
        r#"{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "registries": {},
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
    assert!(stderr.contains("invalid .wax config field languages.compose.registry"));
}

#[test]
fn validate_command_exits_non_zero_on_registry_source_drift() {
    let _guard = env_lock();
    let root = TestDir::new("validate-command-registry-source-drift");
    let repo = root.path.join("repo");
    write_repo(
        &repo,
        "design-system/registry.json",
        r#"[{"id":"ds.button","symbol":"Button"}]"#,
    );
    let registry_sha256 = wax_core::registry_source::resolve_registry_source(
        wax_core::registry_source::RegistrySourceInput {
            repo_root: &repo,
            language_id: "compose",
            source: Some("design-system/registry.json"),
        },
    )
    .unwrap()
    .sha256;
    fs::write(
        repo.join(".wax/wax.lock.json"),
        lockfile_json_with_sha256("legacy/registry.json", &registry_sha256),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .args(["validate", "--repo-root"])
        .arg(&repo)
        .output()
        .expect("spawn wax validate");

    assert!(!output.status.success(), "expected validation failure");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("registry source drift"));
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

    fs::create_dir_all(repo.join(".wax")).unwrap();
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            "{{\n  \"schema_version\": 2,\n  \"languages\": {{\"compose\": {{\"registry\": \"{registry_path}\"}}}}\n}}\n"
        ),
    )
    .unwrap();

    write_lockfile_with_registry(repo, registry_path);
}

fn write_lockfile_with_registry(repo_root: &Path, source: &str) {
    fs::create_dir_all(repo_root.join(".wax")).unwrap();
    fs::write(
        repo_root.join(".wax/wax.lock.json"),
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
    lockfile_json_with_sha256(source, &resolved.sha256)
}

fn lockfile_json_with_sha256(source: &str, sha256: &str) -> String {
    format!(
        r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.0.0",
  "locked_at": null,
  "registries": {{
    "compose": {{
      "source": "{source}",
      "sha256": "{sha256}"
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
"#
    )
}
