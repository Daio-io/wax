use wax_contract::{LanguageId, ScanFacts};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_basic::{BasicLanguage, BasicScanError};

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
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));

    let err = scan_with_config(config).expect_err("roots without registry must fail");
    assert_config_error(err);
}

#[test]
fn invalid_file_extension_entry_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));
    config.insert("file_extensions".to_owned(), serde_json::json!([""]));

    let err = scan_with_config(config).expect_err("empty extension must fail");
    assert_config_error(err);
}

#[test]
fn absolute_registry_path_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("/etc/passwd".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));

    let err = scan_with_config(config).expect_err("absolute registry path must fail");
    assert_config_error(err);
}

#[test]
fn absolute_root_path_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["/tmp/outside"]));

    let err = scan_with_config(config).expect_err("absolute root path must fail");
    assert_config_error(err);
}

#[test]
fn parent_dir_in_root_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["../outside"]));

    let err = scan_with_config(config).expect_err("parent-dir root must fail");
    assert_config_error(err);
}

#[test]
fn parent_dir_in_registry_path_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("../outside/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));

    let err = scan_with_config(config).expect_err("parent-dir registry path must fail");
    assert_config_error(err);
}

#[test]
fn malformed_registry_is_config_error() {
    let fixture_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/invalid-registry");
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("basic").expect("basic id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-malformed-registry".to_owned(),
        config,
    };

    let err = BasicLanguage::new()
        .scan(&request)
        .expect_err("malformed registry must fail");
    assert_config_error(err);
}

#[test]
fn invalid_include_glob_entry_is_config_error() {
    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));
    config.insert("include_globs".to_owned(), serde_json::json!([""]));

    let err = scan_with_config(config).expect_err("empty glob must fail");
    assert_config_error(err);
}

fn scan_with_config(
    config: serde_json::Map<String, serde_json::Value>,
) -> Result<ScanFacts, BasicScanError> {
    let fixture_root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("basic").expect("basic id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-config".to_owned(),
        config,
    };
    BasicLanguage::new().scan(&request)
}

fn assert_config_error(err: BasicScanError) {
    match err {
        BasicScanError::InvalidConfig(message) => {
            assert!(
                message.contains("roots")
                    || message.contains("design_system_registry")
                    || message.contains("file_extensions")
                    || message.contains("include_globs")
                    || message.contains("registry"),
                "expected config validation message, got: {message}"
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
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));
    config.insert("file_extensions".to_owned(), serde_json::json!(["src"]));
    config
}
