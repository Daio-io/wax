use std::path::{Path, PathBuf};

use wax_contract::{LanguageId, ScanFacts};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_react::ReactLanguage;

#[test]
fn small_fixture_matches_golden_facts_projection() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let golden = load_golden(&fixture_root.join("golden.json"));

    let request = scan_request(&fixture_root, "snap-golden-small");

    let facts = ReactLanguage::new()
        .scan(&request)
        .expect("react scan should succeed for the small fixture");

    assert_eq!(
        facts_golden_projection(&facts),
        golden,
        "scan facts drifted from golden projection"
    );
}

#[test]
fn scan_status_is_complete_when_configured() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");

    let request = scan_request(&fixture_root, "snap-status");

    let facts = ReactLanguage::new()
        .scan(&request)
        .expect("scan should succeed");

    assert_eq!(
        facts.status,
        wax_contract::ScanStatus::Complete,
        "configured react scan should report Complete status"
    );
    assert_eq!(facts.language.parser_name, "swc", "parser_name must be swc");
    assert_eq!(
        facts.language.parser_version,
        wax_lang_react::SWC_PARSER_VERSION,
        "parser_version must track the pinned SWC crate version"
    );
    assert!(facts.metrics.files_scanned > 0);
    assert!(facts.metrics.parse_extract_ms >= 1);
    assert!(
        facts
            .token_sites
            .iter()
            .any(|site| site.token_id == "color.primary" && site.parent.is_some()),
        "React token references should include parent attribution inside components"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.category == wax_contract::TokenCategory::Color
                && site.value == "\"#336699\""),
        "inline style color hex should be a color hard-coded candidate"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.category == wax_contract::TokenCategory::Spacing && site.value == "8"),
        "inline padding number should be a spacing hard-coded candidate"
    );
}

#[test]
fn alias_usage_resolves_to_canonical_symbol() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");

    let request = scan_request(&fixture_root, "snap-alias");

    let facts = ReactLanguage::new()
        .scan(&request)
        .expect("scan should succeed");

    let alias_site = facts
        .usage_sites
        .iter()
        .find(|site| site.symbol == "PrimaryBtn")
        .expect("PrimaryBtn alias usage site must be present");

    assert_eq!(
        alias_site.registry_symbol.as_deref(),
        Some("PrimaryButton"),
        "alias must resolve to canonical PrimaryButton"
    );
}

#[test]
fn wrapper_fixture_reports_local_invocations_and_parent_attribution() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wrapper");
    let request = scan_request(&fixture_root, "snap-wrapper");
    let facts = ReactLanguage::new()
        .scan(&request)
        .expect("wrapper scan should succeed");

    assert_eq!(facts.counts.raw_invocations.local, 1);
    assert_eq!(facts.counts.raw_invocations.resolved, 3);
    assert_eq!(facts.counts.raw_invocations.total, 4);

    let episode_sites: Vec<_> = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "EpisodeCard")
        .collect();
    assert_eq!(episode_sites.len(), 1);
    assert_eq!(
        episode_sites[0].match_status,
        wax_contract::MatchStatus::Local
    );
    assert!(
        episode_sites[0]
            .parent
            .as_ref()
            .is_some_and(|parent| { parent.symbol == "DiscoverScreen" })
    );

    let tier_count = facts
        .usage_sites
        .iter()
        .filter(|site| {
            site.symbol == "Tier" && site.match_status == wax_contract::MatchStatus::Resolved
        })
        .count();
    let button_count = facts
        .usage_sites
        .iter()
        .filter(|site| {
            site.symbol == "Button" && site.match_status == wax_contract::MatchStatus::Resolved
        })
        .count();
    assert_eq!(tier_count, 1);
    assert_eq!(button_count, 2);

    assert!(facts.usage_sites.iter().any(|site| {
        site.symbol == "Button"
            && site
                .parent
                .as_ref()
                .is_some_and(|parent| parent.symbol == "DiscoverScreen")
    }));
}

fn scan_request(fixture_root: &Path, snapshot_id: &str) -> ScanRequest {
    let mut config = serde_json::Map::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["src"]));
    config.insert(
        "packages".to_owned(),
        serde_json::json!({
            "@acme/design-system": {
                "exports": {
                    ".": "src/ds/index.ts"
                }
            }
        }),
    );

    ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("react").expect("react id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: snapshot_id.to_owned(),
        config,
    }
}

fn load_golden(path: &Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(path).expect("golden.json must exist");
    serde_json::from_str(&raw).expect("golden.json must be valid JSON")
}

fn facts_golden_projection(facts: &ScanFacts) -> serde_json::Value {
    let mut value = serde_json::to_value(facts).expect("facts must serialize");
    let object = value
        .as_object_mut()
        .expect("serialized facts must be an object");
    object.remove("scanned_at");
    object.remove("snapshot_id");
    if let Some(language) = object.get_mut("language").and_then(|v| v.as_object_mut()) {
        language.remove("version");
    }
    if let Some(metrics) = object.get_mut("metrics").and_then(|v| v.as_object_mut()) {
        metrics.remove("parse_extract_ms");
    }
    object.remove("symbol_usage_summary");
    value
}
