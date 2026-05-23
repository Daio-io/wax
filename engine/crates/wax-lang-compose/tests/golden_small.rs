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
