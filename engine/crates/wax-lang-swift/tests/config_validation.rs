use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{LanguageId, ScanFacts, ScanStatus};
use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_swift::{SwiftLanguage, SwiftScanError};

fn request(repo_root: &Path, config: ScanConfig) -> ScanRequest {
    ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("swift").expect("swift id must be valid"),
        repo_root: repo_root.to_string_lossy().to_string(),
        snapshot_id: "snap-swift-config".to_owned(),
        config,
    }
}

#[test]
fn configured_scan_requires_registry_and_roots() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut missing_registry = ScanConfig::new();
    missing_registry.insert("roots".to_owned(), serde_json::json!(["Sources"]));

    let err = SwiftLanguage::new()
        .scan(&request(tempdir.path(), missing_registry))
        .expect_err("missing registry should fail");
    assert!(err.to_string().contains("registry is required"));

    let mut missing_roots = ScanConfig::new();
    missing_roots.insert(
        "registry".to_owned(),
        serde_json::json!("design-system/registry.json"),
    );

    let err = SwiftLanguage::new()
        .scan(&request(tempdir.path(), missing_roots))
        .expect_err("missing roots should fail");
    assert!(err.to_string().contains("roots is required"));
}

#[test]
fn configured_scan_rejects_parent_directory_registry() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = ScanConfig::new();
    config.insert("registry".to_owned(), serde_json::json!("../registry.json"));
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));

    let err = SwiftLanguage::new()
        .scan(&request(tempdir.path(), config))
        .expect_err("parent registry should fail");

    assert!(err.to_string().contains("parent directory"));
}

#[test]
fn configured_scan_loads_registry_and_reports_missing_root_as_partial() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(tempdir.path().join(".wax")).expect("create .wax");
    fs::write(
        tempdir.path().join(".wax/swift.registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton","targets":["swift"]}]}"#,
    )
    .expect("write registry");

    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::json!(".wax/swift.registry.json"),
    );
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));

    let facts = SwiftLanguage::new()
        .scan(&request(tempdir.path(), config))
        .expect("scan should return partial facts");

    assert_eq!(facts.status, ScanStatus::Partial);
    assert_eq!(facts.design_system_components.len(), 1);
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "root_not_found")
    );
}

#[test]
fn registry_key_is_accepted_as_canonical_registry_path() {
    let mut config = valid_fixture_config();
    let registry = config
        .remove("design_system_registry")
        .expect("fixture key");
    config.insert("registry".to_owned(), registry);

    let facts = scan_fixture_with_config(config).expect("registry key should scan");

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.counts.registry.component_count, 2);
}

#[test]
fn design_system_registry_key_still_scans() {
    let facts = scan_fixture_with_config(valid_fixture_config()).expect("legacy key should scan");

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.counts.registry.component_count, 2);
}

#[test]
fn registry_key_wins_when_both_registry_keys_are_present() {
    let mut config = valid_fixture_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("alt-design-system/registry.json".to_owned()),
    );

    let facts = scan_fixture_with_config(config).expect("canonical registry key should win");

    assert_eq!(facts.counts.registry.component_count, 2);
}

#[test]
fn empty_roots_array_is_config_error_not_scaffold() {
    let mut config = ScanConfig::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!([]));

    let err = scan_fixture_with_config(config).expect_err("empty roots must fail");
    assert_config_error(err, "roots must be a non-empty array of strings");
}

#[test]
fn non_string_root_entry_is_config_error() {
    let mut config = ScanConfig::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!([42]));

    let err = scan_fixture_with_config(config).expect_err("non-string roots must fail");
    assert_config_error(err, "roots[0] must be a non-empty string");
}

#[test]
fn roots_without_registry_is_config_error() {
    let mut config = ScanConfig::new();
    config.insert("roots".to_owned(), serde_json::json!(["app/Sources/App"]));

    let err = scan_fixture_with_config(config).expect_err("roots without registry must fail");
    assert_config_error(err, "registry is required");
}

#[test]
fn absolute_registry_path_is_config_error() {
    let mut config = valid_fixture_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("/tmp/registry.json".to_owned()),
    );

    let err = scan_fixture_with_config(config).expect_err("absolute registry path must fail");
    assert_config_error(err, "repo-relative path");
}

#[test]
fn configured_scan_reports_parse_failed_for_invalid_source() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let repo_root = tempdir.path();
    fs::create_dir_all(repo_root.join("design-system")).expect("create registry dir");
    fs::create_dir_all(repo_root.join("Sources/App")).expect("create sources");
    fs::write(
        repo_root.join("design-system/registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton","targets":["swift"]}]}"#,
    )
    .expect("write registry");
    fs::write(
        repo_root.join("Sources/App/Valid.swift"),
        "import SwiftUI\nstruct ValidView: View { var body: some View { Text(\"ok\") } }\n",
    )
    .expect("write valid swift");
    fs::write(
        repo_root.join("Sources/App/Broken.swift"),
        "struct BrokenView {",
    )
    .expect("write broken swift");

    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::json!("design-system/registry.json"),
    );
    config.insert("roots".to_owned(), serde_json::json!(["Sources/App"]));

    let facts = SwiftLanguage::new()
        .scan(&request(repo_root, config))
        .expect("configured scan should still return facts");

    assert_eq!(facts.status, ScanStatus::Partial);
    assert_eq!(facts.metrics.files_scanned, 2);
    assert_eq!(facts.local_components.len(), 1);
    assert_eq!(facts.local_components[0].symbol, "ValidView");
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "parse_failed"),
        "partial trees with syntax errors must emit parse_failed: {:?}",
        facts.diagnostics
    );
}

fn scan_fixture_with_config(config: ScanConfig) -> Result<ScanFacts, SwiftScanError> {
    let fixture_root = fixture_root();
    SwiftLanguage::new().scan(&request(&fixture_root, config))
}

fn assert_config_error(err: SwiftScanError, expected_substring: &str) {
    match err {
        SwiftScanError::InvalidConfig(message) => {
            assert!(
                message.contains(expected_substring),
                "expected `{expected_substring}` in `{message}`"
            );
        }
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}

fn valid_fixture_config() -> ScanConfig {
    let mut config = ScanConfig::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/Sources/App"]));
    config
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small")
}
