use wax_contract::LanguageId;
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_basic::{BasicLanguage, BasicScanError};

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
) -> Result<(), BasicScanError> {
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("basic").expect("basic id must be valid"),
        repo_root: "/tmp/unused".to_owned(),
        snapshot_id: "snap-config".to_owned(),
        config,
    };
    BasicLanguage::new().scan(&request).map(|_| ())
}

fn assert_config_error(err: BasicScanError) {
    match err {
        BasicScanError::InvalidConfig(message) => {
            assert!(
                message.contains("roots")
                    || message.contains("design_system_registry")
                    || message.contains("file_extensions")
                    || message.contains("include_globs"),
                "expected config validation message, got: {message}"
            );
        }
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}
