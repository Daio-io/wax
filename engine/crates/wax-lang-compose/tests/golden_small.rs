use std::path::{Path, PathBuf};

use wax_contract::{CalleeOrigin, LanguageId, MatchStatus, StyleContext, TokenCategory};
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_compose::ComposeLanguage;

#[derive(Debug)]
struct GoldenCounts {
    raw_invocations_total: u32,
    raw_invocations_resolved: u32,
    raw_invocations_local: u32,
    raw_invocations_unresolved: u32,
    registry_component_count: u32,
    definitions_local_definition_count: u32,
    definitions_invoked_local_definition_count: u32,
    framework_origin_count: u32,
    external_origin_count: u32,
    application_origin_count: u32,
    eligible_invocation_count: u32,
    adoption_excluded_invocation_count: u32,
    configured_token_count: u32,
    used_token_count: u32,
    token_reference_site_count: u32,
    hardcoded_style_candidate_count: u32,
}

#[test]
fn small_fixture_matches_golden_counts() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let golden = load_golden(&fixture_root.join("golden.json"));

    let facts = scan_fixture(&fixture_root, "app/src/main/kotlin", "snap-golden-small");

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
        facts.counts.raw_invocations.unresolved, golden.raw_invocations_unresolved,
        "raw_invocations.unresolved drifted from golden"
    );
    assert_eq!(
        facts.counts.registry.component_count, golden.registry_component_count,
        "registry.component_count drifted from golden"
    );
    assert_eq!(
        facts.counts.definitions.local_definition_count, golden.definitions_local_definition_count,
        "definitions.local_definition_count drifted from golden"
    );
    assert_eq!(
        facts.counts.definitions.invoked_local_definition_count,
        golden.definitions_invoked_local_definition_count,
        "definitions.invoked_local_definition_count drifted from golden"
    );
    assert_eq!(
        facts.counts.invocation_origins.registry,
        golden.raw_invocations_resolved
    );
    assert_eq!(
        facts.counts.invocation_origins.framework,
        golden.framework_origin_count
    );
    assert_eq!(
        facts.counts.invocation_origins.external,
        golden.external_origin_count
    );
    assert_eq!(
        facts.counts.invocation_origins.application,
        golden.application_origin_count
    );
    assert_eq!(
        facts.counts.adoption.eligible_invocation_count,
        golden.eligible_invocation_count
    );
    assert_eq!(
        facts.counts.adoption.adoption_excluded_invocation_count,
        golden.adoption_excluded_invocation_count
    );
    assert_eq!(
        facts.counts.adoption.adoption_excluded_invocation_count,
        facts.counts.invocation_origins.framework + facts.counts.invocation_origins.external,
        "excluded count must equal framework + external origins"
    );
    assert_eq!(
        facts.counts.raw_invocations.unresolved,
        facts.counts.invocation_origins.framework
            + facts.counts.invocation_origins.external
            + facts.counts.invocation_origins.application
            + facts.counts.invocation_origins.unknown,
        "framework/external remain in raw unresolved even when excluded from adoption"
    );
    assert!(
        facts
            .usage_sites
            .iter()
            .any(|site| site.symbol == "AsyncImage" && site.callee_origin == CalleeOrigin::External),
        "Coil AsyncImage must remain a navigable external usage site"
    );
    assert!(
        facts.usage_sites.iter().any(|site| {
            site.symbol == "UnknownProjectCard" && site.callee_origin == CalleeOrigin::Application
        }),
        "unknown project calls must remain eligible application debt"
    );
    assert!(
        facts
            .usage_sites
            .iter()
            .any(|site| site.symbol == "Box" && site.callee_origin == CalleeOrigin::Framework),
        "AndroidX primitives must remain navigable framework usage sites"
    );
    assert_eq!(
        facts.metrics.invocation_adoption_ratio,
        Some(5.0 / 7.0),
        "mobile adoption must not be dominated by AndroidX/third-party calls"
    );
    assert_eq!(
        facts.metrics.registry_resolution_ratio,
        Some(5.0 / 14.0),
        "registry_resolution_ratio remains a raw diagnostic over all invocations"
    );

    let usage_columns = facts
        .usage_sites
        .iter()
        .filter(|site| site.match_status == MatchStatus::Resolved)
        .map(|site| site.location.column)
        .collect::<Vec<_>>();
    assert!(
        usage_columns.iter().all(|c| *c == Some(5)),
        "expected all resolved usage site columns to be Some(5) (one-based), got: {usage_columns:?}"
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
            .any(|site| site.token_id == "color.primary" && site.parent.is_some()),
        "Compose token references should include parent attribution when inside a composable"
    );
    assert_style_context(
        &facts,
        TokenCategory::Spacing,
        StyleContext::Padding,
        "4.dp",
    );
    assert_style_context(&facts, TokenCategory::Spacing, StyleContext::Gap, "4.dp");
    assert_style_context(
        &facts,
        TokenCategory::Spacing,
        StyleContext::Width,
        "200.dp",
    );
    assert_style_context(
        &facts,
        TokenCategory::Spacing,
        StyleContext::Height,
        "40.dp",
    );
    assert_style_context(&facts, TokenCategory::Spacing, StyleContext::Size, "4.dp");
    assert_style_context(&facts, TokenCategory::Radius, StyleContext::Radius, "4.dp");
    assert_style_context(
        &facts,
        TokenCategory::Typography,
        StyleContext::Typography,
        "4.sp",
    );
    assert_style_context(
        &facts,
        TokenCategory::Elevation,
        StyleContext::Elevation,
        "4.dp",
    );
    assert!(
        facts.hardcoded_style_sites.iter().any(|site| {
            site.category == TokenCategory::Color
                && site.context == StyleContext::Color
                && site.value.contains("0x")
        }),
        "Color(0x...) should be a color hard-coded candidate with Color context"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .any(|site| site.context == StyleContext::Width && site.value == "200.dp"),
        "fixed width 200.dp must remain a raw hard-coded site"
    );
    assert!(
        facts
            .hardcoded_style_sites
            .iter()
            .all(|site| site.value != "99.dp" && !site.value.contains("0xFF000000")),
        "preview composable hard-coded styles must stay absent"
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
    facts: &wax_contract::ScanFacts,
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
fn scan_status_is_complete_when_configured() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let facts = scan_fixture(&fixture_root, "app/src/main/kotlin", "snap-status");

    assert_eq!(
        facts.status,
        wax_contract::ScanStatus::Complete,
        "tree-sitter scan should report Complete status"
    );
    assert_eq!(
        facts.language.parser_name, "tree-sitter-kotlin-ng",
        "parser_name must be tree-sitter-kotlin-ng"
    );
    assert!(facts.metrics.files_scanned > 0);
    assert!(facts.metrics.parse_extract_ms >= 1);
}

#[test]
fn alias_usage_resolves_to_canonical_symbol() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let facts = scan_fixture(&fixture_root, "app/src/main/kotlin", "snap-alias");

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

#[test]
fn wrapper_fixture_reports_local_invocations_and_parent_attribution() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wrapper");
    let facts = scan_fixture(&fixture_root, "app/src/main/kotlin", "snap-wrapper");

    assert_eq!(facts.counts.raw_invocations.local, 2);
    assert_eq!(facts.counts.raw_invocations.resolved, 2);
    assert_eq!(facts.counts.raw_invocations.total, 4);

    let episode_sites: Vec<_> = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "EpisodeCard")
        .collect();
    assert_eq!(episode_sites.len(), 2);
    assert!(
        episode_sites
            .iter()
            .all(|site| site.match_status == MatchStatus::Local)
    );
    assert!(episode_sites.iter().all(|site| {
        site.parent
            .as_ref()
            .is_some_and(|parent| parent.symbol == "DiscoverScreen")
    }));

    let tier_count = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "Tier" && site.match_status == MatchStatus::Resolved)
        .count();
    let body_text_count = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "BodyText" && site.match_status == MatchStatus::Resolved)
        .count();
    assert_eq!(tier_count, 1);
    assert_eq!(body_text_count, 1);

    assert!(facts.usage_sites.iter().any(|site| {
        site.symbol == "Tier"
            && site
                .parent
                .as_ref()
                .is_some_and(|parent| parent.symbol == "EpisodeCard")
    }));
}

#[test]
fn slot_fixture_attributes_calls_to_enclosing_composable() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slot");
    let facts = scan_fixture(&fixture_root, "app/src/main/kotlin", "snap-slot");

    for symbol in ["Button", "Tier"] {
        let sites: Vec<_> = facts
            .usage_sites
            .iter()
            .filter(|site| site.symbol == symbol)
            .collect();
        assert_eq!(sites.len(), 2, "{symbol} should have two invocations");
        assert!(sites.iter().all(|site| {
            site.parent
                .as_ref()
                .is_some_and(|parent| parent.symbol == "DiscoverScreen")
        }));
    }
}

fn scan_fixture(fixture_root: &Path, roots: &str, snapshot_id: &str) -> wax_contract::ScanFacts {
    let mut config = serde_json::Map::new();
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String("design-system/registry.json".to_owned()),
    );
    config.insert("roots".to_owned(), serde_json::json!([roots]));

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::try_from("compose").expect("compose id must be valid"),
        repo_root: fixture_root.display().to_string(),
        snapshot_id: snapshot_id.to_owned(),
        config,
    };

    ComposeLanguage::new()
        .scan(&request)
        .expect("compose scan should succeed")
}

fn load_golden(path: &Path) -> GoldenCounts {
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
        raw_invocations_unresolved: value["raw_invocations"]["unresolved"]
            .as_u64()
            .expect("golden raw_invocations.unresolved must be a number")
            as u32,
        registry_component_count: value["registry"]["component_count"]
            .as_u64()
            .expect("golden registry.component_count must be a number")
            as u32,
        definitions_local_definition_count: value["definitions"]["local_definition_count"]
            .as_u64()
            .expect("golden definitions.local_definition_count must be a number")
            as u32,
        definitions_invoked_local_definition_count:
            value["definitions"]["invoked_local_definition_count"]
                .as_u64()
                .expect("golden definitions.invoked_local_definition_count must be a number")
                as u32,
        framework_origin_count: value["invocation_origins"]["framework"]
            .as_u64()
            .expect("golden invocation_origins.framework must be a number")
            as u32,
        external_origin_count: value["invocation_origins"]["external"]
            .as_u64()
            .expect("golden invocation_origins.external must be a number")
            as u32,
        application_origin_count: value["invocation_origins"]["application"]
            .as_u64()
            .expect("golden invocation_origins.application must be a number")
            as u32,
        eligible_invocation_count: value["adoption"]["eligible_invocation_count"]
            .as_u64()
            .expect("golden adoption.eligible_invocation_count must be a number")
            as u32,
        adoption_excluded_invocation_count: value["adoption"]["adoption_excluded_invocation_count"]
            .as_u64()
            .expect("golden adoption.adoption_excluded_invocation_count must be a number")
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
