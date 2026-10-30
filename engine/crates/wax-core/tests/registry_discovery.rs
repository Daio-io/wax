use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use sha2::{Digest, Sha256};
use wax_core::registry_discovery::{
    RegistryDiscoverError, RegistryDiscoverOptions, discover_registry,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn bytes_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRepo {
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
    copy_dir_all(
        &compose_fixture_design_system_dir(),
        &repo.join("design-system"),
    )
    .expect("copy compose fixture");
}

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

fn compose_config_json(roots: &[&str]) -> String {
    let roots_json: Vec<String> = roots.iter().map(|root| format!("\"{root}\"")).collect();
    format!(
        r#"{{
  "schema_version": 2,
  "languages": {{
    "compose": {{
      "roots": [{roots}]
    }}
  }}
}}
"#,
        roots = roots_json.join(", ")
    )
}

fn write_compose_config_with_roots(repo: &Path, roots: &[&str]) {
    let wax_dir = repo.join(".wax");
    fs::create_dir_all(&wax_dir).expect("create .wax directory");
    fs::write(wax_dir.join("wax.config.json"), compose_config_json(roots))
        .expect("write wax config");
}

fn write_compose_config_with_grouped_roots(repo: &Path, roots: &[&str]) {
    let wax_dir = repo.join(".wax");
    fs::create_dir_all(&wax_dir).expect("create .wax directory");
    let roots_json: Vec<String> = roots.iter().map(|root| format!("\"{root}\"")).collect();
    fs::write(
        wax_dir.join("wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "languages": {{
    "compose": {{
      "roots": {{"design-system": [{roots}]}}
    }}
  }}
}}
"#,
            roots = roots_json.join(", ")
        ),
    )
    .expect("write grouped wax config");
}

fn write_config_with_registry_object(repo: &Path, language_id: &str, registry_json: &str) {
    fs::create_dir_all(repo.join(".wax")).expect("create .wax directory");
    fs::write(
        repo.join(".wax/wax.config.json"),
        format!(
            r#"{{
  "schema_version": 2,
  "languages": {{
    "{language_id}": {{
      "registry": {registry_json}
    }}
  }}
}}
"#
        ),
    )
    .expect("write wax config with registry object");
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
  "schema_version": 2,
  "languages": {
    "compose": {
      "roots": ["design-system/src/main/kotlin"]
    },
    "react": {
      "roots": ["design-system/src"]
    }
  }
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

fn install_compose_pack_fixture() -> (PathBuf, EnvVarGuard) {
    install_discover_fixture_pack(
        "compose",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton","SecondaryButton","QualifiedButton"],"diagnostics":[]}"#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
}

fn install_react_discover_fixture_pack() -> (PathBuf, EnvVarGuard) {
    install_discover_fixture_pack(
        "react",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"react","symbols":["Button"],"diagnostics":[]}"#,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
}

fn install_discover_fixture_pack(
    language_id: &str,
    version: &str,
    response_json: &str,
    sha256: &str,
) -> (PathBuf, EnvVarGuard) {
    let wax_home = std::env::temp_dir().join(format!(
        "wax-core-registry-discover-home-{language_id}-{version}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    let install_dir = wax_home.join(format!("langs/{language_id}/{version}"));
    fs::create_dir_all(&install_dir).expect("create language install dir");
    fs::create_dir_all(&wax_home).expect("create wax home");

    let script_path = install_dir.join(format!("wax-lang-{language_id}"));
    fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
request=$(cat)
case "$request" in
  *'"type":"capabilities"'*)
    cat <<'JSON'
{{"type":"capabilities","api_version":1,"language_id":"{language_id}","registry_details_v1":false}}
JSON
    ;;
  *)
    cat <<'JSON'
{response_json}
JSON
    ;;
esac
"#
        ),
    )
    .expect("write fixture language pack script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path)
            .expect("read fixture script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("set fixture script permissions");
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

    fs::write(
        wax_home.join("state.json"),
        format!(
            r#"{{
  "installed_languages": {{
    "{language_id}": {{
      "{version}": {{ "install_dir": "{}" }}
    }}
  }}
}}
"#,
            install_dir.display()
        ),
    )
    .expect("write fixture state file");

    let guard = EnvVarGuard::set("WAX_HOME", &wax_home);
    (wax_home, guard)
}

fn install_rich_discover_fixture_pack() -> (PathBuf, EnvVarGuard) {
    let language_id = "compose";
    let version = "0.1.0";
    let wax_home = std::env::temp_dir().join(format!(
        "wax-core-registry-rich-home-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    let install_dir = wax_home.join(format!("langs/{language_id}/{version}"));
    fs::create_dir_all(&install_dir).expect("create language install dir");
    let script_path = install_dir.join("wax-lang-compose");
    fs::write(
        &script_path,
        r#"#!/bin/sh
request=$(cat)
case "$request" in
  *'"type":"capabilities"'*)
    printf '%s\n' '{"type":"capabilities","api_version":1,"language_id":"compose","registry_details_v1":true}'
    ;;
  *'"type":"discover_registry"'*)
    printf '%s\n' '{"type":"registry_details","api_version":1,"language_id":"compose","document":{"schema_version":1,"components":[{"id":"button","symbol":"Button","metadata":{"sources":[{"kind":"documentation","path":"pack-injected.md"}]},"details":{"status":"partial","documentation":"Generated docs"}},{"id":"card","symbol":"Card","metadata":{"sources":[{"kind":"example","path":"secret.txt"}]},"details":{"status":"complete"}}]},"diagnostics":[]}'
    ;;
  *)
    printf '%s\n' '{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["Button"],"diagnostics":[]}'
    ;;
esac
"#,
    )
    .expect("write rich fixture language pack script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path)
            .expect("read fixture script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("set fixture script permissions");
    }
    fs::write(
        install_dir.join("manifest.json"),
        r#"{
  "id": "compose",
  "version": "0.1.0",
  "api_version": 1,
  "command": ["./wax-lang-compose"],
  "target": "x86_64-unknown-linux-gnu",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}
"#,
    )
    .expect("write rich fixture manifest");
    fs::write(
        wax_home.join("state.json"),
        format!(
            "{}\n",
            serde_json::to_string(&json!({
                "installed_languages": {
                    "compose": {
                        "0.1.0": {"install_dir": install_dir}
                    }
                }
            }))
            .unwrap()
        ),
    )
    .expect("write rich fixture state");
    let guard = EnvVarGuard::set("WAX_HOME", &wax_home);
    (wax_home, guard)
}

fn discover_with_config_roots(
    repo: &Path,
) -> Result<wax_core::registry_discovery::RegistryDiscoverResult, RegistryDiscoverError> {
    discover_registry(RegistryDiscoverOptions {
        repo_root: repo,
        language_id: "compose",
        roots: vec![],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
}

fn discover_with_config_roots_write(
    repo: &Path,
) -> Result<wax_core::registry_discovery::RegistryDiscoverResult, RegistryDiscoverError> {
    discover_registry(RegistryDiscoverOptions {
        repo_root: repo,
        language_id: "compose",
        roots: vec![],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
}

#[test]
fn generated_registry_json_contains_schema_version_1() {
    let registry = dry_run_registry();

    assert_eq!(registry["schema_version"], json!(1));
}

#[test]
fn generated_ids_use_ds_kebab_case_symbol() {
    let registry = dry_run_registry();
    let components = registry["components"].as_array().expect("components array");

    let ids: Vec<&str> = components
        .iter()
        .map(|component| {
            component["id"]
                .as_str()
                .expect("component id should be a string")
        })
        .collect();

    assert_eq!(
        ids,
        vec![
            "ds.primary-button",
            "ds.qualified-button",
            "ds.secondary-button"
        ]
    );
}

#[test]
fn generated_ids_split_acronym_boundaries() {
    let _guard = env_lock();
    let repo = TestRepo::new("registry-discovery-acronym");
    let source_root = repo.path().join("src/main/kotlin");
    fs::create_dir_all(&source_root).expect("create source root");
    fs::write(
        source_root.join("Components.kt"),
        r#"import androidx.compose.runtime.Composable

@Composable
fun XMLButton() {}
"#,
    )
    .expect("write kotlin fixture");
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_discover_fixture_pack(
        "compose",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["XMLButton"],"diagnostics":[]}"#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![source_root],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("dry run should succeed");

    assert_eq!(
        result.registry["components"],
        json!([
            {
                "id": "ds.xml-button",
                "symbol": "XMLButton",
                "details": {"status": "unavailable"}
            }
        ])
    );
}

#[test]
fn conflicting_symbols_with_same_generated_id_are_rejected() {
    let _guard = env_lock();
    let repo = TestRepo::new("registry-discovery-id-collision");
    let source_root = repo.path().join("src/main/kotlin");
    fs::create_dir_all(&source_root).expect("create source root");
    fs::write(
        source_root.join("Components.kt"),
        r#"import androidx.compose.runtime.Composable

@Composable
fun XMLButton() {}

@Composable
fun XmlButton() {}
"#,
    )
    .expect("write kotlin fixture");
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_discover_fixture_pack(
        "compose",
        "0.1.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["XMLButton","XmlButton"],"diagnostics":[]}"#,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![source_root],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect_err("colliding generated ids should fail");

    match err {
        RegistryDiscoverError::IdCollision {
            id,
            first_symbol,
            second_symbol,
        } => {
            assert_eq!(id, "ds.xml-button");
            assert_eq!(first_symbol, "XMLButton");
            assert_eq!(second_symbol, "XmlButton");
        }
        other => panic!("expected id collision error, got {other}"),
    }
}

#[test]
fn output_components_are_sorted() {
    let registry = dry_run_registry();
    let components = registry["components"].as_array().expect("components array");

    let symbols: Vec<&str> = components
        .iter()
        .map(|component| {
            component["symbol"]
                .as_str()
                .expect("component symbol should be a string")
        })
        .collect();

    assert_eq!(
        symbols,
        vec!["PrimaryButton", "QualifiedButton", "SecondaryButton"]
    );
}

#[test]
fn duplicate_symbols_collapse_to_one_component() {
    let registry = dry_run_registry();
    let components = registry["components"].as_array().expect("components array");

    let primary_count = components
        .iter()
        .filter(|component| component["symbol"] == json!("PrimaryButton"))
        .count();

    assert_eq!(primary_count, 1);
    assert_eq!(components.len(), 3);
}

#[test]
fn resolves_roots_from_wax_config_when_roots_omitted() {
    let _guard = env_lock();
    let repo = TestRepo::new("registry-discovery-config-roots");
    link_compose_fixture_into_repo(repo.path());
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("config roots should resolve");

    assert!(result.used_config_roots);
    assert_eq!(
        result.registry["components"]
            .as_array()
            .expect("components array")
            .len(),
        3
    );
}

#[test]
fn resolves_grouped_roots_from_wax_config_when_roots_omitted() {
    let _guard = env_lock();
    let repo = TestRepo::new("registry-discovery-grouped-config-roots");
    link_compose_fixture_into_repo(repo.path());
    write_compose_config_with_grouped_roots(repo.path(), &["design-system/src/main/kotlin"]);
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("grouped config roots should resolve");

    assert!(result.used_config_roots);
    assert_eq!(result.root_count, 1);
}

#[test]
fn missing_configured_roots_fails_with_guidance() {
    let repo = TestRepo::new("registry-discovery-missing-config-roots");
    fs::create_dir_all(repo.path().join(".wax")).expect("create .wax directory");
    fs::write(
        repo.path().join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {"compose": {}}
}
"#,
    )
    .expect("write wax config without roots");

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect_err("missing configured roots should fail");

    let message = err.to_string();
    assert!(message.contains("pass --root path/to/design-system"));
}

#[test]
fn omitted_language_fails_with_guidance() {
    let repo = TestRepo::new("registry-discovery-omitted-language");
    link_compose_fixture_into_repo(repo.path());
    let wax_dir = repo.path().join(".wax");
    fs::create_dir_all(&wax_dir).expect("create .wax directory");
    fs::write(
        wax_dir.join("wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["design-system/src"]
    }
  }
}
"#,
    )
    .expect("write wax config");

    let err = discover_with_config_roots(repo.path())
        .expect_err("omitted language should not resolve roots");

    assert!(matches!(
        err,
        RegistryDiscoverError::LanguageNotConfigured { .. }
    ));
    assert!(
        err.to_string()
            .contains("pass --root path/to/design-system")
    );
}

#[test]
fn absolute_configured_root_is_rejected() {
    let repo = TestRepo::new("registry-discovery-absolute-root");
    link_compose_fixture_into_repo(repo.path());
    let absolute_root = repo.path().join("design-system/src/main/kotlin");
    write_compose_config_with_roots(repo.path(), &[absolute_root.to_str().unwrap()]);

    let err =
        discover_with_config_roots(repo.path()).expect_err("absolute configured root should fail");

    assert!(matches!(err, RegistryDiscoverError::InvalidRootPath { .. }));
}

#[test]
fn parent_dir_configured_root_is_rejected() {
    let repo = TestRepo::new("registry-discovery-parent-dir-root");
    link_compose_fixture_into_repo(repo.path());
    write_compose_config_with_roots(repo.path(), &["../design-system/src/main/kotlin"]);

    let err = discover_with_config_roots(repo.path())
        .expect_err("parent-dir configured root should fail");

    assert!(matches!(err, RegistryDiscoverError::InvalidRootPath { .. }));
}

#[test]
fn missing_configured_root_path_is_rejected() {
    let repo = TestRepo::new("registry-discovery-missing-root-path");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin/missing"]);

    let err = discover_with_config_roots(repo.path())
        .expect_err("missing configured root path should fail");

    assert!(matches!(err, RegistryDiscoverError::RootNotFound { .. }));
}

#[cfg(unix)]
#[test]
fn configured_root_symlink_outside_repo_is_rejected() {
    let repo = TestRepo::new("registry-discovery-symlink-escape");
    let outside = repo.path().parent().expect("temp parent").join(format!(
        "wax-outside-{}",
        repo.path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
    ));
    let _ = fs::remove_dir_all(&outside);
    copy_dir_all(
        &compose_fixture_design_system_dir(),
        &outside.join("design-system"),
    )
    .expect("copy compose fixture outside repo");
    symlink(
        outside.join("design-system"),
        repo.path().join("design-system"),
    )
    .expect("symlink design-system outside repo");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);

    let err = discover_with_config_roots(repo.path()).expect_err("symlink escape should fail");

    assert!(matches!(err, RegistryDiscoverError::RootEscapesRepo { .. }));
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn discover_writes_language_specific_default_registry_path() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-compose-default-path");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let result = discover_with_config_roots_write(repo.path()).expect("discover should succeed");

    assert_eq!(
        result.output_path,
        repo.path().join(".wax/compose.registry.json")
    );
    assert!(!repo.path().join(".wax/wax.registry.json").exists());

    let config_path = repo.path().join(".wax/wax.config.json");
    let config_contents = fs::read_to_string(&config_path).unwrap();
    assert!(config_contents.ends_with('\n'));
    let config: serde_json::Value = serde_json::from_str(&config_contents).unwrap();
    wax_core::config::waxrc::load_waxrc(&config_path).expect("production config reader");
    assert_eq!(
        config["languages"]["compose"]["registry"],
        ".wax/compose.registry.json"
    );

    let lock_path = repo.path().join(".wax/wax.lock.json");
    let lock_contents = fs::read_to_string(&lock_path).unwrap();
    assert!(lock_contents.ends_with('\n'));
    let lock: serde_json::Value = serde_json::from_str(&lock_contents).unwrap();
    wax_core::config::lockfile::load_lockfile(&lock_path).expect("production lockfile reader");
    assert_eq!(
        lock["registries"]["compose"]["source"],
        ".wax/compose.registry.json"
    );
    let written_bytes = fs::read(repo.path().join(".wax/compose.registry.json")).unwrap();
    assert_eq!(
        lock["registries"]["compose"]["sha256"],
        bytes_sha256(&written_bytes)
    );
}

#[test]
fn discover_compose_then_react_writes_both_without_force() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-multi-language");
    write_multi_language_config(repo.path());
    link_compose_fixture_into_repo(repo.path());
    write_multi_language_lockfile(repo.path());
    let (_compose_home, _compose_home_guard) = install_compose_pack_fixture();

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![repo.path().join("design-system/src/main/kotlin")],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("compose discover");

    let (_react_home, _react_home_guard) = install_react_discover_fixture_pack();
    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "react",
        roots: vec![repo.path().join("design-system/src")],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("react discover via fixture pack");

    let compose_path = repo.path().join(".wax/compose.registry.json");
    let react_path = repo.path().join(".wax/react.registry.json");
    assert!(compose_path.is_file());
    assert!(react_path.is_file());

    let compose_registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(compose_path).unwrap()).unwrap();
    let react_registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(react_path).unwrap()).unwrap();
    assert!(
        compose_registry["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["symbol"] == "PrimaryButton")
    );
    assert!(
        react_registry["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["symbol"] == "Button")
    );
}

#[test]
fn discover_rejects_external_registry_source() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-external-registry-source");
    write_config_with_registry_object(
        repo.path(),
        "compose",
        r#"{ "source": "https://example.com/registry.json" }"#,
    );
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let err = discover_with_config_roots_write(repo.path())
        .expect_err("hosted registry source should not be writable by discover");

    assert!(matches!(
        err,
        RegistryDiscoverError::RegistryExternalSource { .. }
    ));
}

#[test]
fn discover_rejects_uppercase_https_and_other_external_schemes() {
    let _guard = env_lock();
    for source in [
        "HTTPS://example.com/registry.json",
        "ftp://example.com/registry.json",
    ] {
        let repo = TestRepo::new("discover-external-registry-scheme");
        write_config_with_registry_object(repo.path(), "compose", &format!(r#""{source}""#));
        link_compose_fixture_into_repo(repo.path());
        write_compose_lockfile(repo.path());
        let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

        let err = discover_with_config_roots_write(repo.path())
            .expect_err("external registry source should not be writable by discover");

        assert!(
            matches!(err, RegistryDiscoverError::RegistryExternalSource { .. }),
            "expected external source rejection for {source}, got {err}"
        );
    }
}

#[test]
fn discover_writes_configured_registry_path() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-registry-key");
    fs::create_dir_all(repo.path().join(".wax")).unwrap();
    fs::write(
        repo.path().join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {
    "compose": {
      "registry": "design-system/registry.json",
      "roots": ["design-system/src/main/kotlin"]
    }
  }
}"#,
    )
    .unwrap();
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let result = discover_with_config_roots_write(repo.path()).expect("registry path discover");

    assert_eq!(
        result.output_path,
        repo.path().join("design-system/registry.json")
    );
}

#[test]
fn discover_without_installed_pack_returns_clear_error() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-missing-pack");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    let wax_home = std::env::temp_dir().join(format!(
        "wax-core-empty-home-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&wax_home).expect("create empty wax home");
    fs::write(
        wax_home.join("state.json"),
        "{\"installed_languages\":{}}\n",
    )
    .expect("write empty wax state");
    let _wax_home_guard = EnvVarGuard::set("WAX_HOME", &wax_home);

    let err = discover_with_config_roots_write(repo.path()).expect_err("missing installed pack");

    assert!(matches!(
        err,
        RegistryDiscoverError::PackNotInstalled { .. }
    ));
}

#[test]
fn unchanged_discover_rerun_succeeds_without_force_and_is_stable() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-same-language-overwrite");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let first = discover_with_config_roots_write(repo.path()).expect("first discover");
    let first_contents = fs::read_to_string(&first.output_path).expect("read first registry");
    let second = discover_with_config_roots_write(repo.path()).expect("second discover");
    let second_contents = fs::read_to_string(&second.output_path).expect("read second registry");

    assert_eq!(second_contents, first_contents);
    assert!(second.changes.is_empty());
}

#[test]
fn dry_run_generates_registry_without_writing_output() {
    let repo = TestRepo::new("registry-discovery-dry-run");
    let _guard = env_lock();
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("dry run should succeed");

    assert_eq!(
        result.output_path,
        repo.path().join(".wax/compose.registry.json")
    );
    assert!(!result.output_path.exists());
}

#[test]
fn default_write_targets_centralized_registry_path() {
    let repo = TestRepo::new("registry-discovery-default-write");
    let _guard = env_lock();
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("write should succeed");

    let expected_path = repo.path().join(".wax/compose.registry.json");
    assert_eq!(result.output_path, expected_path);
    assert!(expected_path.is_file());
}

#[test]
fn rediscovery_without_force_retains_unobserved_components() {
    let repo = TestRepo::new("registry-discovery-safe-refresh");
    let _guard = env_lock();
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());
    let output_path = repo.path().join(".wax/compose.registry.json");
    fs::create_dir_all(output_path.parent().expect("registry parent")).unwrap();
    let original_contents = r#"{"schema_version":1,"components":[{"id":"primary","symbol":"PrimaryButton","metadata":{"description":"Authored"}},{"id":"legacy","symbol":"LegacyControl"}],"tokens":[{"id":"color.brand","key":"color.brand","category":"color"}]}
"#;
    fs::write(&output_path, original_contents).unwrap();

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("existing registry should refresh through lossless merge");

    assert_eq!(
        result.document.components[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("Authored")
    );
    assert_eq!(result.document.tokens.len(), 1);
    assert_eq!(result.document.components[1].id, "legacy");
}

#[test]
fn invalid_existing_registry_failure_keeps_previous_file_recoverable() {
    let repo = TestRepo::new("registry-discovery-invalid-existing-recovery");
    let _guard = env_lock();
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());
    let output_path = repo.path().join(".wax/compose.registry.json");
    fs::create_dir_all(output_path.parent().expect("registry parent"))
        .expect("create registry dir");
    let original_contents = "{\"schema_version\":1,\"components\":[";
    fs::write(&output_path, original_contents).expect("seed invalid registry");

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect_err("invalid existing registry should fail before replacement");

    assert!(matches!(
        err,
        RegistryDiscoverError::InvalidExistingRegistry { .. }
    ));
    assert_eq!(
        fs::read_to_string(output_path).expect("read recoverable registry"),
        original_contents
    );
}

#[test]
fn failed_forced_discovery_leaves_previous_file_unchanged() {
    let repo = TestRepo::new("registry-discovery-force-failure");
    let _guard = env_lock();
    let (wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());
    let script_path = wax_home.join("langs/compose/0.1.0/wax-lang-compose");
    let marker_path = wax_home.join("replacement-discover-started");
    fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
request=$(cat)
case "$request" in
  *'"type":"capabilities"'*)
    printf '%s\n' '{{"type":"capabilities","api_version":1,"language_id":"compose","registry_details_v1":false}}'
    ;;
  *)
    if [ -f '{}' ]; then
      printf '%s\n' 'replacement enrichment failed' >&2
      exit 7
    fi
    touch '{}'
    printf '%s\n' '{{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton"],"diagnostics":[]}}'
    ;;
esac
"#,
            marker_path.display(),
            marker_path.display(),
        ),
    )
    .expect("replace fixture pack script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }
    let output_path = repo.path().join(".wax/compose.registry.json");
    fs::create_dir_all(output_path.parent().expect("registry parent"))
        .expect("create registry dir");
    let original_contents =
        r#"{"schema_version":1,"components":[{"id":"legacy","symbol":"LegacyControl"}]}"#;
    fs::write(&output_path, original_contents).expect("seed registry");

    let error = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: true,
        design_system_id: None,
        design_system_name: None,
    })
    .expect_err("failed replacement enrichment should abort publication");

    assert!(matches!(
        error,
        RegistryDiscoverError::DiscoverSubprocess(_)
    ));
    assert_eq!(
        fs::read_to_string(output_path).expect("read recoverable registry"),
        original_contents
    );
}

#[test]
fn force_prunes_unobserved_components_and_keeps_observed_creator_fields() {
    let repo = TestRepo::new("registry-discovery-force");
    let _guard = env_lock();
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());
    let output_path = repo.path().join(".wax/compose.registry.json");
    fs::create_dir_all(output_path.parent().expect("registry parent")).unwrap();
    fs::write(
        &output_path,
        r#"{"schema_version":1,"publisher":"design-systems","components":[{"id":"primary","symbol":"PrimaryButton","metadata":{"description":"Keep me"}},{"id":"legacy","symbol":"LegacyControl"}],"tokens":[{"id":"color.brand","key":"color.brand","category":"color"}]}"#,
    )
    .unwrap();

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: true,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("force should replace existing registry");

    let written = fs::read_to_string(&output_path).expect("read written registry");
    let written_json: serde_json::Value = serde_json::from_str(&written).expect("valid json");
    assert_eq!(
        written_json,
        json!({
            "schema_version": 1,
            "publisher": "design-systems",
            "components": [
                {
                    "id": "primary",
                    "symbol": "PrimaryButton",
                    "metadata": {"description": "Keep me"},
                    "details": {"status": "unavailable"},
                },
                {
                    "id": "ds.qualified-button",
                    "symbol": "QualifiedButton",
                    "details": {"status": "unavailable"},
                },
                {
                    "id": "ds.secondary-button",
                    "symbol": "SecondaryButton",
                    "details": {"status": "unavailable"},
                },
            ],
            "tokens": [{
                "id": "color.brand",
                "key": "color.brand",
                "category": "color",
            }],
        })
    );
}

#[test]
fn configless_discover_without_lockfile_uses_global_install() {
    let _guard = env_lock();
    let repo = TestRepo::new("configless-discover-no-lockfile-core");
    link_compose_fixture_into_repo(repo.path());
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("configless discover should succeed without lockfile");

    assert!(result.output_path.exists());
    assert!(
        !repo.path().join(".wax/wax.lock.json").exists(),
        "configless discover should not create a lockfile"
    );
    assert!(!result.wax_config_present);
    assert!(!result.lockfile_present);
}

#[test]
fn discover_with_lockfile_does_not_fallback_to_latest_global_install() {
    let _guard = env_lock();
    let repo = TestRepo::new("discover-lockfile-no-global-fallback");
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    let (_wax_home, _wax_home_guard) = install_discover_fixture_pack(
        "compose",
        "0.2.0",
        r#"{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["WrongPack"],"diagnostics":[]}"#,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect_err("lockfile pin should not fall back to a different global install");

    assert!(matches!(
        err,
        RegistryDiscoverError::PackNotInstalled { .. }
    ));
}

#[test]
fn dry_run_returns_losslessly_merged_rich_document_and_structured_changes() {
    let _guard = env_lock();
    let repo = TestRepo::new("registry-discovery-rich-merge");
    link_compose_fixture_into_repo(repo.path());
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    write_compose_lockfile(repo.path());
    fs::create_dir_all(repo.path().join("docs")).expect("create docs directory");
    fs::write(repo.path().join("docs/button.md"), "Offline button docs\n")
        .expect("write attachment source");
    fs::write(repo.path().join("secret.txt"), "must not be read\n")
        .expect("write unassociated file");
    let output_path = repo.path().join(".wax/compose.registry.json");
    fs::write(
        &output_path,
        r#"{
  "schema_version": 1,
  "components": [{
    "id": "button",
    "symbol": "Button",
    "metadata": {
      "description": "Creator description",
      "sources": [{"kind": "documentation", "path": "docs/button.md"}]
    }
  }, {
    "id": "legacy",
    "symbol": "LegacyControl",
    "metadata": {"description": "Keep outside narrow roots"}
  }],
  "tokens": [{
    "id": "color.brand",
    "key": "color.brand",
    "category": "color",
    "metadata": {"description": "Creator token"}
  }]
}
"#,
    )
    .expect("write authored registry");
    let (_wax_home, _wax_home_guard) = install_rich_discover_fixture_pack();

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![repo.path().join("design-system/src/main/kotlin")],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("rich dry run should merge");

    assert_eq!(
        result.document.components[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("Creator description")
    );
    assert_eq!(result.document.components.len(), 3);
    assert_eq!(
        result.document.components[1]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.description.as_deref()),
        Some("Keep outside narrow roots")
    );
    assert_eq!(result.document.tokens.len(), 1);
    let card = result
        .document
        .components
        .iter()
        .find(|component| component.id == "card")
        .expect("new rich component");
    assert!(card.metadata.is_none());
    assert!(
        card.details
            .as_ref()
            .expect("rich card extraction")
            .attachments
            .is_empty()
    );
    assert_eq!(
        result.document.components[0]
            .details
            .as_ref()
            .expect("rich extraction")
            .attachments[0]
            .content,
        "Offline button docs\n"
    );
    assert!(result.changes.iter().any(|change| change.id == "card"));
    assert_eq!(
        fs::read_to_string(output_path).expect("dry run preserves source registry"),
        r#"{
  "schema_version": 1,
  "components": [{
    "id": "button",
    "symbol": "Button",
    "metadata": {
      "description": "Creator description",
      "sources": [{"kind": "documentation", "path": "docs/button.md"}]
    }
  }, {
    "id": "legacy",
    "symbol": "LegacyControl",
    "metadata": {"description": "Keep outside narrow roots"}
  }],
  "tokens": [{
    "id": "color.brand",
    "key": "color.brand",
    "category": "color",
    "metadata": {"description": "Creator token"}
  }]
}
"#
    );
}

#[cfg(unix)]
#[test]
fn attachment_failure_leaves_existing_registry_recoverable() {
    let _guard = env_lock();
    let repo = TestRepo::new("registry-discovery-attachment-recovery");
    link_compose_fixture_into_repo(repo.path());
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    write_compose_lockfile(repo.path());
    fs::create_dir_all(repo.path().join("docs")).expect("create docs directory");
    let outside_path = repo.path().with_extension("outside.md");
    fs::write(&outside_path, "outside\n").expect("write outside file");
    symlink(&outside_path, repo.path().join("docs/button.md"))
        .expect("create escaping attachment symlink");
    let output_path = repo.path().join(".wax/compose.registry.json");
    let original_contents = r#"{"schema_version":1,"components":[{"id":"button","symbol":"Button","metadata":{"description":"Recover me","sources":[{"kind":"documentation","path":"docs/button.md"}]}}]}
"#;
    fs::write(&output_path, original_contents).expect("write authored registry");
    let (_wax_home, _wax_home_guard) = install_rich_discover_fixture_pack();

    let error = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![],
        dry_run: false,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect_err("unsafe attachment should abort publication");

    assert!(matches!(error, RegistryDiscoverError::Attachment(_)));
    assert_eq!(
        fs::read_to_string(output_path).expect("read recoverable registry"),
        original_contents
    );
    fs::remove_file(outside_path).expect("remove outside file");
}

fn dry_run_registry() -> serde_json::Value {
    let repo = TestRepo::new("registry-discovery-dry-run-shared");
    let _guard = env_lock();
    let (_wax_home, _wax_home_guard) = install_compose_pack_fixture();
    write_compose_lockfile(repo.path());

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: true,
        force: false,
        design_system_id: None,
        design_system_name: None,
    })
    .expect("dry run should succeed")
    .registry
}
