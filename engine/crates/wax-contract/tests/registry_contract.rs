use std::collections::BTreeSet;

use wax_contract::{
    RegistryDetailResponse, RegistryDocument, RegistryEntryKind, RegistryError,
    RegistryExtractionStatus, RegistryLookupError, RegistryLookupErrorCode,
    RegistryParameterTypeKind, RegistrySearchResponse, RegistrySnapshotRef, TokenCategory,
    registry_from_json, registry_to_json,
};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../fixtures/registry-search/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path}: {err}"))
}

fn registry_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/registry.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn search_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/registry-search.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn detail_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/registry-detail.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn lookup_error_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/registry-lookup-error.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn assert_schema_valid(validator: &jsonschema::Validator, value: &serde_json::Value) {
    let errors = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "expected schema acceptance, got: {errors:?}\nvalue={value}"
    );
}

fn assert_schema_invalid(validator: &jsonschema::Validator, value: &serde_json::Value) {
    assert!(
        validator.iter_errors(value).next().is_some(),
        "expected schema rejection\nvalue={value}"
    );
}

#[test]
fn registry_details_roundtrip_uses_details_field() {
    let input = serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "button",
            "symbol": "Button",
            "details": {
                "status": "complete",
                "documentation": "Primary action"
            }
        }]
    });

    let document = registry_from_json(&input.to_string()).expect("details should deserialize");
    assert_eq!(
        document.components[0]
            .details
            .as_ref()
            .and_then(|details| details.documentation.as_deref()),
        Some("Primary action")
    );

    let encoded = serde_json::to_value(&document).expect("details should serialize");
    assert_eq!(
        encoded["components"][0]["details"],
        input["components"][0]["details"]
    );
    assert!(encoded["components"][0].get("extracted").is_none());
    assert_schema_valid(&registry_schema(), &encoded);
}

#[test]
fn legacy_and_enriched_registry_roundtrip() {
    let component_only = registry_from_json(&fixture("legacy-component-only.json"))
        .expect("component-only registry should load");
    assert_eq!(component_only.schema_version, 1);
    assert_eq!(component_only.components.len(), 2);
    assert!(component_only.tokens.is_empty());
    assert!(component_only.components[0].metadata.is_none());
    assert!(component_only.components[0].details.is_none());
    assert_eq!(
        component_only.components[1].package.as_deref(),
        Some("com.acme.design")
    );

    let with_tokens = registry_from_json(&fixture("legacy-with-tokens.json"))
        .expect("token-bearing registry should load");
    let categories = with_tokens
        .tokens
        .iter()
        .map(|token| token.category)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        categories,
        BTreeSet::from([
            TokenCategory::Color,
            TokenCategory::Spacing,
            TokenCategory::Typography,
            TokenCategory::Radius,
            TokenCategory::Elevation,
            TokenCategory::Unknown,
        ])
    );
    assert_eq!(with_tokens.tokens[1].value.as_deref(), Some("16"));

    let enriched = registry_from_json(&fixture("enriched-registry.json"))
        .expect("enriched registry should load");
    assert_eq!(
        enriched
            .extra
            .get("publisher_note")
            .and_then(|v| v.as_str()),
        Some("root extension")
    );
    assert_eq!(
        enriched.extra.get("system_id").and_then(|v| v.as_str()),
        Some("acme-ds")
    );
    let button = &enriched.components[0];
    assert_eq!(
        button.extra.get("legacy_flag"),
        Some(&serde_json::json!(true))
    );
    let metadata = button.metadata.as_ref().expect("metadata");
    assert_eq!(metadata.description, None);
    assert_eq!(metadata.tags, vec!["action", "form"]);
    assert_eq!(
        metadata.extra.get("author").and_then(|v| v.as_str()),
        Some("design-system")
    );
    assert_eq!(metadata.sources.len(), 3);
    let details = button.details.as_ref().expect("details");
    assert_eq!(details.status, RegistryExtractionStatus::Partial);
    let parameters = &details.signature.as_ref().expect("signature").parameters;
    assert_eq!(
        parameters[0].type_kind,
        Some(RegistryParameterTypeKind::Enum)
    );
    assert_eq!(
        parameters[0].description.as_deref(),
        Some("Visual density.")
    );
    assert_eq!(parameters[0].allowed_values, ["Compact", "Comfortable"]);
    assert_eq!(
        parameters[3].extra.get("type"),
        Some(&serde_json::json!("legacy-string"))
    );
    assert_eq!(details.best_practices.len(), 2);
    assert_eq!(
        details.best_practices[0].extra.get("source"),
        Some(&serde_json::json!("design-guidelines"))
    );
    assert_eq!(details.field_gaps.len(), 1);
    assert_eq!(
        details
            .extra
            .get("extractor_version")
            .and_then(|v| v.as_str()),
        Some("1.2.3")
    );
    assert_eq!(
        enriched.tokens[0].extra.get("token_ext"),
        Some(&serde_json::json!(1))
    );

    for name in [
        "legacy-component-only.json",
        "legacy-with-tokens.json",
        "enriched-registry.json",
    ] {
        let document = registry_from_json(&fixture(name)).unwrap();
        let encoded = registry_to_json(&document).expect("encode");
        let round_tripped = registry_from_json(&encoded).expect("re-parse");
        assert_eq!(
            round_tripped, document,
            "{name} must survive a lossless typed round trip"
        );
        let encoded_value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_schema_valid(&registry_schema(), &encoded_value);
    }

    let enriched_json: serde_json::Value =
        serde_json::from_str(&registry_to_json(&enriched).unwrap()).unwrap();
    assert_eq!(enriched_json["publisher_note"], "root extension");
    assert_eq!(enriched_json["components"][0]["legacy_flag"], true);
    assert_eq!(
        enriched_json["components"][0]["metadata"]["author"],
        "design-system"
    );
    assert_eq!(
        enriched_json["components"][0]["details"]["extractor_version"],
        "1.2.3"
    );
    assert_eq!(enriched_json["tokens"][0]["token_ext"], 1);
}

#[test]
fn structured_parameter_and_guidance_validate() {
    let valid = serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "button",
            "symbol": "Button",
            "details": {
                "status": "complete",
                "signature": {
                    "parameters": [{
                        "name": "size",
                        "type_kind": "enum",
                        "allowed_values": ["small", "large"],
                        "description": "Visual size"
                    }]
                },
                "best_practices": [{"description": "Use one primary action"}]
            }
        }]
    });
    registry_from_json(&valid.to_string()).expect("structured facts should validate");
    assert_schema_valid(&registry_schema(), &valid);

    let invalid_cases = [
        (
            "empty parameter description",
            serde_json::json!({"description": ""}),
            "components[0].details.signature.parameters[0].description",
        ),
        (
            "empty allowed value",
            serde_json::json!({"type_kind": "enum", "allowed_values": [""]}),
            "components[0].details.signature.parameters[0].allowed_values[0]",
        ),
        (
            "duplicate allowed value",
            serde_json::json!({"type_kind": "enum", "allowed_values": ["small", "small"]}),
            "components[0].details.signature.parameters[0].allowed_values[1]",
        ),
        (
            "allowed values on non-enum",
            serde_json::json!({"type_kind": "string", "allowed_values": ["small"]}),
            "components[0].details.signature.parameters[0].allowed_values",
        ),
        (
            "enum without allowed values",
            serde_json::json!({"type_kind": "enum"}),
            "components[0].details.signature.parameters[0].allowed_values",
        ),
    ];

    for (case, mut parameter, expected_field) in invalid_cases {
        parameter["name"] = serde_json::json!("value");
        let input = serde_json::json!({
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "details": {
                    "status": "complete",
                    "signature": {"parameters": [parameter]}
                }
            }]
        });
        let error = registry_from_json(&input.to_string()).expect_err(case);
        assert!(
            matches!(error, RegistryError::ContractViolation { ref field, .. } if field == expected_field),
            "case: {case}; error: {error}"
        );
        assert_schema_invalid(&registry_schema(), &input);
    }

    let empty_guidance = serde_json::json!({
        "schema_version": 1,
        "components": [{
            "id": "button",
            "symbol": "Button",
            "details": {
                "status": "complete",
                "best_practices": [{"description": ""}]
            }
        }]
    });
    let error = registry_from_json(&empty_guidance.to_string())
        .expect_err("empty best-practice description must fail");
    assert!(matches!(
        error,
        RegistryError::ContractViolation { field, .. }
            if field == "components[0].details.best_practices[0].description"
    ));
    assert_schema_invalid(&registry_schema(), &empty_guidance);
}

#[test]
fn malformed_known_fields_and_duplicate_ids_fail() {
    let err = registry_from_json(r#"{"schema_version":1,"components":[{"id":"a","symbol":1}]}"#)
        .expect_err("symbol must be a string");
    assert!(matches!(err, RegistryError::InvalidJson(_)));

    for malformed_parameter in [
        serde_json::json!({"name": "size", "type_kind": "choice"}),
        serde_json::json!({"name": "size", "allowed_values": "small"}),
        serde_json::json!({"name": "size", "description": false}),
    ] {
        let input = serde_json::json!({
            "schema_version": 1,
            "components": [{
                "id": "button",
                "symbol": "Button",
                "details": {
                    "status": "complete",
                    "signature": {"parameters": [malformed_parameter]}
                }
            }]
        });
        let err = registry_from_json(&input.to_string())
            .expect_err("malformed structured parameter field must fail");
        assert!(matches!(err, RegistryError::InvalidJson(_)));
    }

    let err = registry_from_json(r#"{"schema_version":2,"components":[{"id":"a","symbol":"A"}]}"#)
        .expect_err("schema_version must remain 1");
    assert!(matches!(
        err,
        RegistryError::UnsupportedSchemaVersion {
            found: 2,
            supported: 1
        }
    ));

    let err = registry_from_json(
        r#"{
            "schema_version": 1,
            "components": [
                {"id":"dup","symbol":"A"},
                {"id":"dup","symbol":"B"}
            ]
        }"#,
    )
    .expect_err("duplicate component ids must fail");
    assert!(matches!(
        err,
        RegistryError::ContractViolation { field, .. } if field.contains("components")
    ));

    let err = registry_from_json(
        r#"{
            "schema_version": 1,
            "tokens": [
                {"id":"dup","key":"a","category":"color"},
                {"id":"dup","key":"b","category":"spacing"}
            ]
        }"#,
    )
    .expect_err("duplicate token ids must fail");
    assert!(matches!(
        err,
        RegistryError::ContractViolation { field, .. } if field.contains("tokens")
    ));
}

#[test]
fn token_only_lookup_document_is_valid() {
    let document =
        registry_from_json(&fixture("token-only.json")).expect("token-only registry should load");
    assert!(document.components.is_empty());
    assert_eq!(document.tokens.len(), 1);
    let encoded = registry_to_json(&document).unwrap();
    let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_schema_valid(&registry_schema(), &value);
}

#[test]
fn search_detail_and_error_json_match_schemas_and_preserve_extensions() {
    let search = RegistrySearchResponse {
        schema_version: 1,
        query: "buton".to_owned(),
        filters: wax_contract::RegistrySearchFilters {
            kind: Some(RegistryEntryKind::Component),
            language: Some(wax_contract::LanguageId::try_from("compose").unwrap()),
            limit: 20,
            extra: [("client".to_owned(), serde_json::json!("agent"))]
                .into_iter()
                .collect(),
        },
        snapshots: vec![RegistrySnapshotRef {
            source: "file:./.wax/compose.registry.json".to_owned(),
            language: wax_contract::LanguageId::try_from("compose").unwrap(),
            digest: "a".repeat(64),
            system_id: Some("acme-ds".to_owned()),
            git_commit: None,
            extra: Default::default(),
        }],
        results: vec![wax_contract::RegistrySearchResult {
            snapshot: RegistrySnapshotRef {
                source: "file:./.wax/compose.registry.json".to_owned(),
                language: wax_contract::LanguageId::try_from("compose").unwrap(),
                digest: "a".repeat(64),
                system_id: None,
                git_commit: Some("abc123".to_owned()),
                extra: Default::default(),
            },
            kind: RegistryEntryKind::Component,
            id: "ds.button".to_owned(),
            label: "Button".to_owned(),
            summary: "Primary action control".to_owned(),
            extraction_status: Some(RegistryExtractionStatus::Partial),
            extra: [("score_hint".to_owned(), serde_json::json!(0.91))]
                .into_iter()
                .collect(),
        }],
        diagnostics: vec![],
        extra: [("trace_id".to_owned(), serde_json::json!("t-1"))]
            .into_iter()
            .collect(),
    };
    let search_value = serde_json::to_value(&search).unwrap();
    assert_eq!(search_value["filters"]["client"], "agent");
    assert_eq!(search_value["results"][0]["score_hint"], 0.91);
    assert_eq!(search_value["trace_id"], "t-1");
    assert_schema_valid(&search_schema(), &search_value);

    let document = registry_from_json(&fixture("enriched-registry.json")).unwrap();
    let detail = RegistryDetailResponse {
        schema_version: 1,
        snapshot: RegistrySnapshotRef {
            source: "git:https://example.com/ds.git".to_owned(),
            language: wax_contract::LanguageId::try_from("compose").unwrap(),
            digest: "b".repeat(64),
            system_id: Some("acme-ds".to_owned()),
            git_commit: Some("deadbeef".to_owned()),
            extra: Default::default(),
        },
        kind: RegistryEntryKind::Component,
        entry: serde_json::to_value(&document.components[0]).unwrap(),
        extra: [("request_id".to_owned(), serde_json::json!("r-1"))]
            .into_iter()
            .collect(),
    };
    let detail_value = serde_json::to_value(&detail).unwrap();
    assert_eq!(detail_value["request_id"], "r-1");
    assert_eq!(detail_value["entry"]["legacy_flag"], true);
    assert_schema_valid(&detail_schema(), &detail_value);

    let error = RegistryLookupError {
        code: RegistryLookupErrorCode::AmbiguousEntry,
        message: "multiple entries matched ds.button".to_owned(),
        diagnostics: vec![],
        extra: [("candidates".to_owned(), serde_json::json!(2))]
            .into_iter()
            .collect(),
    };
    let error_value = serde_json::to_value(&error).unwrap();
    assert_eq!(error_value["code"], "ambiguous_entry");
    assert_eq!(error_value["candidates"], 2);
    assert_schema_valid(&lookup_error_schema(), &error_value);

    for code in [
        RegistryLookupErrorCode::InvalidArgument,
        RegistryLookupErrorCode::SourceNotFound,
        RegistryLookupErrorCode::InvalidRegistry,
        RegistryLookupErrorCode::OfflineCacheMiss,
        RegistryLookupErrorCode::LockedSnapshotUnavailable,
        RegistryLookupErrorCode::AmbiguousEntry,
        RegistryLookupErrorCode::EntryNotFound,
        RegistryLookupErrorCode::SnapshotMismatch,
    ] {
        let value = serde_json::to_value(RegistryLookupError {
            code,
            message: "x".to_owned(),
            diagnostics: vec![],
            extra: Default::default(),
        })
        .unwrap();
        assert_schema_valid(&lookup_error_schema(), &value);
    }
}

#[test]
fn snapshot_schema_requires_language_identity() {
    let snapshot_without_language = serde_json::json!({
        "source": "file:./.wax/compose.registry.json",
        "digest": "a".repeat(64)
    });
    assert!(
        search_schema()
            .iter_errors(&serde_json::json!({
                "schema_version": 1,
                "query": "button",
                "filters": {"limit": 10},
                "snapshots": [snapshot_without_language],
                "results": []
            }))
            .next()
            .is_some()
    );
}

#[test]
fn registry_document_type_is_usable_for_later_phases() {
    let _: fn(&str) -> Result<RegistryDocument, RegistryError> = registry_from_json;
    let _: fn(&RegistryDocument) -> Result<String, RegistryError> = registry_to_json;
}

#[test]
fn existing_language_pack_registry_fixture_still_loads() {
    let path = format!(
        "{}/../../crates/wax-lang-basic/tests/fixtures/small/design-system/registry.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path}: {err}"));
    let document = registry_from_json(&raw).expect("legacy pack fixture should load");
    assert!(!document.components.is_empty());
    assert!(!document.tokens.is_empty());
}
