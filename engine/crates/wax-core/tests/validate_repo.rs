use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::Digest;
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
fn validate_repo_warns_when_file_registry_components_key_missing() {
    let root = TestDir::new("validate-repo-warning-file-missing-components");
    let outside_registry = root.path.with_extension("outside-registry.json");
    fs::write(
        &outside_registry,
        r#"{
  "schema_version": 1
}
"#,
    )
    .unwrap();
    let source = format!("file://{}", outside_registry.display());
    write_repo_with_registry_path(&root.path, &source);
    write_lockfile_with_registry_and_sha256(&root.path, &source, &file_sha256(&outside_registry));

    let report = validate_repo(&root.path).expect("missing components should warn");

    assert!(matches!(
        &report.warnings[..],
        [ValidateWarning::EmptyRegistryComponents { language_id, registry_path }]
        if language_id.as_str() == "compose"
            && registry_path.starts_with(".wax/cache/registries/compose-")
            && root.path.join(registry_path).is_file()
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

    fs::remove_file(root.path.join(".wax/wax.lock.json")).expect("lockfile should exist");

    let err = validate_repo(&root.path).expect_err("missing lockfile should fail");
    assert!(matches!(err, ValidateError::Lockfile(_)));
}

#[test]
fn validate_repo_rejects_missing_per_language_registry_file() {
    let root = TestDir::new("validate-repo-missing-default-registry");
    fs::create_dir_all(root.path.join(".wax")).unwrap();
    fs::write(
        root.path.join(".wax/wax.config.json"),
        r#"{
  "schema_version": 2,
  "languages": {"compose": {}}
}"#,
    )
    .unwrap();
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("missing per-language registry should fail");
    assert!(matches!(
        err,
        ValidateError::RegistrySource {
            source: wax_core::registry_source::RegistrySourceError::Read { .. },
            ..
        }
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
        ValidateError::RegistrySource {
            source: wax_core::registry_source::RegistrySourceError::PlainAbsolutePath { .. },
            ..
        }
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
        ValidateError::RegistrySource {
            source: wax_core::registry_source::RegistrySourceError::PathEscapesRepo { .. },
            ..
        }
    ));
}

#[test]
fn validate_repo_rejects_missing_registry_file() {
    let root = TestDir::new("validate-repo-missing-registry-file");
    write_repo_with_registry_path(&root.path, "design-system/missing.json");
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("missing registry file should fail");
    assert!(matches!(
        err,
        ValidateError::RegistrySource {
            source: wax_core::registry_source::RegistrySourceError::Read { .. },
            ..
        }
    ));
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
    assert!(matches!(
        err,
        ValidateError::RegistrySource {
            source: wax_core::registry_source::RegistrySourceError::PathEscapesRepo { .. },
            ..
        }
    ));
}

#[test]
fn validate_repo_accepts_default_centralized_registry() {
    let root = TestDir::new("validate-repo-default-centralized-registry");
    fs::create_dir_all(root.path.join(".wax")).unwrap();
    fs::write(
        root.path.join(".wax/wax.config.json"),
        r#"{"schema_version": 2,"languages":{"compose": {}}}"#,
    )
    .unwrap();
    fs::write(
        root.path.join(".wax/compose.registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    write_lockfile_with_registry(&root.path, ".wax/compose.registry.json");

    let report = validate_repo(&root.path).unwrap();

    assert!(report.warnings.is_empty());
}

#[test]
fn validate_repo_rejects_missing_registry_lock() {
    let root = TestDir::new("validate-repo-missing-registry-lock");
    fs::create_dir_all(root.path.join("design-system")).unwrap();
    fs::write(
        root.path.join("design-system/registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    write_repo_with_registry_path(&root.path, "design-system/registry.json");
    write_lockfile(&root.path);

    let err = validate_repo(&root.path).expect_err("missing registry lock should fail");

    assert!(
        matches!(err, ValidateError::MissingRegistryLock { language_id } if language_id.as_str() == "compose")
    );
}

#[test]
fn validate_repo_rejects_registry_source_drift() {
    let root = TestDir::new("validate-repo-registry-source-drift");
    write_repo_with_registry_json(
        &root.path,
        "design-system/registry.json",
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    );
    let registry_sha256 = {
        use sha2::{Digest, Sha256};
        let bytes = fs::read(root.path.join("design-system/registry.json")).unwrap();
        Sha256::digest(bytes)
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    };
    write_lockfile_with_registry_and_sha256(&root.path, "legacy/registry.json", &registry_sha256);

    let err = validate_repo(&root.path).expect_err("registry source drift should fail");

    assert!(matches!(
        err,
        ValidateError::RegistrySourceDrift {
            language_id,
            lockfile_source,
            resolved_source,
        } if language_id.as_str() == "compose"
            && lockfile_source == "legacy/registry.json"
            && resolved_source == "design-system/registry.json"
    ));
}

#[test]
fn validate_repo_rejects_registry_digest_drift() {
    let root = TestDir::new("validate-repo-registry-digest-drift");
    write_repo_with_registry_json(
        &root.path,
        "design-system/registry.json",
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    );
    write_lockfile_with_registry_and_sha256(
        &root.path,
        "design-system/registry.json",
        "2222222222222222222222222222222222222222222222222222222222222222",
    );

    let err = validate_repo(&root.path).expect_err("registry digest drift should fail");

    assert!(matches!(
        err,
        ValidateError::RegistryDigestDrift {
            language_id,
            lockfile_sha256,
            resolved_sha256,
        } if language_id.as_str() == "compose"
            && lockfile_sha256 == "2222222222222222222222222222222222222222222222222222222222222222"
            && resolved_sha256.len() == 64
    ));
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
    write_lockfile_with_registry(repo_root, registry_path);
}

fn write_repo_with_registry_path(repo_root: &Path, registry_path: &str) {
    fs::create_dir_all(repo_root.join(".wax")).unwrap();
    fs::write(
        repo_root.join(".wax/wax.config.json"),
        format!(
            "{{\n  \"schema_version\": 2,\n  \"languages\": {{\"compose\": {{\"registry\": \"{registry_path}\"}}}}\n}}\n"
        ),
    )
    .unwrap();
}

fn write_lockfile(repo_root: &Path) {
    fs::create_dir_all(repo_root.join(".wax")).unwrap();
    fs::write(
        repo_root.join(".wax/wax.lock.json"),
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
}

fn write_lockfile_with_registry(repo_root: &Path, source: &str) {
    fs::create_dir_all(repo_root.join(".wax")).unwrap();
    fs::write(
        repo_root.join(".wax/wax.lock.json"),
        lockfile_json(repo_root, source),
    )
    .unwrap();
}

fn write_lockfile_with_registry_and_sha256(repo_root: &Path, source: &str, sha256: &str) {
    fs::create_dir_all(repo_root.join(".wax")).unwrap();
    fs::write(
        repo_root.join(".wax/wax.lock.json"),
        lockfile_json_with_sha256(source, sha256),
    )
    .unwrap();
}

fn lockfile_json(repo_root: &Path, source: &str) -> String {
    let sha256 = repo_registry_sha256(repo_root, source);
    lockfile_json_with_sha256(source, &sha256)
}

fn lockfile_json_with_sha256(source: &str, sha256: &str) -> String {
    format!(
        r#"{{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "locked_at": null,
  "registries": {{
    "compose": {{
      "source": "{source}",
      "sha256": "{}"
    }}
  }},
  "languages": {{
    "compose": {{
      "version": "0.1.0-alpha.0",
      "api_version": 1,
      "source": "file:///tmp/index.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "file:///tmp/wax-lang-compose.tar.gz",
        "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "signature": null
      }}
    }}
  }}
}}"#,
        sha256
    )
}

fn repo_registry_sha256(repo_root: &Path, source: &str) -> String {
    let bytes = fs::read(repo_root.join(source)).unwrap();
    bytes_sha256(&bytes)
}

fn file_sha256(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    bytes_sha256(&bytes)
}

fn bytes_sha256(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
