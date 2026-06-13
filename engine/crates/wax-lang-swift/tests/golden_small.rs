use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{ScanFacts, ScanStatus};
use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_swift::SwiftLanguage;

#[test]
fn golden_small_swiftui_fixture_matches_counts() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let golden: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(fixture.join("golden.json")).unwrap()).unwrap();

    let facts = scan_small_fixture();

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(
        facts.counts.usage_site_count,
        golden["usage_site_count"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.resolved_count,
        golden["resolved_count"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.local_component_count,
        golden["local_component_count"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.design_system_component_count,
        golden["design_system_component_count"].as_u64().unwrap() as u32
    );
    assert!(facts.usage_sites.iter().any(|site| {
        site.symbol == "PrimaryCTA" && site.registry_symbol.as_deref() == Some("PrimaryButton")
    }));
    assert!(
        !facts
            .design_system_components
            .iter()
            .any(|component| component.symbol == "ComposeOnly")
    );
}

#[test]
fn scan_status_is_complete_when_configured() {
    let facts = scan_small_fixture();

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.language.parser_name, "tree-sitter-swift");
}

#[test]
fn alias_usage_resolves_to_canonical_symbol() {
    let facts = scan_small_fixture();
    let alias_site = facts
        .usage_sites
        .iter()
        .find(|site| site.symbol == "PrimaryCTA")
        .expect("alias usage should be present");

    assert_eq!(alias_site.registry_symbol.as_deref(), Some("PrimaryButton"));
}

fn scan_small_fixture() -> ScanFacts {
    let fixture = fixture_root();
    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::json!("design-system/registry.json"),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/Sources"]));

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().unwrap(),
        repo_root: fixture.to_string_lossy().to_string(),
        snapshot_id: "snap-swift-golden".to_owned(),
        config,
    };

    SwiftLanguage::new().scan(&request).unwrap()
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small")
}
