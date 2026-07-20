use std::path::{Path, PathBuf};

use wax_contract::{LanguageId, ScanFacts, StyleContext, TokenCategory};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_react::{
    ReactConfigMode, ReactLanguage, collect_react_source_files, configured_scan_facts,
    load_react_registry, parse_react_scan_config,
};

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
    assert_style_context(
        &facts,
        TokenCategory::Color,
        StyleContext::Color,
        "\"#336699\"",
    );
    assert_style_context(
        &facts,
        TokenCategory::Spacing,
        StyleContext::Padding,
        "\"4px\"",
    );
    assert_style_context(
        &facts,
        TokenCategory::Spacing,
        StyleContext::Margin,
        "\"4px\"",
    );
    assert_style_context(&facts, TokenCategory::Spacing, StyleContext::Gap, "\"4px\"");
    assert_style_context(&facts, TokenCategory::Spacing, StyleContext::Width, "200");
    assert_style_context(&facts, TokenCategory::Spacing, StyleContext::Height, "40");
    assert_style_context(
        &facts,
        TokenCategory::Typography,
        StyleContext::Typography,
        "\"4px\"",
    );
    assert_style_context(
        &facts,
        TokenCategory::Radius,
        StyleContext::Radius,
        "\"4px\"",
    );
    assert_style_context(
        &facts,
        TokenCategory::Elevation,
        StyleContext::Elevation,
        "\"0 1px 2px #000\"",
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.context == StyleContext::Width && site.value == "200"),
        "fixed width 200 must remain a raw hard-coded site"
    );
    assert!(
        !facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.value == "200" && site.context != StyleContext::Width),
        "non-style ordinary numbers must stay absent"
    );
}

fn assert_style_context(
    facts: &ScanFacts,
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) {
    assert!(
        facts.hardcoded_style_sites.iter().any(|site| {
            site.category == category && site.context == context && site.value == value
        }),
        "expected {category:?}/{context:?} value={value}, got: {:?}",
        facts.hardcoded_style_sites
    );
}

#[test]
fn configured_scan_facts_records_timing_when_files_are_scanned() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let request = scan_request(&fixture_root, "snap-public-configured-facts");
    let facts = direct_configured_scan_facts(&fixture_root, &request);

    assert!(
        facts.metrics.parse_extract_ms >= 1,
        "public configured facts should record timing for {} scanned files",
        facts.metrics.files_scanned
    );
}

#[test]
fn configured_scan_facts_reports_zero_timing_when_no_files_are_scanned() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let mut request = scan_request(&fixture_root, "snap-public-configured-facts-empty");
    request
        .config
        .insert("roots".to_owned(), serde_json::json!(["missing"]));

    let facts = direct_configured_scan_facts(&fixture_root, &request);

    assert_eq!(facts.metrics.parse_extract_ms, 0);
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

fn direct_configured_scan_facts(fixture_root: &Path, request: &ScanRequest) -> ScanFacts {
    let config = match parse_react_scan_config(&request.config).expect("config should parse") {
        ReactConfigMode::Configured(config) => config,
        ReactConfigMode::Scaffold => panic!("fixture config should be configured"),
    };
    let registry = load_react_registry(&fixture_root.join(&config.design_system_registry))
        .expect("registry should load");
    let collection = collect_react_source_files(fixture_root, &config.roots, &config.ignore)
        .expect("source files should collect");

    configured_scan_facts(
        request,
        &request.language_id,
        registry,
        collection,
        fixture_root,
        &config,
    )
    .expect("configured facts should assemble")
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
