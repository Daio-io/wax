use std::path::PathBuf;

use wax_contract::LanguageId;
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_compose::ComposeLanguage;

#[derive(Debug)]
struct GoldenCounts {
    usage_site_count: u32,
    resolved_count: u32,
    local_component_count: u32,
    design_system_component_count: u32,
}

#[test]
fn small_fixture_matches_golden_counts() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let golden = load_golden(&fixture_root.join("golden.json"));

    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-golden-small".to_owned(),
        config,
    };

    let facts = ComposeLanguage::new()
        .scan(&request)
        .expect("compose scan should succeed for the small fixture");

    assert_eq!(
        facts.counts.usage_site_count, golden.usage_site_count,
        "usage_site_count drifted from golden"
    );
    assert_eq!(
        facts.counts.resolved_count, golden.resolved_count,
        "resolved_count drifted from golden"
    );
    assert_eq!(
        facts.counts.local_component_count, golden.local_component_count,
        "local_component_count drifted from golden"
    );
    assert_eq!(
        facts.counts.design_system_component_count, golden.design_system_component_count,
        "design_system_component_count drifted from golden"
    );

    // All usage sites are at 4-space indent → one-based column 5.
    let usage_columns = facts
        .usage_sites
        .iter()
        .map(|site| site.location.column)
        .collect::<Vec<_>>();
    assert!(
        usage_columns.iter().all(|c| *c == Some(5)),
        "expected all usage site columns to be Some(5) (one-based), got: {usage_columns:?}"
    );
}

#[test]
fn scan_status_is_complete_when_configured() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");

    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-status".to_owned(),
        config,
    };

    let facts = ComposeLanguage::new()
        .scan(&request)
        .expect("scan should succeed");

    assert_eq!(
        facts.status,
        wax_contract::ScanStatus::Complete,
        "tree-sitter scan should report Complete status"
    );
    assert_eq!(
        facts.language.parser_name, "tree-sitter-kotlin",
        "parser_name must be tree-sitter-kotlin"
    );
}

#[test]
fn alias_usage_resolves_to_canonical_symbol() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");

    let mut config = serde_json::Map::new();
    config.insert(
        "design_system_registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert(
        "roots".to_owned(),
        serde_json::json!(["app/src/main/kotlin"]),
    );

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-alias".to_owned(),
        config,
    };

    let facts = ComposeLanguage::new()
        .scan(&request)
        .expect("scan should succeed");

    let alias_site = facts
        .usage_sites
        .iter()
        .find(|s| s.symbol == "PrimaryBtn")
        .expect("PrimaryBtn alias usage site must be present");

    assert_eq!(
        alias_site.registry_symbol.as_deref(),
        Some("PrimaryButton"),
        "alias must resolve to canonical PrimaryButton"
    );
}

fn load_golden(path: &PathBuf) -> GoldenCounts {
    let raw = std::fs::read_to_string(path).expect("golden.json must exist");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("golden.json must be valid JSON");
    GoldenCounts {
        usage_site_count: value["usage_site_count"]
            .as_u64()
            .expect("golden.usage_site_count must be a number") as u32,
        resolved_count: value["resolved_count"]
            .as_u64()
            .expect("golden.resolved_count must be a number") as u32,
        local_component_count: value["local_component_count"]
            .as_u64()
            .expect("golden.local_component_count must be a number")
            as u32,
        design_system_component_count: value["design_system_component_count"]
            .as_u64()
            .expect("golden.design_system_component_count must be a number")
            as u32,
    }
}
