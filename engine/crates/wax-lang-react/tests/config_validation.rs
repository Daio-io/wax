use std::collections::BTreeMap;
use std::path::PathBuf;

use wax_contract::{LanguageId, ScanStatus};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_react::{ReactConfigMode, ReactLanguage, ReactScanError, parse_react_scan_config};

#[test]
fn empty_config_uses_scaffold_mode() {
    let mode = parse_react_scan_config(&serde_json::Map::new()).unwrap();

    assert_eq!(mode, ReactConfigMode::Scaffold);
}

#[test]
fn configured_react_scan_config_parses_all_fields() {
    let mode = parse_react_scan_config(&valid_config()).unwrap();

    let ReactConfigMode::Configured(config) = mode else {
        panic!("expected configured mode");
    };
    assert_eq!(
        config.design_system_registry,
        PathBuf::from("design-system/registry.json")
    );
    assert_eq!(config.roots, vec![PathBuf::from("src")]);
    assert_eq!(config.ignore, vec!["src/generated/**"]);
    assert_eq!(config.tsconfig, Some(PathBuf::from("tsconfig.json")));
    assert_eq!(
        config.aliases,
        BTreeMap::from([("@/*".to_owned(), vec!["src/*".to_owned()])])
    );
    assert_eq!(
        config
            .packages
            .get("@acme/design-system")
            .expect("package must parse")
            .exports,
        BTreeMap::from([("Button".to_owned(), "src/ds/Button.tsx".to_owned())])
    );
}

#[test]
fn configured_scan_reports_parse_failed_for_invalid_source() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let registry_dir = temp.path().join("design-system");
    std::fs::create_dir_all(&registry_dir).expect("registry dir should be created");
    std::fs::write(
        registry_dir.join("registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
    )
    .expect("registry fixture should be written");
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir should be created");
    std::fs::write(src_dir.join("App.tsx"), "export {}").expect("valid source fixture");
    std::fs::write(
        src_dir.join("Broken.tsx"),
        "export function Broken() { return <button><span></button>; }",
    )
    .expect("invalid source fixture");

    let facts = scan_with_repo_root(
        temp.path().to_string_lossy().as_ref(),
        serde_json::Value::Object(valid_config()),
    )
    .expect("configured scan should return partial facts");

    assert_eq!(facts.status, ScanStatus::Partial);
    assert_eq!(facts.metrics.files_scanned, 2);
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "parse_failed"),
        "expected parse_failed diagnostic, got: {:?}",
        facts.diagnostics
    );
}

#[test]
fn valid_configured_config_loads_registry_symbols() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let registry_dir = temp.path().join("design-system");
    std::fs::create_dir_all(&registry_dir).expect("registry dir should be created");
    std::fs::write(
        registry_dir.join("registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
    )
    .expect("registry fixture should be written");
    let src_dir = temp.path().join("src");
    let ds_dir = src_dir.join("ds");
    std::fs::create_dir_all(&ds_dir).expect("ds dir should be created");
    std::fs::create_dir_all(&src_dir).expect("src dir should be created");
    std::fs::write(src_dir.join("App.tsx"), "export {}").expect("source fixture should be written");
    std::fs::write(ds_dir.join("Button.tsx"), "export {}")
        .expect("package export fixture should be written");

    let facts = scan_with_repo_root(
        temp.path().to_string_lossy().as_ref(),
        serde_json::Value::Object(valid_config()),
    )
    .expect("valid configured config should load registry symbols");

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.design_system_components.len(), 1);
    assert_eq!(facts.counts.registry.component_count, 1);
    assert_eq!(facts.design_system_components[0].symbol, "Button");
    assert!(facts.local_components.is_empty());
    assert!(facts.usage_sites.is_empty());
    assert_eq!(facts.metrics.files_scanned, 2);
    assert!(facts.diagnostics.is_empty());
    assert_eq!(facts.language.parser_name, "swc");
}

#[test]
fn roots_without_registry_is_config_error() {
    let err = scan_with_config(serde_json::json!({
        "roots": ["src"]
    }))
    .expect_err("roots without registry must fail");

    assert_config_error(err, "registry");
}

#[test]
fn empty_roots_array_is_config_error() {
    let err = scan_with_config(serde_json::json!({
        "design_system_registry": "design-system/registry.json",
        "roots": []
    }))
    .expect_err("empty roots must fail");

    assert_config_error(err, "roots");
}

#[test]
fn absolute_paths_are_config_errors() {
    let mut config = valid_config();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("/tmp/registry.json".to_owned()),
    );

    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("absolute registry path must fail");

    assert_config_error(err, "registry");
}

#[test]
fn windows_drive_absolute_paths_are_config_errors_on_this_host() {
    let mut config = valid_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String(r"C:\tmp\registry.json".to_owned()),
    );

    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("Windows drive absolute registry path must fail");

    assert_config_error(err, "registry");
}

#[test]
fn unc_absolute_paths_are_config_errors_on_this_host() {
    let mut config = valid_config();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String(r"\\server\share\registry.json".to_owned()),
    );

    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("UNC registry path must fail");

    assert_config_error(err, "registry");
}

#[test]
fn parent_directory_segments_are_config_errors() {
    let mut config = valid_config();
    config.insert("roots".to_owned(), serde_json::json!(["../outside/src"]));

    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("parent-dir root path must fail");

    assert_config_error(err, "roots[0]");
}

#[test]
fn backslash_parent_directory_segments_are_config_errors_on_this_host() {
    let mut config = valid_config();
    config.insert("roots".to_owned(), serde_json::json!([r"src\..\outside"]));

    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("backslash parent-dir root path must fail");

    assert_config_error(err, "roots[0]");
}

#[test]
fn tsconfig_aliases_and_package_exports_reject_path_escapes() {
    let mut config = valid_config();
    config.insert(
        "tsconfig".to_owned(),
        serde_json::Value::String("../tsconfig.json".to_owned()),
    );
    let err = scan_with_config(serde_json::Value::Object(config.clone()))
        .expect_err("parent-dir tsconfig path must fail");
    assert_config_error(err, "tsconfig");

    config.insert(
        "tsconfig".to_owned(),
        serde_json::Value::String("tsconfig.json".to_owned()),
    );
    config.insert(
        "aliases".to_owned(),
        serde_json::json!({
            "@/*": ["/tmp/src/*"]
        }),
    );
    let err = scan_with_config(serde_json::Value::Object(config.clone()))
        .expect_err("absolute alias target must fail");
    assert_config_error(err, "aliases.@/*[0]");

    config.insert(
        "aliases".to_owned(),
        serde_json::json!({
            "@/*": ["src/*"]
        }),
    );
    config.insert(
        "packages".to_owned(),
        serde_json::json!({
            "@acme/design-system": {
                "exports": {
                    ".": "../outside/index.ts"
                }
            }
        }),
    );
    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("parent-dir package export target must fail");
    assert_config_error(err, "packages.@acme/design-system.exports.");
}

#[test]
fn path_like_ignore_escapes_are_config_errors() {
    let mut config = valid_config();
    config.insert("ignore".to_owned(), serde_json::json!(["../outside/**"]));

    let err = scan_with_config(serde_json::Value::Object(config))
        .expect_err("path-like ignore escape must fail");

    assert_config_error(err, "ignore[0]");
}

fn scan_with_config(config: serde_json::Value) -> Result<wax_contract::ScanFacts, ReactScanError> {
    scan_with_repo_root("/tmp/repo", config)
}

fn scan_with_repo_root(
    repo_root: &str,
    config: serde_json::Value,
) -> Result<wax_contract::ScanFacts, ReactScanError> {
    let serde_json::Value::Object(config) = config else {
        panic!("test config must be a JSON object");
    };
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("react").expect("react id must be valid"),
        repo_root: repo_root.to_owned(),
        snapshot_id: "snap-config".to_owned(),
        config,
    };
    ReactLanguage::new().scan(&request)
}

fn assert_config_error(err: ReactScanError, expected_field: &str) {
    match err {
        ReactScanError::InvalidConfig(message) => assert!(
            message.contains(expected_field),
            "expected {expected_field} in config error message, got: {message}"
        ),
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}

fn valid_config() -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({
        "design_system_registry": "design-system/registry.json",
        "roots": ["src"],
        "ignore": ["src/generated/**"],
        "tsconfig": "tsconfig.json",
        "aliases": {
            "@/*": ["src/*"]
        },
        "packages": {
            "@acme/design-system": {
                "exports": {
                    "Button": "src/ds/Button.tsx"
                }
            }
        }
    })
    .as_object()
    .expect("valid config is an object")
    .clone()
}
