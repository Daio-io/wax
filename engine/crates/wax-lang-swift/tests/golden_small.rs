use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{MatchStatus, ScanFacts, ScanStatus};
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
        facts.counts.raw_invocations.total,
        golden["raw_invocations"]["total"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.raw_invocations.resolved,
        golden["raw_invocations"]["resolved"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.raw_invocations.local,
        golden["raw_invocations"]["local"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.definitions.local_definition_count,
        golden["definitions"]["local_definition_count"]
            .as_u64()
            .unwrap() as u32
    );
    assert_eq!(
        facts.counts.registry.component_count,
        golden["registry"]["component_count"].as_u64().unwrap() as u32
    );
    let alias_sites = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "PrimaryCTA")
        .collect::<Vec<_>>();
    assert_eq!(
        alias_sites.len(),
        2,
        "expected qualified and unqualified PrimaryCTA alias usages"
    );
    assert!(
        alias_sites
            .iter()
            .all(|site| site.registry_symbol.as_deref() == Some("PrimaryButton")),
        "all PrimaryCTA alias usages must resolve to PrimaryButton"
    );
    assert!(
        facts
            .token_sites
            .iter()
            .any(|site| site.token_id == "color.primary" && site.parent.is_some()),
        "SwiftUI token references should include parent attribution inside views"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.category == wax_contract::TokenCategory::Color
                && site.value.contains("Color")),
        "SwiftUI Color(...) should be a color hard-coded candidate"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.category == wax_contract::TokenCategory::Radius && site.value == "8"),
        "cornerRadius(8) should be a radius hard-coded candidate"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.category == wax_contract::TokenCategory::Spacing && site.value == "12"),
        "VStack(spacing: 12) should be a spacing hard-coded candidate"
    );
    assert!(
        facts.hardcoded_style_sites.iter().any(|site| {
            site.category == wax_contract::TokenCategory::Typography && site.value == "14"
        }),
        ".font(.system(size: 14)) should be a typography hard-coded candidate"
    );
    assert_eq!(
        facts
            .token_sites
            .iter()
            .filter(|site| site.token_id == "color.primary")
            .count(),
        1,
        "overlapping alias matches must collapse to one token site"
    );
}

#[test]
fn non_ds_import_is_not_counted_when_registry_package_is_set() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let registry_dir = tmp.path().join("design-system");
    std::fs::create_dir_all(&registry_dir).unwrap();
    std::fs::write(
        registry_dir.join("registry.json"),
        r#"{
            "schema_version": 1,
            "components": [
                {
                    "id": "ds.btn",
                    "symbol": "Button",
                    "package": "AcmeDesignSystem"
                }
            ]
        }"#,
    )
    .unwrap();

    let source_dir = tmp.path().join("app/Sources");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::write(
        source_dir.join("Screen.swift"),
        r#"
import SwiftUI

struct Screen: View {
    var body: some View {
        Button("Save") {}
    }
}
"#,
    )
    .unwrap();

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
        repo_root: tmp.path().to_string_lossy().to_string(),
        snapshot_id: "snap-swift-non-ds-import".to_owned(),
        config,
    };

    let facts = SwiftLanguage::new().scan(&request).unwrap();

    assert_eq!(facts.counts.raw_invocations.total, 0);
    assert_eq!(facts.counts.raw_invocations.resolved, 0);
    assert_eq!(facts.counts.raw_invocations.candidate, 0);
}

#[test]
fn scan_status_is_complete_when_configured() {
    let facts = scan_small_fixture();

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.language.parser_name, "tree-sitter-swift");
    assert!(facts.metrics.files_scanned > 0);
    assert!(facts.metrics.parse_extract_ms >= 1);
}

#[test]
fn alias_usage_resolves_to_canonical_symbol() {
    let facts = scan_small_fixture();
    let alias_sites = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "PrimaryCTA")
        .collect::<Vec<_>>();

    assert_eq!(
        alias_sites.len(),
        2,
        "Sample.swift and Extended.swift each contribute one PrimaryCTA alias usage"
    );
    assert!(
        alias_sites
            .iter()
            .all(|site| site.registry_symbol.as_deref() == Some("PrimaryButton")),
        "all PrimaryCTA alias usages must resolve to PrimaryButton"
    );
}

#[test]
fn wrapper_fixture_reports_local_invocations_and_parent_attribution() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wrapper");
    let facts = scan_fixture(&fixture_root, "app/Sources", "snap-wrapper");

    assert_eq!(facts.counts.raw_invocations.local, 1);
    assert_eq!(facts.counts.raw_invocations.resolved, 3);
    assert_eq!(facts.counts.raw_invocations.total, 4);

    let episode_sites: Vec<_> = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "EpisodeCardView")
        .collect();
    assert_eq!(episode_sites.len(), 1);
    assert_eq!(episode_sites[0].match_status, MatchStatus::Local);
    assert!(
        episode_sites[0]
            .parent
            .as_ref()
            .is_some_and(|parent| { parent.symbol == "DiscoverView" })
    );

    let tier_count = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "Tier" && site.match_status == MatchStatus::Resolved)
        .count();
    let button_count = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "Button" && site.match_status == MatchStatus::Resolved)
        .count();
    assert_eq!(tier_count, 1);
    assert_eq!(button_count, 2);

    assert!(facts.usage_sites.iter().any(|site| {
        site.symbol == "Button"
            && site
                .parent
                .as_ref()
                .is_some_and(|parent| parent.symbol == "DiscoverView")
    }));
}

fn scan_fixture(fixture_root: &Path, roots: &str, snapshot_id: &str) -> ScanFacts {
    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::json!("design-system/registry.json"),
    );
    config.insert("roots".to_owned(), serde_json::json!([roots]));

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().unwrap(),
        repo_root: fixture_root.to_string_lossy().to_string(),
        snapshot_id: snapshot_id.to_owned(),
        config,
    };

    SwiftLanguage::new().scan(&request).unwrap()
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
