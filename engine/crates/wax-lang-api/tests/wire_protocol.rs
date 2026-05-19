use serde_json::{Map, Value, json};
use std::str::FromStr;
use time::macros::datetime;
use wax_contract::{
    CountSummary, DesignSystemComponent, Diagnostic, LanguageId, LanguageMetadata, LocalComponent,
    MatchStatus, Metrics, ScanFacts, ScanStatus, SourceLocation, UsageSite,
};
use wax_lang_api::{
    ScanRequest, ScanRequestType, WireErrorCode, WireScanRequest, WireScanResponse,
};

#[test]
fn wire_protocol_request_fixture_roundtrips_required_fields() {
    let fixture = json!({
        "type": "scan",
        "api_version": 1,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "snapshot_id": "snap-123",
        "config": {
            "design_system_registry": "./registry/components.json",
            "strict": true
        }
    });

    let request: WireScanRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn wire_protocol_scan_request_and_wire_request_stay_in_sync() {
    let in_process = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: 1,
        language_id: LanguageId::from_str("compose").unwrap(),
        repo_root: "/repo/root".to_owned(),
        snapshot_id: "snap-123".to_owned(),
        config: Map::from_iter([(
            String::from("design_system_registry"),
            Value::from("./registry/components.json"),
        )]),
    };

    let scan_request_json = serde_json::to_value(&in_process).unwrap();
    let reparsed_wire: WireScanRequest = serde_json::from_value(scan_request_json).unwrap();
    let wire_json = serde_json::to_value(&reparsed_wire).unwrap();
    let reparsed_wire: WireScanRequest = serde_json::from_value(wire_json).unwrap();
    let in_process_back = ScanRequest::from(reparsed_wire);

    assert_eq!(in_process, in_process_back);
}

#[test]
fn wire_protocol_success_fixture_requires_scan_facts_type_tag() {
    let response = json!({
        "type": "scan_facts",
        "api_version": 1,
        "language_id": "compose",
        "facts": sample_scan_facts(),
    });

    let parsed: WireScanResponse = serde_json::from_value(response).unwrap();

    match parsed {
        WireScanResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "compose");
            assert_eq!(facts.snapshot_id, "snap-123");
        }
        _ => panic!("expected scan_facts response"),
    }
}

#[test]
fn wire_protocol_success_fixture_rejects_invalid_scan_facts() {
    let mut facts = sample_scan_facts();
    facts.counts.usage_site_count = 2;

    let response = json!({
        "type": "scan_facts",
        "api_version": 1,
        "language_id": "compose",
        "facts": facts,
    });

    assert!(serde_json::from_value::<WireScanResponse>(response).is_err());
}

#[test]
fn wire_protocol_error_fixture_deserializes_registry_not_found() {
    let response = json!({
        "type": "error",
        "api_version": 1,
        "language_id": "compose",
        "code": "registry_not_found",
        "message": "registry missing",
        "diagnostics": []
    });

    let parsed: WireScanResponse = serde_json::from_value(response).unwrap();

    match parsed {
        WireScanResponse::Error {
            api_version,
            language_id,
            code,
            message,
            diagnostics,
        } => {
            assert_eq!(api_version, 1);
            assert_eq!(language_id.as_str(), "compose");
            assert_eq!(code, WireErrorCode::RegistryNotFound);
            assert_eq!(message, "registry missing");
            assert!(diagnostics.is_empty());
        }
        _ => panic!("expected error response"),
    }
}

#[test]
fn wire_protocol_untagged_or_malformed_response_fails() {
    let untagged = json!({
        "facts": sample_scan_facts()
    });

    let old_success_shape = json!({
        "type": "scan_facts",
        "scan_facts": sample_scan_facts()
    });

    let malformed = json!({
        "type": "unknown",
        "message": "bad"
    });

    assert!(serde_json::from_value::<WireScanResponse>(untagged).is_err());
    assert!(serde_json::from_value::<WireScanResponse>(old_success_shape).is_err());
    assert!(serde_json::from_value::<WireScanResponse>(malformed).is_err());
}

fn sample_scan_facts() -> ScanFacts {
    ScanFacts {
        schema_version: 1,
        language: LanguageMetadata {
            id: LanguageId::from_str("compose").unwrap(),
            version: "1.0.0".to_owned(),
            ecosystem: "android".to_owned(),
            parser_name: "tree-sitter".to_owned(),
            parser_version: "0.22.0".to_owned(),
        },
        snapshot_id: "snap-123".to_owned(),
        scanned_at: datetime!(2026-05-16 12:00:00 UTC),
        status: ScanStatus::Complete,
        design_system_components: vec![DesignSystemComponent {
            id: "button".to_owned(),
            symbol: "Button".to_owned(),
            registry_symbol: "ds.Button".to_owned(),
        }],
        local_components: vec![LocalComponent {
            id: "local-card".to_owned(),
            symbol: "Card".to_owned(),
            location: SourceLocation {
                file: "src/Card.kt".to_owned(),
                line: 10,
                column: Some(5),
            },
        }],
        usage_sites: vec![UsageSite {
            id: "site-1".to_owned(),
            location: SourceLocation {
                file: "src/Screen.kt".to_owned(),
                line: 21,
                column: Some(3),
            },
            symbol: "Button".to_owned(),
            match_status: MatchStatus::Resolved,
            registry_symbol: Some("ds.Button".to_owned()),
        }],
        diagnostics: vec![Diagnostic {
            severity: wax_contract::DiagnosticSeverity::Warning,
            code: "partial_parse".to_owned(),
            message: "skipped generated file".to_owned(),
            location: None,
        }],
        metrics: Metrics {
            adoption_coverage_ratio: Some(1.0),
            parse_extract_ms: 12,
            files_scanned: 3,
        },
        counts: CountSummary {
            design_system_component_count: 1,
            local_component_count: 1,
            usage_site_count: 1,
            resolved_count: 1,
            candidate_count: 0,
        },
    }
}
