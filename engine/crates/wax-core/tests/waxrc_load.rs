use wax_core::config::waxrc::{LanguageRegistrySource, WaxRcError, load_waxrc};

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/config")
        .join(name)
}

#[test]
fn loads_minimal_waxrc() {
    let rc = load_waxrc(fixture_path("minimal.waxrc")).unwrap();

    assert_eq!(rc.schema_version, 1);
    assert_eq!(rc.engine.scan_concurrency, 2);
    assert_eq!(rc.languages.len(), 1);
    assert_eq!(rc.languages[0].id.as_str(), "compose");
    assert!(rc.languages[0].enabled);
}

#[test]
fn waxrc_preserves_language_extra_config() {
    let rc = load_waxrc(fixture_path("with-extra.waxrc")).unwrap();

    assert_eq!(rc.engine.scan_concurrency, 4);
    assert_eq!(
        rc.languages[0].extra["design_system_registry"],
        "design-system/registry.json"
    );
    assert_eq!(
        rc.languages[0].extra["roots"],
        serde_json::json!(["app/src"])
    );
}

#[test]
fn waxrc_loads_multiple_languages() {
    let rc = load_waxrc(fixture_path("multiple-languages.waxrc")).unwrap();

    assert_eq!(rc.languages.len(), 2);
    assert_eq!(rc.languages[0].id.as_str(), "compose");
    assert_eq!(
        rc.languages[0].extra["roots"],
        serde_json::json!(["app/src/main/kotlin"])
    );
    assert_eq!(rc.languages[1].id.as_str(), "react");
    assert_eq!(
        rc.languages[1].extra["roots"],
        serde_json::json!(["apps/web/src"])
    );
}

#[test]
fn waxrc_rejects_unsupported_schema_version() {
    let err = load_waxrc(fixture_path("unsupported-schema.waxrc")).unwrap_err();

    assert!(matches!(
        err,
        WaxRcError::UnsupportedSchemaVersion {
            path: _,
            found: 999,
            supported: 1
        }
    ));
    assert!(
        err.to_string()
            .contains("unsupported wax config schema_version 999 in")
    );
    assert!(
        err.to_string()
            .contains("unsupported-schema.waxrc; this engine supports 1")
    );
}

#[test]
fn waxrc_rejects_unsupported_schema_version_before_v1_shape() {
    let err = load_waxrc(fixture_path("unsupported-schema-missing-v1-fields.waxrc")).unwrap_err();

    assert!(matches!(
        err,
        WaxRcError::UnsupportedSchemaVersion {
            path: _,
            found: 999,
            supported: 1
        }
    ));
}

#[test]
fn waxrc_rejects_unknown_root_fields() {
    let err = load_waxrc(fixture_path("unknown-root-field.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("unknown field"));
    assert!(err.to_string().contains("schemaVersion"));
}

#[test]
fn waxrc_rejects_unknown_engine_fields() {
    let err = load_waxrc(fixture_path("unknown-engine-field.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("unknown field"));
    assert!(err.to_string().contains("scanConcurrency"));
}

#[test]
fn waxrc_rejects_invalid_language_id() {
    let err = load_waxrc(fixture_path("invalid-language-id.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("invalid language id"));
    assert!(err.to_string().contains("Compose"));
}

#[test]
fn waxrc_rejects_missing_required_language_fields() {
    let missing_id = load_waxrc(fixture_path("missing-language-id.waxrc")).unwrap_err();
    let missing_enabled = load_waxrc(fixture_path("missing-language-enabled.waxrc")).unwrap_err();

    assert!(matches!(missing_id, WaxRcError::InvalidConfig { .. }));
    assert!(missing_id.to_string().contains("missing field `id`"));
    assert!(matches!(missing_enabled, WaxRcError::InvalidConfig { .. }));
    assert!(
        missing_enabled
            .to_string()
            .contains("missing field `enabled`")
    );
}

#[test]
fn waxrc_rejects_non_object_root_as_invalid_config() {
    let err = load_waxrc(fixture_path("non-object-root.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("invalid wax config"));
}

#[test]
fn waxrc_distinguishes_malformed_json_from_invalid_config() {
    let malformed = load_waxrc(fixture_path("malformed.waxrc")).unwrap_err();
    let invalid_config = load_waxrc(fixture_path("missing-languages.waxrc")).unwrap_err();

    assert!(matches!(malformed, WaxRcError::MalformedJson { .. }));
    assert!(matches!(invalid_config, WaxRcError::InvalidConfig { .. }));
}

#[test]
fn waxrc_reports_missing_file_as_read_error() {
    let err = load_waxrc(fixture_path("does-not-exist.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::Read { .. }));
    assert!(err.to_string().contains("failed to read wax config"));
    assert!(err.to_string().contains("does-not-exist.waxrc"));
}

#[test]
fn parses_registry_string_without_removing_pack_config() {
    let rc = load_waxrc(fixture_path("with-registry-string.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(
        language.registry_source().unwrap(),
        LanguageRegistrySource {
            source: ".wax/compose.registry.json".to_owned(),
            field_name: "registry",
            deprecated: false,
        }
    );
    assert_eq!(
        language.extra["registry"],
        serde_json::Value::String(".wax/compose.registry.json".to_owned())
    );
    assert_eq!(
        language.extra["roots"],
        serde_json::json!(["app/src/main/kotlin"])
    );
}

#[test]
fn parses_registry_source_object() {
    let rc = load_waxrc(fixture_path("with-registry-object.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(
        language.registry_source().unwrap(),
        LanguageRegistrySource {
            source: "https://example.com/acme-ds/registry/v2.4.1/compose.json".to_owned(),
            field_name: "registry.source",
            deprecated: false,
        }
    );
    assert_eq!(
        language.extra["registry"],
        serde_json::json!({
            "source": "https://example.com/acme-ds/registry/v2.4.1/compose.json"
        })
    );
    assert_eq!(
        language.extra["roots"],
        serde_json::json!(["app/src/main/kotlin"])
    );
}

#[test]
fn parses_legacy_design_system_registry_source() {
    let rc = load_waxrc(fixture_path("with-extra.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(
        language.registry_source().unwrap(),
        LanguageRegistrySource {
            source: "design-system/registry.json".to_owned(),
            field_name: "design_system_registry",
            deprecated: true,
        }
    );
}

#[test]
fn malformed_registry_does_not_fall_back_to_legacy_alias() {
    let rc = load_waxrc(fixture_path("with-malformed-registry-and-legacy.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(language.extra["registry"], serde_json::json!({}));
    assert_eq!(
        language.extra["design_system_registry"],
        "design-system/registry.json"
    );
    assert_eq!(language.registry_source(), None);
}
