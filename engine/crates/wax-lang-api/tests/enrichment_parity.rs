//! Cross-pack enrichment parity for React, Compose, and Swift rich discovery.
//!
//! Parser packs must expose the same enrichment semantics: complete direct API
//! facts when declarations are local, and the same attachment/deprecation shape.
//! The legacy `basic` pack does not support registry discovery
//! (`WireErrorCode::DiscoverUnsupported`); engines fall back to symbol-only
//! discovery when `registry_details_v1` is false.
//!
//! End-to-end publish/merge → `wax search` / `wax component|token` coverage for
//! each parser pack lives at the CLI binary seam in
//! `wax-cli/tests/pack_search_integration.rs`. Cross-pack merge → search/detail
//! without the CLI binary is covered in
//! `wax-core/tests/cross_pack_registry_search.rs`.

use serde_json::json;
use std::io::Cursor;
use std::path::PathBuf;
use wax_contract::{RegistryDocument, RegistryExtractionStatus, RegistrySourceKind};
use wax_lang_api::{
    DiscoverRegistryRequest, WIRE_API_VERSION, WireErrorCode, WirePackHandler, WirePackResponse,
    serve_one,
};
use wax_lang_basic::BasicWireHandler;
use wax_lang_compose::discover_registry_details as compose_details;
use wax_lang_react::discover_registry_details as react_details;
use wax_lang_swift::discover_registry_details as swift_details;

fn react_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wax-lang-react/tests/fixtures/discover")
}

fn compose_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wax-lang-compose/tests/fixtures/discover")
}

fn swift_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wax-lang-swift/tests/fixtures/rich-discover")
}

#[derive(Debug)]
struct PackEnrichment {
    language: &'static str,
    status: RegistryExtractionStatus,
    documentation: Option<String>,
    parameter_count: usize,
    parameter_description: Option<String>,
    enum_values: Vec<String>,
    best_practices: Vec<String>,
    has_deprecation: bool,
    has_example_attachment: bool,
}

fn enrich_react() -> PackEnrichment {
    let registry: RegistryDocument = serde_json::from_value(json!({
        "schema_version": 1,
        "components": [{
            "id": "react.rich-button",
            "symbol": "RichButton",
            "package": "@acme/design-system"
        }]
    }))
    .unwrap();
    let result = react_details(&DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: react_fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/src".to_owned()],
        registry,
    })
    .expect("react enrichment");
    summarize("react", &result.document)
}

fn enrich_compose() -> PackEnrichment {
    let registry: RegistryDocument = serde_json::from_value(json!({
        "schema_version": 1,
        "components": [{
            "id": "compose.rich-button",
            "symbol": "RichButton",
            "package": "com.example.ds"
        }]
    }))
    .unwrap();
    let result = compose_details(&DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: compose_fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
        registry,
    })
    .expect("compose enrichment");
    summarize("compose", &result.document)
}

fn enrich_swift() -> PackEnrichment {
    let registry: RegistryDocument = serde_json::from_value(json!({
        "schema_version": 1,
        "components": [{
            "id": "swift.rich-button",
            "symbol": "RichButton",
            "package": "AcmeDesignSystem"
        }]
    }))
    .unwrap();
    let result = swift_details(&DiscoverRegistryRequest {
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().unwrap(),
        repo_root: swift_fixture_root().to_string_lossy().into_owned(),
        roots: vec!["design-system/Sources".to_owned()],
        registry,
    })
    .expect("swift enrichment");
    summarize("swift", &result.document)
}

fn summarize(language: &'static str, document: &RegistryDocument) -> PackEnrichment {
    let extracted = document.components[0]
        .extracted
        .as_ref()
        .expect("enriched component");
    let parameters = extracted
        .signature
        .as_ref()
        .map(|signature| signature.parameters.as_slice())
        .unwrap_or_default();
    let enum_parameter = parameters
        .iter()
        .find(|parameter| !parameter.allowed_values.is_empty())
        .expect("enum parameter");
    PackEnrichment {
        language,
        status: extracted.status,
        documentation: extracted.documentation.clone(),
        parameter_count: parameters.len(),
        parameter_description: enum_parameter.description.clone(),
        enum_values: enum_parameter
            .allowed_values
            .iter()
            .map(|value| value.trim_matches('"').to_owned())
            .collect(),
        best_practices: extracted
            .best_practices
            .iter()
            .map(|practice| practice.description.clone())
            .collect(),
        has_deprecation: extracted.deprecation.is_some(),
        has_example_attachment: extracted
            .attachments
            .iter()
            .any(|attachment| attachment.kind == RegistrySourceKind::Example),
    }
}

#[test]
fn parser_packs_share_rich_button_enrichment_semantics() {
    let packs = [enrich_react(), enrich_compose(), enrich_swift()];

    for pack in &packs {
        assert_eq!(
            pack.status,
            RegistryExtractionStatus::Complete,
            "{} enrichment should be complete for directly declared API facts",
            pack.language
        );
        assert_eq!(
            pack.documentation.as_deref(),
            Some("A button used for important actions."),
            "{} should extract the shared documentation sentence",
            pack.language
        );
        assert!(
            pack.parameter_count >= 2,
            "{} should extract at least two direct parameters",
            pack.language
        );
        assert!(
            pack.parameter_description
                .as_deref()
                .is_some_and(|description| !description.is_empty()),
            "{} should describe its enum parameter",
            pack.language
        );
        assert_eq!(
            pack.enum_values.len(),
            2,
            "{} should extract both enum values",
            pack.language
        );
        assert_eq!(
            pack.best_practices.len(),
            2,
            "{} should preserve both ordered guidance items",
            pack.language
        );
        assert!(
            pack.has_deprecation,
            "{} should surface deprecation notes when present",
            pack.language
        );
        assert!(
            pack.has_example_attachment,
            "{} should materialize an example attachment",
            pack.language
        );
    }

    for pack in &packs[1..] {
        assert_eq!(
            pack.parameter_description, packs[0].parameter_description,
            "{} should match React's enum parameter description",
            pack.language
        );
        assert_eq!(
            pack.enum_values, packs[0].enum_values,
            "{} should match React's enum values",
            pack.language
        );
        assert_eq!(
            pack.best_practices, packs[0].best_practices,
            "{} should match React's ordered best practices",
            pack.language
        );
    }
}

fn assert_discover_unsupported(output: &[u8]) {
    let line = std::str::from_utf8(output).expect("utf8").trim();
    let response: WirePackResponse = serde_json::from_str(line).expect("wire response");
    match response {
        WirePackResponse::Error {
            code, language_id, ..
        } => {
            assert_eq!(code, WireErrorCode::DiscoverUnsupported);
            assert_eq!(language_id.as_str(), "basic");
        }
        other => panic!("expected discover_unsupported, got {other:?}"),
    }
}

#[test]
fn basic_pack_discovery_path_returns_discover_unsupported() {
    let handler = BasicWireHandler::new();

    let mut symbol_output = Vec::new();
    serve_one(
        Cursor::new(
            r#"{"type":"discover","api_version":1,"language_id":"basic","repo_root":"/tmp/repo","roots":["src"]}
"#,
        ),
        &mut symbol_output,
        &handler,
    )
    .expect("serve symbol discover");
    assert_discover_unsupported(&symbol_output);

    let mut rich_output = Vec::new();
    serve_one(
        Cursor::new(
            r#"{"type":"discover_registry","api_version":1,"language_id":"basic","repo_root":"/tmp/repo","roots":["src"],"registry":{"schema_version":1,"components":[]}}
"#,
        ),
        &mut rich_output,
        &handler,
    )
    .expect("serve rich discover");
    assert_discover_unsupported(&rich_output);

    assert!(!handler.registry_details_v1());
    assert_eq!(
        serde_json::to_value(WireErrorCode::DiscoverUnsupported).unwrap(),
        json!("discover_unsupported")
    );
}
