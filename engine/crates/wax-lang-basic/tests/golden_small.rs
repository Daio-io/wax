use std::path::PathBuf;

use wax_contract::LanguageId;
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_basic::BasicLanguage;

#[derive(Debug)]
struct GoldenCounts {
    raw_invocations_total: u32,
    raw_invocations_resolved: u32,
    raw_invocations_local: u32,
    registry_component_count: u32,
    registry_used_component_count: u32,
    definitions_local_definition_count: u32,
    configured_token_count: u32,
    used_token_count: u32,
    token_reference_site_count: u32,
    hardcoded_style_candidate_count: u32,
}

#[test]
fn small_fixture_matches_golden_counts() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let golden = load_golden(&fixture_root.join("golden.json"));

    let mut config = serde_json::Map::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/src"]));
    config.insert("file_extensions".to_owned(), serde_json::json!(["src"]));

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("basic").expect("basic id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: "snap-golden-small".to_owned(),
        config,
    };

    let facts = BasicLanguage::new()
        .scan(&request)
        .expect("basic scan should succeed for the small fixture");

    assert_eq!(
        facts.counts.raw_invocations.total, golden.raw_invocations_total,
        "raw_invocations.total drifted from golden"
    );
    assert_eq!(
        facts.counts.raw_invocations.resolved, golden.raw_invocations_resolved,
        "raw_invocations.resolved drifted from golden"
    );
    assert_eq!(
        facts.counts.raw_invocations.local, golden.raw_invocations_local,
        "raw_invocations.local drifted from golden"
    );
    assert_eq!(
        facts.counts.registry.component_count, golden.registry_component_count,
        "registry.component_count drifted from golden"
    );
    assert_eq!(
        facts.counts.registry.used_component_count, golden.registry_used_component_count,
        "registry.used_component_count drifted from golden"
    );
    assert_eq!(
        facts.counts.definitions.local_definition_count, golden.definitions_local_definition_count,
        "definitions.local_definition_count drifted from golden"
    );
    let usage_columns = facts
        .usage_sites
        .iter()
        .map(|site| site.location.column)
        .collect::<Vec<_>>();
    assert_eq!(
        usage_columns,
        vec![Some(5), Some(5), Some(5)],
        "usage site columns should remain one-based source columns"
    );
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|diag| diag.code == "basic_text_scan"),
        "basic scan should emit a text-scanner diagnostic"
    );
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|diag| diag.code == "basic_capability_gap"),
        "basic scan should emit capability gap diagnostics"
    );
    assert_eq!(
        facts.counts.tokens.configured_token_count, golden.configured_token_count,
        "tokens.configured_token_count drifted from golden"
    );
    assert_eq!(
        facts.counts.tokens.used_token_count, golden.used_token_count,
        "tokens.used_token_count drifted from golden"
    );
    assert_eq!(
        facts.counts.tokens.token_reference_site_count, golden.token_reference_site_count,
        "tokens.token_reference_site_count drifted from golden"
    );
    assert_eq!(
        facts.counts.tokens.hardcoded_style_candidate_count, golden.hardcoded_style_candidate_count,
        "tokens.hardcoded_style_candidate_count drifted from golden"
    );
    assert_eq!(facts.design_system_tokens.len(), 2);
    assert!(
        facts
            .token_sites
            .iter()
            .any(|site| site.token_id == "color.primary" && site.key == "Theme.colors.primary"),
        "basic scanner should find exact token key references"
    );
    assert!(
        facts
            .token_sites
            .iter()
            .any(|site| site.token_id == "space.medium" && site.key == "Spacing.Medium"),
        "basic scanner should find exact token references"
    );
    assert!(
        facts.hardcoded_style_sites.is_empty(),
        "basic scanner must not emit hard-coded styling candidates"
    );
    assert!(facts.metrics.files_scanned > 0);
    assert!(facts.metrics.parse_extract_ms >= 1);
}

fn load_golden(path: &PathBuf) -> GoldenCounts {
    let raw = std::fs::read_to_string(path).expect("golden.json must exist");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("golden.json must be valid JSON");
    GoldenCounts {
        raw_invocations_total: value["raw_invocations"]["total"]
            .as_u64()
            .expect("golden raw_invocations.total must be a number")
            as u32,
        raw_invocations_resolved: value["raw_invocations"]["resolved"]
            .as_u64()
            .expect("golden raw_invocations.resolved must be a number")
            as u32,
        raw_invocations_local: value["raw_invocations"]["local"]
            .as_u64()
            .expect("golden raw_invocations.local must be a number")
            as u32,
        registry_component_count: value["registry"]["component_count"]
            .as_u64()
            .expect("golden registry.component_count must be a number")
            as u32,
        registry_used_component_count: value["registry"]["used_component_count"]
            .as_u64()
            .expect("golden registry.used_component_count must be a number")
            as u32,
        definitions_local_definition_count: value["definitions"]["local_definition_count"]
            .as_u64()
            .expect("golden definitions.local_definition_count must be a number")
            as u32,
        configured_token_count: value["tokens"]["configured_token_count"]
            .as_u64()
            .expect("golden tokens.configured_token_count must be a number")
            as u32,
        used_token_count: value["tokens"]["used_token_count"]
            .as_u64()
            .expect("golden tokens.used_token_count must be a number")
            as u32,
        token_reference_site_count: value["tokens"]["token_reference_site_count"]
            .as_u64()
            .expect("golden tokens.token_reference_site_count must be a number")
            as u32,
        hardcoded_style_candidate_count: value["tokens"]["hardcoded_style_candidate_count"]
            .as_u64()
            .expect("golden tokens.hardcoded_style_candidate_count must be a number")
            as u32,
    }
}
