use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

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

fn write_compose_lockfile(repo: &Path) {
    fs::create_dir_all(repo.join(".wax")).expect("create .wax directory");
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
      "source": "https://example.invalid/compose-index.json",
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
    .expect("write compose lockfile");
}

fn write_multi_language_config(repo: &Path) {
    fs::create_dir_all(repo.join(".wax")).expect("create .wax directory");
    fs::write(
        repo.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "roots": ["design-system/src/main/kotlin"]
    },
    {
      "id": "react",
      "enabled": true,
      "roots": ["design-system/src"]
    }
  ]
}
"#,
    )
    .expect("write multi-language wax config");
}

fn write_multi_language_lockfile(repo: &Path) {
    fs::create_dir_all(repo.join(".wax")).expect("create .wax directory");
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
      "source": "https://example.invalid/compose-index.json",
      "resolved": {
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/compose-0.1.0.tgz",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "signature": null
      }
    },
    "react": {
      "version": "0.1.0",
      "api_version": 1,
      "source": "https://example.invalid/react-index.json",
      "resolved": {
        "target": "x86_64-unknown-linux-gnu",
        "url": "https://example.invalid/react-0.1.0.tgz",
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "signature": null
      }
    }
  }
}
"#,
    )
    .expect("write multi-language lockfile");
}

fn install_discover_fixture_pack_files(
    wax_home: &Path,
    language_id: &str,
    version: &str,
    response_json: &str,
    sha256: &str,
) -> PathBuf {
    let install_dir = wax_home.join(format!("langs/{language_id}/{version}"));
    fs::create_dir_all(&install_dir).expect("create language install dir");

    let script = install_dir.join(format!("wax-lang-{language_id}"));
    fs::write(
        &script,
        format!("#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{response_json}\nJSON\n"),
    )
    .expect("write discover fixture script");
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&script)
            .expect("read script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("set script executable");
    }

    fs::write(
        install_dir.join("manifest.json"),
        format!(
            r#"{{
  "id": "{language_id}",
  "version": "{version}",
  "api_version": 1,
  "command": ["./wax-lang-{language_id}"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "{sha256}",
  "ecosystem": "test",
  "parser_name": "fixture",
  "parser_version": "1.0.0"
}}
"#
        ),
    )
    .expect("write fixture manifest");

    install_dir
}

fn write_discover_fixture_state(wax_home: &Path, languages: &[(&str, &str, PathBuf)]) {
    let state_entries: Vec<String> = languages
        .iter()
        .map(|(language_id, version, install_dir)| {
            format!(
                r#"    "{language_id}": {{
      "{version}": {{ "install_dir": "{}" }}
    }}"#,
                install_dir.display()
            )
        })
        .collect();

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
{}
  }}
}}"#,
            state_entries.join(",\n")
        ),
    )
    .expect("write wax global state");
}

fn install_compose_discover_fixture_pack(wax_home: &Path) {
    let install_dir = install_discover_fixture_pack_files(
        wax_home,
        "compose",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton","SecondaryButton","QualifiedButton"],"diagnostics":[]}"#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    write_discover_fixture_state(wax_home, &[("compose", "0.1.0", install_dir)]);
}

fn install_compose_and_react_discover_fixture_packs(wax_home: &Path) {
    let compose_dir = install_discover_fixture_pack_files(
        wax_home,
        "compose",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton","SecondaryButton","QualifiedButton"],"diagnostics":[]}"#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    let react_dir = install_discover_fixture_pack_files(
        wax_home,
        "react",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"react","symbols":["Button"],"diagnostics":[]}"#,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    write_discover_fixture_state(
        wax_home,
        &[
            ("compose", "0.1.0", compose_dir),
            ("react", "0.1.0", react_dir),
        ],
    );
}

struct DiscoverHarness {
    _wax_home_guard: EnvVarGuard,
}

fn setup_compose_discover_repo(repo: &Path, with_fixture: bool) -> DiscoverHarness {
    if with_fixture {
        link_compose_fixture_into_repo(repo);
    }
    write_compose_lockfile(repo);
    let wax_home = repo.parent().expect("repo parent").join("wax-home");
    fs::create_dir_all(&wax_home).expect("create wax-home");
    install_compose_discover_fixture_pack(&wax_home);
    let wax_home_guard = EnvVarGuard::set("WAX_HOME", &wax_home);
    DiscoverHarness {
        _wax_home_guard: wax_home_guard,
    }
}

fn run_discover_for_language(
    repo: &Path,
    language_id: &str,
    root: &Path,
    extra_args: &[&str],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wax"));
    command.args([
        "registry",
        "discover",
        "--language",
        language_id,
        "--repo-root",
    ]);
    command.arg(repo);
    command.arg("--root");
    command.arg(root);
    command.args(extra_args);
    command.output().expect("spawn wax registry discover")
}

fn run_discover(repo: &Path, extra_args: &[&str]) -> std::process::Output {
    run_discover_for_language(repo, "compose", &compose_fixture_root(), extra_args)
}

#[test]
fn dry_run_prints_valid_json_to_stdout() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-dry-run-json");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let _harness = setup_compose_discover_repo(&repo, false);

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
    assert!(!repo.join(".wax/compose.registry.json").exists());
}

#[test]
fn dry_run_writes_summaries_and_warnings_to_stderr_not_stdout() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-dry-run-streams");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let _harness = setup_compose_discover_repo(&repo, false);

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
fn default_write_creates_language_specific_registry_path() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-write");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let _harness = setup_compose_discover_repo(&repo, false);

    let output = run_discover(&repo, &[]);

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let registry_path = repo.join(".wax/compose.registry.json");
    assert!(registry_path.is_file());
    assert!(!repo.join(".wax/wax.registry.json").exists());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("Wrote .wax/compose.registry.json"));
    assert!(stdout.contains("Review before committing"));
    assert!(stdout.contains("wax validate"));
    assert!(stdout.contains("wax language update"));
    assert!(!stderr.contains("false positives"));
    assert!(stdout.contains("false positives"));

    let written: Value =
        serde_json::from_str(&fs::read_to_string(&registry_path).unwrap()).unwrap();
    assert_eq!(written["schema_version"], 1);
    assert!(written["components"].as_array().unwrap().len() >= 2);
}

#[test]
fn discover_compose_then_react_writes_both_without_force() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-multi-language");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();
    write_multi_language_config(&repo);
    link_compose_fixture_into_repo(&repo);
    write_multi_language_lockfile(&repo);

    let wax_home = root.path.join("wax-home");
    fs::create_dir_all(&wax_home).unwrap();
    install_compose_and_react_discover_fixture_packs(&wax_home);
    let _wax_home_guard = EnvVarGuard::set("WAX_HOME", &wax_home);

    let compose_output = run_discover_for_language(
        &repo,
        "compose",
        &repo.join("design-system/src/main/kotlin"),
        &[],
    );
    assert!(
        compose_output.status.success(),
        "compose discover should succeed, stderr: {}",
        String::from_utf8_lossy(&compose_output.stderr)
    );
    let compose_stdout = String::from_utf8(compose_output.stdout).unwrap();
    assert!(compose_stdout.contains("Wrote .wax/compose.registry.json"));
    assert!(!compose_stdout.contains("Wrote .wax/react.registry.json"));

    let react_output =
        run_discover_for_language(&repo, "react", &repo.join("design-system/src"), &[]);
    assert!(
        react_output.status.success(),
        "react discover should succeed without --force, stderr: {}",
        String::from_utf8_lossy(&react_output.stderr)
    );
    let react_stdout = String::from_utf8(react_output.stdout).unwrap();
    assert!(react_stdout.contains("Wrote .wax/react.registry.json"));
    assert!(!react_stdout.contains("Wrote .wax/compose.registry.json"));

    let compose_path = repo.join(".wax/compose.registry.json");
    let react_path = repo.join(".wax/react.registry.json");
    assert!(compose_path.is_file());
    assert!(react_path.is_file());
    assert!(!repo.join(".wax/wax.registry.json").exists());

    let compose_registry: Value =
        serde_json::from_str(&fs::read_to_string(&compose_path).unwrap()).unwrap();
    let react_registry: Value =
        serde_json::from_str(&fs::read_to_string(&react_path).unwrap()).unwrap();
    assert!(
        compose_registry["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|component| component["symbol"] == "PrimaryButton")
    );
    assert!(
        react_registry["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|component| component["symbol"] == "Button")
    );
}

#[test]
fn second_write_fails_without_force() {
    let _guard = env_lock();
    let root = TestDir::new("registry-discover-refuse-overwrite");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let _harness = setup_compose_discover_repo(&repo, false);

    let first = run_discover(&repo, &[]);
    assert!(first.status.success(), "first write should succeed");

    let registry_path = repo.join(".wax/compose.registry.json");
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
    let _harness = setup_compose_discover_repo(&repo, false);

    let first = run_discover(&repo, &[]);
    assert!(first.status.success());

    let registry_path = repo.join(".wax/compose.registry.json");
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
    let _harness = setup_compose_discover_repo(&repo, false);

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
    let _harness = setup_compose_discover_repo(&repo, false);

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
    assert!(!repo.join(".wax/compose.registry.json").exists());
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
    assert!(!repo.join(".wax/compose.registry.json").exists());
}
