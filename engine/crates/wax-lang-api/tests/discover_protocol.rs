use serde_json::json;
use std::str::FromStr;
use wax_contract::LanguageId;
use wax_lang_api::{
    DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION, WireErrorCode, WirePackRequest,
    WirePackResponse, WireScanRequest,
};

#[test]
fn discover_wire_request_fixture_roundtrips() {
    let fixture = json!({
        "type": "discover",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "roots": ["design-system/src/main/kotlin"]
    });

    let request: WirePackRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn discover_success_response_fixture_roundtrips() {
    let response = json!({
        "type": "discover_symbols",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "symbols": ["PrimaryButton", "SecondaryButton"],
        "diagnostics": [{
            "severity": "info",
            "code": "compose_discover_skipped_private",
            "message": "skipped 1 private composable"
        }]
    });

    let parsed: WirePackResponse = serde_json::from_value(response.clone()).unwrap();
    let back = serde_json::to_value(&parsed).unwrap();

    assert_eq!(back, response);
}

#[test]
fn discover_symbols_response_rejects_unknown_fields() {
    let response = json!({
        "type": "discover_symbols",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "symbols": ["PrimaryButton"],
        "diagnostics": [],
        "extra": true
    });

    assert!(serde_json::from_value::<WirePackResponse>(response).is_err());
}

#[test]
fn pack_error_response_rejects_unknown_fields() {
    let response = json!({
        "type": "error",
        "api_version": WIRE_API_VERSION,
        "language_id": "react",
        "code": "discover_unsupported",
        "message": "react does not support registry discovery yet",
        "diagnostics": [],
        "extra": true
    });

    assert!(serde_json::from_value::<WirePackResponse>(response).is_err());
}

#[test]
fn discover_request_rejects_unknown_fields() {
    let request = json!({
        "type": "discover",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "roots": ["src"],
        "extra": true
    });

    assert!(serde_json::from_value::<WirePackRequest>(request).is_err());
}

#[test]
fn scan_request_still_deserializes_as_wire_scan_request() {
    let fixture = json!({
        "type": "scan",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "snapshot_id": "snap-123",
        "config": {}
    });

    let request: WireScanRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn wire_pack_request_scan_variant_roundtrips() {
    let fixture = json!({
        "type": "scan",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "snapshot_id": "snap-123",
        "config": {}
    });

    let request: WirePackRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn discover_unsupported_error_code_serializes() {
    let response = json!({
        "type": "error",
        "api_version": WIRE_API_VERSION,
        "language_id": "react",
        "code": "discover_unsupported",
        "message": "react does not support registry discovery yet",
        "diagnostics": []
    });

    let parsed: WirePackResponse = serde_json::from_value(response).unwrap();
    match parsed {
        WirePackResponse::Error { code, .. } => {
            assert_eq!(code, WireErrorCode::DiscoverUnsupported);
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn in_process_discover_request_converts_to_wire_request() {
    let in_process = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::from_str("compose").unwrap(),
        repo_root: "/repo/root".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
    };

    let wire = WirePackRequest::from(in_process.clone());
    let back: DiscoverRequest = wire
        .try_into()
        .expect("discover wire request converts back");

    assert_eq!(in_process, back);
}

#[test]
fn scan_wire_request_cannot_convert_to_discover_request() {
    let scan = WirePackRequest::Scan {
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::from_str("compose").unwrap(),
        repo_root: "/repo/root".to_owned(),
        snapshot_id: "snap-123".to_owned(),
        config: Default::default(),
    };

    let result = DiscoverRequest::try_from(scan);

    assert!(result.is_err());
}
