use time::macros::datetime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, MatchStatus,
    Metrics, SCHEMA_VERSION, ScanFacts, ScanStatus, SourceLocation, UsageSite,
};

fn scan_facts_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/scan-facts.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn assert_schema_rejects(value: &serde_json::Value) {
    let validator = scan_facts_schema();
    let errors = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(
        !errors.is_empty(),
        "expected schema rejection, but value was valid"
    );
}

fn minimal_facts() -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("compose").unwrap(),
            version: "0.0.0".into(),
            ecosystem: "jetpack-compose".into(),
            parser_name: "tree-sitter-kotlin".into(),
            parser_version: "0.3.8".into(),
        },
        snapshot_id: "test-snapshot".into(),
        scanned_at: datetime!(2026-05-16 12:00 UTC),
        status: ScanStatus::Complete,
        design_system_components: vec![],
        local_components: vec![],
        usage_sites: vec![
            UsageSite {
                id: "a:1:Button:resolved".into(),
                location: SourceLocation {
                    file: "a.kt".into(),
                    line: 1,
                    column: Some(5),
                },
                symbol: "Button".into(),
                match_status: MatchStatus::Resolved,
                registry_symbol: Some("com.ds.Button".into()),
            },
            UsageSite {
                id: "a:2:Card:candidate".into(),
                location: SourceLocation {
                    file: "a.kt".into(),
                    line: 2,
                    column: None,
                },
                symbol: "Card".into(),
                match_status: MatchStatus::Candidate,
                registry_symbol: Some("com.ds.Card".into()),
            },
        ],
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "W001".into(),
            message: "example".into(),
            location: None,
        }],
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 12,
            files_scanned: 1,
        },
        counts: CountSummary {
            design_system_component_count: 0,
            local_component_count: 0,
            usage_site_count: 0,
            resolved_count: 0,
            candidate_count: 0,
            framework_shadow_count: 0,
        },
    }
}

#[test]
fn scan_facts_roundtrip() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(facts, back);
    assert_eq!(back.metrics.adoption_coverage_ratio, Some(0.5));
}

#[test]
fn serialized_scan_facts_validate_against_schema() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    let validator = scan_facts_schema();

    assert!(validator.is_valid(&value));
}

#[test]
fn schema_rejects_values_outside_integer_bounds() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut value = serde_json::to_value(&facts).unwrap();
    value["usage_sites"][0]["location"]["line"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["usage_sites"][0]["location"]["column"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["metrics"]["files_scanned"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["metrics"]["parse_extract_ms"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["counts"]["usage_site_count"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);
}

#[test]
fn rejects_invalid_language_id() {
    assert!(LanguageId::try_from("Compose").is_err());
    assert!(LanguageId::try_from("1compose").is_err());
    assert!(LanguageId::try_from("").is_err());
}

#[test]
fn rejects_invalid_language_id_from_json() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["language"]["id"] = serde_json::json!("Compose");

    assert_schema_rejects(&value);
    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_unsupported_schema_version() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["schema_version"] = serde_json::json!(999);

    let err = wax_contract::scan_facts_from_json(&value.to_string()).unwrap_err();

    assert!(matches!(
        err,
        wax_contract::ScanFactsError::UnsupportedSchemaVersion {
            found: 999,
            supported: SCHEMA_VERSION
        }
    ));
}

#[test]
fn rejects_zero_line_and_column() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut zero_line = serde_json::to_value(&facts).unwrap();
    zero_line["usage_sites"][0]["location"]["line"] = serde_json::json!(0);
    assert!(wax_contract::scan_facts_from_json(&zero_line.to_string()).is_err());

    let mut zero_column = serde_json::to_value(&facts).unwrap();
    zero_column["usage_sites"][0]["location"]["column"] = serde_json::json!(0);
    assert!(wax_contract::scan_facts_from_json(&zero_column.to_string()).is_err());
}

#[test]
fn rejects_empty_required_strings() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["language"]["parser_name"] = serde_json::json!("");

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_explicit_null_for_optional_schema_fields() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["usage_sites"][1]["location"]["column"] = serde_json::Value::Null;

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_inconsistent_counts_and_metrics() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut stale_counts = serde_json::to_value(&facts).unwrap();
    stale_counts["counts"]["resolved_count"] = serde_json::json!(0);
    assert!(wax_contract::scan_facts_from_json(&stale_counts.to_string()).is_err());

    let mut stale_ratio = serde_json::to_value(&facts).unwrap();
    stale_ratio["metrics"]["adoption_coverage_ratio"] = serde_json::json!(1.0);
    assert!(wax_contract::scan_facts_from_json(&stale_ratio.to_string()).is_err());
}

#[test]
fn rejects_parse_extract_ms_above_contract_max() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    facts.metrics.parse_extract_ms = 4_294_967_296;

    let json = serde_json::to_string(&facts).unwrap();

    assert!(wax_contract::scan_facts_from_json(&json).is_err());
}

#[test]
fn requires_registry_symbol_for_resolved_and_candidate_usage() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut resolved_missing = serde_json::to_value(&facts).unwrap();
    resolved_missing["usage_sites"][0]
        .as_object_mut()
        .unwrap()
        .remove("registry_symbol");
    assert_schema_rejects(&resolved_missing);
    assert!(wax_contract::scan_facts_from_json(&resolved_missing.to_string()).is_err());

    let mut candidate_missing = serde_json::to_value(&facts).unwrap();
    candidate_missing["usage_sites"][1]
        .as_object_mut()
        .unwrap()
        .remove("registry_symbol");
    assert_schema_rejects(&candidate_missing);
    assert!(wax_contract::scan_facts_from_json(&candidate_missing.to_string()).is_err());
}

#[test]
fn framework_shadow_usage_requires_registry_symbol() {
    let mut facts = minimal_facts();
    facts.usage_sites.push(UsageSite {
        id: "a:3:Button:framework_shadow".into(),
        location: SourceLocation {
            file: "a.kt".into(),
            line: 3,
            column: Some(1),
        },
        symbol: "Button".into(),
        match_status: MatchStatus::FrameworkShadow,
        registry_symbol: Some("Button".into()),
    });
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();

    assert_eq!(back.counts.framework_shadow_count, 1);
    assert_eq!(back.counts.usage_site_count, 3);
    assert_eq!(back.metrics.adoption_coverage_ratio, Some(1.0 / 3.0));
}

#[test]
fn rejects_registry_symbol_for_unresolved_usage() {
    let mut facts = minimal_facts();
    facts.usage_sites[0].match_status = MatchStatus::Unresolved;
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert_schema_rejects(&value);
    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn accepts_zero_usage_sites_with_null_coverage() {
    let mut facts = minimal_facts();
    facts.usage_sites.clear();
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();

    assert_eq!(back.metrics.adoption_coverage_ratio, None);
    assert_eq!(back.counts.usage_site_count, 0);
}

#[test]
fn rejects_missing_adoption_coverage_ratio() {
    let mut facts = minimal_facts();
    facts.usage_sites.clear();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["metrics"]
        .as_object_mut()
        .unwrap()
        .remove("adoption_coverage_ratio");

    assert_schema_rejects(&value);
    let err = wax_contract::scan_facts_from_json(&value.to_string()).unwrap_err();

    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "metrics.adoption_coverage_ratio"
    ));
}

#[test]
fn all_candidate_usage_has_zero_coverage() {
    let mut facts = minimal_facts();
    facts.usage_sites[0].match_status = MatchStatus::Candidate;
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();

    assert_eq!(back.metrics.adoption_coverage_ratio, Some(0.0));
    assert_eq!(back.counts.resolved_count, 0);
    assert_eq!(back.counts.candidate_count, 2);
}

#[test]
fn scanned_at_serializes_as_rfc3339() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert_eq!(value["scanned_at"], "2026-05-16T12:00:00Z");
}

#[test]
fn accepts_non_utc_rfc3339_timestamp() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["scanned_at"] = serde_json::json!("2026-05-16T14:00:00+02:00");

    let back = wax_contract::scan_facts_from_json(&value.to_string()).unwrap();

    assert_eq!(back.scanned_at, facts.scanned_at);
}
