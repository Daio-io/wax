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
    assert_eq!(
        facts.counts.framework_shadow_count,
        golden
            .get("framework_shadow_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32
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
        !facts
            .design_system_components
            .iter()
            .any(|component| component.symbol == "ComposeOnly")
    );
}

#[test]
fn scan_reports_framework_shadow_count_for_configured_framework_imports() {
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
                    "package": "AcmeDesignSystem",
                    "targets": ["swift"]
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
    config.insert(
        "framework_packages".to_owned(),
        serde_json::json!(["SwiftUI"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: "swift".try_into().unwrap(),
        repo_root: tmp.path().to_string_lossy().to_string(),
        snapshot_id: "snap-swift-framework-shadow".to_owned(),
        config,
    };

    let facts = SwiftLanguage::new().scan(&request).unwrap();

    assert_eq!(facts.counts.usage_site_count, 1);
    assert_eq!(facts.counts.framework_shadow_count, 1);
    assert_eq!(facts.counts.resolved_count, 0);
    assert_eq!(facts.counts.candidate_count, 0);
    assert_eq!(
        facts.usage_sites[0].match_status,
        wax_contract::MatchStatus::FrameworkShadow
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
