use wax_contract::Catalog;

#[test]
fn two_language_bundle_catalog_roundtrips_with_schema() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/registry-catalog/two-languages.json"
    ))
    .unwrap();
    let catalog: Catalog = serde_json::from_value(value.clone()).unwrap();
    catalog.validate().unwrap();
    assert_eq!(serde_json::to_value(catalog).unwrap(), value);
    let schema =
        serde_json::from_str(include_str!("../schemas/registry-catalog.schema.json")).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
}

#[test]
fn catalog_rejects_invalid_bundle_metadata_and_ordering() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/registry-catalog/two-languages.json"
    ))
    .unwrap();
    let invalid = [
        ("revision", serde_json::json!(0)),
        ("id", serde_json::json!("Acme")),
        ("subdir", serde_json::json!("systems/../acme")),
        ("minimum_wax_version", serde_json::json!("latest")),
        ("registries", serde_json::json!({})),
        ("coverage", serde_json::json!(["x".repeat(2049)])),
        (
            "definition",
            serde_json::json!({"path":".wax/wax.config.json","sha256":"A".repeat(64)}),
        ),
    ];
    for (field, value) in invalid {
        let mut input = fixture.clone();
        input["entries"][0][field] = value;
        let catalog: Catalog = serde_json::from_value(input).unwrap();
        assert!(catalog.validate().is_err(), "accepted {field}");
    }
    let mut catalog: Catalog = serde_json::from_value(fixture).unwrap();
    catalog.entries.push(catalog.entries[0].clone());
    assert!(catalog.validate().is_err(), "duplicate revisions");
    catalog.entries[0].revision = 2;
    assert!(catalog.validate().is_err(), "unsorted revisions");
}

#[test]
fn config_schema_accepts_nested_git_and_rejects_partial_or_unsafe_bundle_fields() {
    let schema = serde_json::from_str(include_str!("../schemas/waxrc.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let valid = serde_json::json!({"schema_version":2,"languages":{"react":{"registry":{"git":"https://example.com/ds.git","tag":"v1","subdir":"systems/acme/1","system":"acme"}}}});
    assert!(validator.is_valid(&valid));
    for path in ["", "../acme", "/acme", "a//b", "a\\b", "a/-b", "."] {
        let mut invalid = valid.clone();
        invalid["languages"]["react"]["registry"]["subdir"] = path.into();
        assert!(!validator.is_valid(&invalid), "accepted {path:?}");
    }
    let mut partial = valid;
    partial["languages"]["react"]["registry"]
        .as_object_mut()
        .unwrap()
        .remove("subdir");
    assert!(!validator.is_valid(&partial));
}
