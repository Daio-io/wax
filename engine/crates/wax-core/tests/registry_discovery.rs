use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use wax_core::registry_discovery::{
    RegistryDiscoverError, RegistryDiscoverOptions, discover_registry,
};

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

fn compose_config_json(enabled: bool, roots: &[&str]) -> String {
    let roots_json: Vec<String> = roots.iter().map(|root| format!("\"{root}\"")).collect();
    format!(
        r#"{{
  "schema_version": 1,
  "languages": [
    {{
      "id": "compose",
      "enabled": {enabled},
      "roots": [{roots}]
    }}
  ]
}}
"#,
        enabled = enabled,
        roots = roots_json.join(", ")
    )
}

fn write_compose_config_with_roots(repo: &Path, roots: &[&str]) {
    let wax_dir = repo.join(".wax");
    fs::create_dir_all(&wax_dir).expect("create .wax directory");
    fs::write(
        wax_dir.join("wax.config.json"),
        compose_config_json(true, roots),
    )
    .expect("write wax config");
}

fn write_legacy_waxrc_with_roots(repo: &Path, roots: &[&str]) {
    fs::write(repo.join(".waxrc"), compose_config_json(true, roots)).expect("write legacy waxrc");
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

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![source_root],
        dry_run: true,
        force: false,
    })
    .expect("dry run should succeed");

    assert_eq!(
        result.registry["components"],
        json!([
            {
                "id": "ds.xml-button",
                "symbol": "XMLButton"
            }
        ])
    );
}

#[test]
fn conflicting_symbols_with_same_generated_id_are_rejected() {
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

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![source_root],
        dry_run: true,
        force: false,
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
    let repo = TestRepo::new("registry-discovery-config-roots");
    link_compose_fixture_into_repo(repo.path());
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![],
        dry_run: true,
        force: false,
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
fn missing_configured_roots_fails_with_guidance() {
    let repo = TestRepo::new("registry-discovery-missing-config-roots");
    fs::create_dir_all(repo.path().join(".wax")).expect("create .wax directory");
    fs::write(
        repo.path().join(".wax/wax.config.json"),
        r#"{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true
    }
  ]
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
    })
    .expect_err("missing configured roots should fail");

    let message = err.to_string();
    assert!(message.contains("pass --root path/to/design-system"));
}

#[test]
fn resolves_roots_from_legacy_waxrc_when_preferred_config_missing() {
    let repo = TestRepo::new("registry-discovery-legacy-waxrc");
    link_compose_fixture_into_repo(repo.path());
    write_legacy_waxrc_with_roots(repo.path(), &["design-system/src/main/kotlin"]);

    let result =
        discover_with_config_roots(repo.path()).expect("legacy waxrc roots should resolve");

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
fn disabled_language_fails_with_guidance() {
    let repo = TestRepo::new("registry-discovery-disabled-language");
    link_compose_fixture_into_repo(repo.path());
    let wax_dir = repo.path().join(".wax");
    fs::create_dir_all(&wax_dir).expect("create .wax directory");
    fs::write(
        wax_dir.join("wax.config.json"),
        compose_config_json(false, &["design-system/src/main/kotlin"]),
    )
    .expect("write wax config");

    let err = discover_with_config_roots(repo.path())
        .expect_err("disabled language should not resolve roots");

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
fn dry_run_generates_registry_without_writing_output() {
    let repo = TestRepo::new("registry-discovery-dry-run");

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: true,
        force: false,
    })
    .expect("dry run should succeed");

    assert_eq!(
        result.output_path,
        repo.path().join(".wax/wax.registry.json")
    );
    assert!(!result.output_path.exists());
}

#[test]
fn default_write_targets_centralized_registry_path() {
    let repo = TestRepo::new("registry-discovery-default-write");

    let result = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
    })
    .expect("write should succeed");

    let expected_path = repo.path().join(".wax/wax.registry.json");
    assert_eq!(result.output_path, expected_path);
    assert!(expected_path.is_file());
}

#[test]
fn existing_registry_refuses_overwrite_without_force() {
    let repo = TestRepo::new("registry-discovery-refuse-overwrite");
    let output_path = repo.path().join(".wax/wax.registry.json");
    fs::create_dir_all(output_path.parent().expect("registry parent")).unwrap();
    let original_contents = "{\"schema_version\":1,\"components\":[]}\n";
    fs::write(&output_path, original_contents).unwrap();

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
    })
    .expect_err("existing registry should block overwrite");

    assert!(matches!(err, RegistryDiscoverError::OutputExists { .. }));
    let message = err.to_string();
    assert!(message.contains("--force"));
    assert!(message.contains("--dry-run"));
    assert_eq!(
        fs::read_to_string(&output_path).expect("read existing registry"),
        original_contents
    );
}

#[test]
#[cfg(unix)]
fn existing_registry_refuses_overwrite_before_temp_creation_failures() {
    let repo = TestRepo::new("registry-discovery-refuse-overwrite-preflight");
    let wax_dir = repo.path().join(".wax");
    let output_path = wax_dir.join("wax.registry.json");
    fs::create_dir_all(&wax_dir).expect("create registry dir");
    fs::write(&output_path, "{\"schema_version\":1,\"components\":[]}\n").expect("seed registry");

    let original_permissions = fs::metadata(&wax_dir)
        .expect("read dir metadata")
        .permissions();
    let mut read_only_permissions = original_permissions.clone();
    read_only_permissions.set_mode(0o555);
    fs::set_permissions(&wax_dir, read_only_permissions).expect("make registry dir read-only");

    let err = discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: false,
    })
    .expect_err("existing registry should be refused before temp writes");

    fs::set_permissions(&wax_dir, original_permissions).expect("restore registry dir permissions");

    assert!(matches!(err, RegistryDiscoverError::OutputExists { .. }));
    let message = err.to_string();
    assert!(message.contains("--force"));
    assert!(message.contains("--dry-run"));
}

#[test]
fn force_replaces_existing_registry() {
    let repo = TestRepo::new("registry-discovery-force");
    let output_path = repo.path().join(".wax/wax.registry.json");
    fs::create_dir_all(output_path.parent().expect("registry parent")).unwrap();
    fs::write(&output_path, "{\"schema_version\":1,\"components\":[]}").unwrap();

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: false,
        force: true,
    })
    .expect("force should replace existing registry");

    let written = fs::read_to_string(&output_path).expect("read written registry");
    let written_json: serde_json::Value = serde_json::from_str(&written).expect("valid json");
    assert_eq!(written_json["schema_version"], json!(1));
    assert_eq!(
        written_json["components"][0]["id"],
        json!("ds.primary-button")
    );
}

fn dry_run_registry() -> serde_json::Value {
    let repo = TestRepo::new("registry-discovery-dry-run-shared");

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![compose_fixture_root()],
        dry_run: true,
        force: false,
    })
    .expect("dry run should succeed")
    .registry
}
