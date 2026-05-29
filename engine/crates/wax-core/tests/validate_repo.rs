use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use wax_core::validate::{ValidateError, ValidateWarning, validate_repo};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-core-{name}-{nonce}"));
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
fn validate_repo_accepts_valid_repo() {
    let root = TestDir::new("validate-repo-valid");
    write_valid_repo(&root.path, "design-system/registry.json", "[]");

    let report = validate_repo(&root.path).expect("valid repo should pass");
    assert_eq!(report.warnings.len(), 1);
    assert!(matches!(
        &report.warnings[0],
        ValidateWarning::EmptyRegistryComponents { language_id, .. }
            if language_id.as_str() == "compose"
    ));
}

#[test]
fn validate_repo_warns_when_components_array_empty() {
    let root = TestDir::new("validate-repo-warning");
    write_valid_repo(&root.path, "design-system/registry.json", "[]");

    let report = validate_repo(&root.path).expect("empty components should warn");
    assert!(matches!(
        &report.warnings[..],
        [ValidateWarning::EmptyRegistryComponents { language_id, registry_path }]
        if language_id.as_str() == "compose" && registry_path == "design-system/registry.json"
    ));
}

#[test]
fn validate_repo_rejects_missing_design_system_registry() {
    let root = TestDir::new("validate-repo-missing-registry-field");
    fs::write(
        root.path.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [{"id":"compose","enabled":true}]
}"#,
    )
    .unwrap();
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("missing registry field should fail");
    assert!(matches!(
        err,
        ValidateError::MissingDesignSystemRegistry { .. }
    ));
}

#[test]
fn validate_repo_rejects_duplicate_enabled_language_ids() {
    let root = TestDir::new("validate-repo-duplicate-language");
    fs::create_dir_all(root.path.join("design-system")).unwrap();
    fs::write(
        root.path.join("design-system/registry.json"),
        r#"{
  "schema_version": 1,
  "components": []
}"#,
    )
    .unwrap();
    fs::write(
        root.path.join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [
    {"id":"compose","enabled":true,"design_system_registry":"design-system/registry.json"},
    {"id":"compose","enabled":true,"design_system_registry":"design-system/registry.json"}
  ]
}"#,
    )
    .unwrap();
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("duplicate language ids should fail");
    assert!(matches!(
        err,
        ValidateError::DuplicateEnabledLanguageId { .. }
    ));
}

fn write_valid_repo(repo_root: &std::path::Path, registry_path: &str, components: &str) {
    let registry_abs = repo_root.join(registry_path);
    if let Some(parent) = registry_abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        &registry_abs,
        format!("{{\n  \"schema_version\": 1,\n  \"components\": {components}\n}}\n"),
    )
    .unwrap();

    fs::write(
        repo_root.join(".waxrc"),
        format!(
            "{{\n  \"schema_version\": 1,\n  \"languages\": [{{\"id\":\"compose\",\"enabled\":true,\"design_system_registry\":\"{registry_path}\"}}]\n}}\n"
        ),
    )
    .unwrap();
    write_lockfile(repo_root);
}

fn write_lockfile(repo_root: &std::path::Path) {
    fs::write(
        repo_root.join("wax.lock.json"),
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
}
