use std::fs;

use wax_contract::{LanguageId, ScanFacts};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_compose::{ComposeLanguage, ComposeScanError};

#[test]
fn registry_key_is_accepted_as_canonical_registry_path() {
    let mut config = valid_config();
    let registry = config.remove("design_system_registry").unwrap();
    config.insert("registry".to_owned(), registry);

    let facts = scan_with_config(config).expect("registry key should scan");

    assert_eq!(facts.counts.registry.component_count, 2);
}

#[test]
fn design_system_registry_key_still_scans() {
    let facts = scan_with_config(valid_config()).expect("legacy registry key should still scan");

    assert_eq!(facts.counts.registry.component_count, 2);
}

#[test]
fn registry_key_wins_when_both_registry_keys_are_present() {
    let mut config = valid_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("alt-design-system/registry.json".to_owned()),
    );

    let facts = scan_with_config(config).expect("canonical registry key should win");

    assert_eq!(facts.counts.registry.component_count, 1);
}

#[test]
fn empty_roots_array_is_config_error_not_scaffold() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!([]));

    let err = scan_with_config(config).expect_err("empty roots must fail");
    assert_config_error(err);
}

#[test]
fn non_string_root_entry_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!([123]));

    let err = scan_with_config(config).expect_err("non-string roots must fail");
    assert_config_error(err);
}

#[test]
fn roots_without_registry_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );

    let err = scan_with_config(config).expect_err("roots without registry must fail");
    assert_config_error(err);
}

#[test]
fn absolute_registry_path_is_config_error() {
    let mut config = valid_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("/etc/passwd".to_owned()),
    );

    let err = scan_with_config(config).expect_err("absolute registry path must fail");
    assert_config_error(err);
}

#[test]
fn parent_dir_in_registry_path_is_config_error() {
    let mut config = valid_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("../outside/registry.json".to_owned()),
    );

    let err = scan_with_config(config).expect_err("parent-dir registry path must fail");
    assert_config_error(err);
}

#[test]
fn absolute_root_path_is_config_error() {
    let mut config = valid_config();
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["/tmp/src/main/kotlin"]),
    );

    let err = scan_with_config(config).expect_err("absolute root path must fail");
    assert_config_error(err);
}

#[test]
fn parent_dir_in_root_path_is_config_error() {
    let mut config = valid_config();
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["../app/src/main/kotlin"]),
    );

    let err = scan_with_config(config).expect_err("parent-dir root path must fail");
    assert_config_error(err);
}

#[test]
fn tree_sitter_scan_rejects_non_array_excludes() {
    let mut config = valid_config();
    config.insert("excludes".to_owned(), serde_json::json!(42));

    let err = scan_with_config(config).expect_err("invalid excludes must fail");
    assert_invalid_excludes(err, "excludes must be an array");
}

#[test]
fn tree_sitter_scan_rejects_empty_excludes_entry() {
    let mut config = valid_config();
    config.insert("excludes".to_owned(), serde_json::json!([""]));

    let err = scan_with_config(config).expect_err("invalid excludes must fail");
    assert_invalid_excludes(err, "excludes[0]");
}

#[test]
fn tree_sitter_scan_rejects_non_string_excludes_entry() {
    let mut config = valid_config();
    config.insert("excludes".to_owned(), serde_json::json!(["valid/**", 42]));

    let err = scan_with_config(config).expect_err("invalid excludes must fail");
    assert_invalid_excludes(err, "excludes[1]");
}

#[test]
fn absolute_excludes_path_is_config_error() {
    let mut config = valid_config();
    config.insert("excludes".to_owned(), serde_json::json!(["/tmp/**"]));

    let err = scan_with_config(config).expect_err("absolute excludes path must fail");
    assert_invalid_excludes(err, "excludes[0]");
}

#[test]
fn parent_dir_in_excludes_path_is_config_error() {
    let mut config = valid_config();
    config.insert("excludes".to_owned(), serde_json::json!(["../outside/**"]));

    let err = scan_with_config(config).expect_err("parent-dir excludes path must fail");
    assert_invalid_excludes(err, "excludes[0]");
}

#[test]
fn tree_sitter_scan_excludes_repo_relative_compose_files_from_counts_and_facts() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let repo_root = tempdir.path();
    fs::create_dir_all(repo_root.join("design-system")).expect("create registry dir");
    fs::create_dir_all(repo_root.join("app/src/main/kotlin")).expect("create source dir");
    fs::write(
        repo_root.join("design-system/registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton"}]}"#,
    )
    .expect("write registry");
    fs::write(
        repo_root.join("app/src/main/kotlin/Included.kt"),
        "@Composable\nfun IncludedScreen() {\n    PrimaryButton(onClick = {})\n}\n",
    )
    .expect("write included source");
    fs::write(
        repo_root.join("app/src/main/kotlin/Excluded.kt"),
        "@Composable\nfun ExcludedScreen() {\n    PrimaryButton(onClick = {})\n}\n",
    )
    .expect("write excluded source");

    let mut config = serde_json::Map::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );
    config.insert(
        "excludes".to_owned(),
        serde_json::json!(["app/src/main/kotlin/Excluded.kt"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: repo_root.display().to_string(),
        snapshot_id: "snap-compose-excludes".to_owned(),
        config,
    };

    let facts = ComposeLanguage::new()
        .scan(&request)
        .expect("scan with excludes should succeed");

    assert_eq!(facts.metrics.files_scanned, 1);
    assert_eq!(facts.local_components.len(), 1);
    assert_eq!(facts.local_components[0].symbol, "IncludedScreen");
    assert_eq!(facts.usage_sites.len(), 1);
    assert!(
        facts
            .usage_sites
            .iter()
            .all(|site| site.location.file == "app/src/main/kotlin/Included.kt")
    );
}

#[test]
fn tree_sitter_scan_excludes_compose_files_matching_glob_patterns() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let repo_root = tempdir.path();
    fs::create_dir_all(repo_root.join("design-system")).expect("create registry dir");
    fs::create_dir_all(repo_root.join("app/src/main/kotlin")).expect("create source dir");
    fs::write(
        repo_root.join("design-system/registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton"}]}"#,
    )
    .expect("write registry");
    fs::write(
        repo_root.join("app/src/main/kotlin/Included.kt"),
        "@Composable\nfun IncludedScreen() {\n    PrimaryButton(onClick = {})\n}\n",
    )
    .expect("write included source");
    fs::write(
        repo_root.join("app/src/main/kotlin/Excluded.preview.kt"),
        "@Composable\nfun ExcludedPreview() {\n    PrimaryButton(onClick = {})\n}\n",
    )
    .expect("write excluded source");

    let mut config = serde_json::Map::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );
    config.insert(
        "excludes".to_owned(),
        serde_json::json!(["**/*.preview.kt"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: repo_root.display().to_string(),
        snapshot_id: "snap-compose-excludes-glob".to_owned(),
        config,
    };

    let facts = ComposeLanguage::new()
        .scan(&request)
        .expect("scan with glob excludes should succeed");

    assert_eq!(facts.metrics.files_scanned, 1);
    assert_eq!(facts.local_components.len(), 1);
    assert_eq!(facts.local_components[0].symbol, "IncludedScreen");
    assert_eq!(facts.usage_sites.len(), 1);
    assert!(
        facts
            .usage_sites
            .iter()
            .all(|site| site.location.file == "app/src/main/kotlin/Included.kt")
    );
}

fn scan_with_config(
    config: serde_json::Map<String, serde_json::Value>,
) -> Result<ScanFacts, ComposeScanError> {
    let fixture_root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-config".to_owned(),
        config,
    };
    ComposeLanguage::new().scan(&request)
}

fn assert_config_error(err: ComposeScanError) {
    match err {
        ComposeScanError::InvalidConfig(message) => {
            assert!(
                message.contains("roots")
                    || message.contains("design_system_registry")
                    || message.contains("registry"),
                "expected config validation message, got: {message}"
            );
        }
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}

fn assert_invalid_excludes(err: ComposeScanError, expected: &str) {
    match err {
        ComposeScanError::InvalidConfig(message) => {
            assert!(
                message.contains(expected),
                "expected validation message to contain {expected:?}, got: {message}"
            );
        }
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}

fn valid_config() -> serde_json::Map<String, serde_json::Value> {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );
    config
}
