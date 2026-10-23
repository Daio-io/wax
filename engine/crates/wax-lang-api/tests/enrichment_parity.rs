//! Cross-pack enrichment parity for React, Compose, and Swift rich discovery.
//!
//! Parser packs must expose the same enrichment semantics: complete direct API
//! facts when declarations are local, and the same attachment/deprecation shape.
//! The legacy `basic` pack does not support registry discovery
//! (`WireErrorCode::DiscoverUnsupported`); engines fall back to symbol-only
//! discovery when `registry_details_v1` is false.

use serde_json::json;
use std::path::PathBuf;
use wax_contract::{RegistryDocument, RegistryExtractionStatus, RegistrySourceKind};
use wax_lang_api::{DiscoverRegistryRequest, WIRE_API_VERSION, WireErrorCode};
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
    PackEnrichment {
        language,
        status: extracted.status,
        documentation: extracted.documentation.clone(),
        parameter_count: extracted
            .signature
            .as_ref()
            .map(|signature| signature.parameters.len())
            .unwrap_or_default(),
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
}

#[test]
fn basic_pack_discovery_limitation_uses_stable_unsupported_code() {
    // `wax-lang-basic` has no rich discovery path. Agents and engines must treat
    // `discover_unsupported` as the explicit compatibility signal rather than
    // inventing enrichment for the legacy pack.
    assert_eq!(
        serde_json::to_value(WireErrorCode::DiscoverUnsupported).unwrap(),
        json!("discover_unsupported")
    );
}
