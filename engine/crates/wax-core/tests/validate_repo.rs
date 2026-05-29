use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs as unix_fs;
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
fn validate_repo_accepts_populated_components_without_warnings() {
    let root = TestDir::new("validate-repo-valid-populated");
    write_valid_repo(
        &root.path,
        "design-system/registry.json",
        r#"[
    {
      "canonical_name": "Button",
      "aliases": [],
      "kind": "component",
      "props": [],
      "slots": [],
      "events": []
    }
  ]"#,
    );

    let report = validate_repo(&root.path).expect("valid repo should pass");
    assert!(report.warnings.is_empty());
}

#[test]
fn validate_repo_warns_when_components_array_empty() {
    let root = TestDir::new("validate-repo-warning-empty-components");
    write_valid_repo(&root.path, "design-system/registry.json", "[]");

    let report = validate_repo(&root.path).expect("empty components should warn");
    assert!(matches!(
        &report.warnings[..],
        [ValidateWarning::EmptyRegistryComponents { language_id, registry_path }]
        if language_id.as_str() == "compose" && registry_path == "design-system/registry.json"
    ));
}

#[test]
fn validate_repo_warns_when_components_key_missing() {
    let root = TestDir::new("validate-repo-warning-missing-components");
    write_repo_with_registry_json(
        &root.path,
        "design-system/registry.json",
        r#"{
  "schema_version": 1
}
"#,
    );

    let report = validate_repo(&root.path).expect("missing components should warn");
    assert!(matches!(
        &report.warnings[..],
        [ValidateWarning::EmptyRegistryComponents { language_id, registry_path }]
        if language_id.as_str() == "compose" && registry_path == "design-system/registry.json"
    ));
}

#[test]
fn validate_repo_requires_lockfile_when_language_enabled() {
    let root = TestDir::new("validate-repo-missing-lockfile");
    write_repo_with_registry_json(
        &root.path,
        "design-system/registry.json",
        r#"{
  "schema_version": 1,
  "components": []
}
"#,
    );

    fs::remove_file(root.path.join("wax.lock.json")).expect("lockfile should exist");

    let err = validate_repo(&root.path).expect_err("missing lockfile should fail");
    assert!(matches!(err, ValidateError::Lockfile(_)));
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

#[test]
fn validate_repo_rejects_absolute_registry_path() {
    let root = TestDir::new("validate-repo-absolute-path");
    let absolute = root.path.join("design-system/registry.json");
    write_repo_with_registry_path(&root.path, absolute.to_string_lossy().as_ref());
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("absolute path should fail");
    assert!(matches!(
        err,
        ValidateError::InvalidDesignSystemRegistryPath { .. }
    ));
}

#[test]
fn validate_repo_rejects_parent_dir_registry_path() {
    let root = TestDir::new("validate-repo-parent-dir-path");
    write_repo_with_registry_path(&root.path, "../registry.json");
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("parent dir path should fail");
    assert!(matches!(
        err,
        ValidateError::InvalidDesignSystemRegistryPath { .. }
    ));
}

#[test]
fn validate_repo_rejects_missing_registry_file() {
    let root = TestDir::new("validate-repo-missing-registry-file");
    write_repo_with_registry_path(&root.path, "design-system/missing.json");
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("missing registry file should fail");
    assert!(matches!(err, ValidateError::RegistryRead { .. }));
}

#[cfg(unix)]
#[test]
fn validate_repo_rejects_symlink_registry_that_escapes_repo_root() {
    let root = TestDir::new("validate-repo-symlink-escape");
    let outside_registry = std::env::temp_dir().join(format!(
        "wax-outside-registry-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    ));
    fs::write(
        &outside_registry,
        r#"{
  "schema_version": 1,
  "components": []
}
"#,
    )
    .unwrap();

    let registry_link = root.path.join("design-system/registry.json");
    fs::create_dir_all(registry_link.parent().unwrap()).unwrap();
    unix_fs::symlink(&outside_registry, &registry_link).unwrap();

    write_repo_with_registry_path(&root.path, "design-system/registry.json");
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("symlink escape should fail");
    let _ = fs::remove_file(outside_registry);
    assert!(matches!(err, ValidateError::RegistryPathEscapesRepo { .. }));
}

fn write_valid_repo(repo_root: &Path, registry_path: &str, components: &str) {
    write_repo_with_registry_json(
        repo_root,
        registry_path,
        &format!("{{\n  \"schema_version\": 1,\n  \"components\": {components}\n}}\n"),
    );
}

fn write_repo_with_registry_json(repo_root: &Path, registry_path: &str, registry_json: &str) {
    let registry_abs = repo_root.join(registry_path);
    if let Some(parent) = registry_abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&registry_abs, registry_json).unwrap();

    write_repo_with_registry_path(repo_root, registry_path);
    write_lockfile(repo_root);
}

fn write_repo_with_registry_path(repo_root: &Path, registry_path: &str) {
    fs::write(
        repo_root.join(".waxrc"),
        format!(
            "{{\n  \"schema_version\": 1,\n  \"languages\": [{{\"id\":\"compose\",\"enabled\":true,\"design_system_registry\":\"{registry_path}\"}}]\n}}\n"
        ),
    )
    .unwrap();
}

fn write_lockfile(repo_root: &Path) {
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
