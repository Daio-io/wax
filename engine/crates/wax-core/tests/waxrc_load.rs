use wax_core::config::waxrc::{LanguageRegistrySource, WaxRcError, load_waxrc};

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/config")
        .join(name)
}

#[test]
fn loads_minimal_waxrc() {
    let rc = load_waxrc(fixture_path("minimal.waxrc")).unwrap();

    assert_eq!(rc.schema_version, 2);
    assert_eq!(rc.engine.scan_concurrency, 2);
    assert_eq!(rc.languages.len(), 1);
    assert_eq!(rc.languages[0].id.as_str(), "compose");
    assert!(rc.languages[0].roots.is_empty());
    assert!(rc.languages[0].registry_source.is_none());
    assert!(rc.reporting.source_boundaries.is_empty());
    assert!(rc.design_systems.is_empty());
}

#[test]
fn waxrc_preserves_language_extra_config() {
    let rc = load_waxrc(fixture_path("with-extra.waxrc")).unwrap();

    assert_eq!(rc.engine.scan_concurrency, 4);
    assert_eq!(rc.languages[0].roots, ["app/src"]);
    assert_eq!(
        rc.languages[0]
            .registry_source
            .as_ref()
            .unwrap()
            .path_or_url_parts()
            .unwrap()
            .0,
        "design-system/registry.json"
    );
}

#[test]
fn waxrc_loads_multiple_languages() {
    let rc = load_waxrc(fixture_path("multiple-languages.waxrc")).unwrap();

    assert_eq!(rc.languages.len(), 2);
    assert_eq!(rc.languages[0].id.as_str(), "compose");
    assert_eq!(rc.languages[0].roots, ["app/src/main/kotlin"]);
    assert_eq!(rc.languages[1].id.as_str(), "react");
    assert_eq!(rc.languages[1].roots, ["apps/web/src"]);
}

#[test]
fn waxrc_parses_supported_adoption_config() {
    let rc = load_waxrc(fixture_path("with-adoption.waxrc")).unwrap();

    assert!(rc.adoption.track_local_invocations);
    assert!(rc.adoption.track_unresolved_invocations);
    assert!(rc.adoption.parent_attribution.enabled);
    assert_eq!(
        rc.adoption.parent_attribution.scope_visibility,
        ["public", "internal", "private"]
    );
    assert_eq!(rc.adoption.symbol_usage_summary.parent_scope_limit, Some(0));
}

#[test]
fn waxrc_loads_ordered_source_boundaries() {
    let path = std::env::temp_dir().join(format!(
        "waxrc-source-boundaries-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"{
          "schema_version": 2,
          "languages": {"compose": {}, "react": {}},
          "reporting": {
            "source_boundaries": [
              {
                "id": "feature/devices",
                "languages": ["compose"],
                "include": ["mobile\\**\\feature\\devices\\**\\*.kt"],
                "exclude": ["**/generated/**"]
              },
              {
                "id": "app/web",
                "include": ["web/**"]
              }
            ]
          }
        }"#,
    )
    .unwrap();

    let rc = load_waxrc(&path).unwrap();
    std::fs::remove_file(path).unwrap();

    assert_eq!(rc.reporting.source_boundaries.len(), 2);
    assert_eq!(rc.reporting.source_boundaries[0].id, "feature/devices");
    assert_eq!(
        rc.reporting.source_boundaries[0].languages,
        Some(vec!["compose".try_into().unwrap()])
    );
    assert_eq!(
        rc.reporting.source_boundaries[0].include,
        ["mobile\\**\\feature\\devices\\**\\*.kt"]
    );
    assert_eq!(
        rc.reporting.source_boundaries[0].exclude,
        ["**/generated/**"]
    );
    assert!(rc.reporting.source_boundaries[1].languages.is_none());
}

#[test]
fn waxrc_rejects_source_boundary_language_not_configured() {
    let path = std::env::temp_dir().join(format!(
        "waxrc-source-boundary-language-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"{
          "schema_version": 2,
          "languages": {"compose": {}},
          "reporting": {
            "source_boundaries": [{
              "id": "feature/devices",
              "languages": ["react"],
              "include": ["mobile/**"]
            }]
          }
        }"#,
    )
    .unwrap();

    let err = load_waxrc(&path).unwrap_err();
    std::fs::remove_file(path).unwrap();

    assert!(err.to_string().contains("not configured"));
}

#[test]
fn waxrc_rejects_unsafe_or_ambiguous_source_boundaries() {
    let cases = [
        (
            "duplicate",
            r#"[{"id":"same","include":["src/**"]},{"id":"same","include":["app/**"]}]"#,
            "must be unique",
        ),
        (
            "absolute",
            r#"[{"id":"absolute","include":["/src/**"]}]"#,
            "must be repo-relative",
        ),
        (
            "parent",
            r#"[{"id":"parent","include":["src/../app/**"]}]"#,
            "parent-directory",
        ),
        (
            "empty-include",
            r#"[{"id":"empty","include":[]}]"#,
            "at least one glob",
        ),
    ];

    for (index, (name, boundaries, expected)) in cases.into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!(
            "waxrc-source-boundary-invalid-{name}-{}-{index}.json",
            std::process::id()
        ));
        let contents = format!(
            r#"{{"schema_version":2,"languages":{{"compose":{{}}}},"reporting":{{"source_boundaries":{boundaries}}}}}"#
        );
        std::fs::write(&path, contents).unwrap();
        let err = load_waxrc(&path).unwrap_err();
        std::fs::remove_file(path).unwrap();
        assert!(err.to_string().contains(expected), "{name}: {err}");
    }
}

#[test]
fn waxrc_rejects_reserved_adoption_modes() {
    let err = load_waxrc(fixture_path("with-unsupported-adoption.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(
        err.to_string()
            .contains("adoption.track_local_invocations=false is not supported yet")
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
            supported: 2
        }
    ));
    assert!(
        err.to_string()
            .contains("unsupported wax config schema_version 999 in")
    );
    assert!(
        err.to_string()
            .contains("unsupported-schema.waxrc; this engine supports 2")
    );
}

#[test]
fn waxrc_rejects_unsupported_schema_version_before_v2_shape() {
    let err = load_waxrc(fixture_path("unsupported-schema-missing-v1-fields.waxrc")).unwrap_err();

    assert!(matches!(
        err,
        WaxRcError::UnsupportedSchemaVersion {
            path: _,
            found: 999,
            supported: 2
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
fn waxrc_rejects_legacy_language_fields() {
    let legacy_id = load_waxrc(fixture_path("missing-language-id.waxrc")).unwrap_err();
    let legacy_enabled = load_waxrc(fixture_path("missing-language-enabled.waxrc")).unwrap_err();

    assert!(matches!(legacy_id, WaxRcError::InvalidConfig { .. }));
    assert!(
        legacy_id
            .to_string()
            .contains("languages.*.id is not supported")
    );
    assert!(matches!(legacy_enabled, WaxRcError::InvalidConfig { .. }));
    assert!(
        legacy_enabled
            .to_string()
            .contains("languages.*.enabled is not supported")
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
        language.registry_source.as_ref().unwrap(),
        &LanguageRegistrySource::PathOrUrl {
            source: ".wax/compose.registry.json".to_owned(),
            upstream: None,
        }
    );
    assert!(!language.extra.contains_key("registry"));
    assert_eq!(language.roots, ["app/src/main/kotlin"]);
}

#[test]
fn parses_registry_source_object() {
    let rc = load_waxrc(fixture_path("with-registry-object.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(
        language.registry_source.as_ref().unwrap(),
        &LanguageRegistrySource::PathOrUrl {
            source: "https://example.com/acme-ds/registry/v2.4.1/compose.json".to_owned(),
            upstream: None,
        }
    );
    assert!(!language.extra.contains_key("registry"));
    assert_eq!(language.roots, ["app/src/main/kotlin"]);
}

#[test]
fn malformed_registry_is_reported_as_invalid_config() {
    let err = load_waxrc(fixture_path("with-malformed-registry-and-legacy.waxrc")).unwrap_err();

    assert!(matches!(err, WaxRcError::InvalidConfig { .. }));
    assert!(err.to_string().contains("invalid wax config"));
}
