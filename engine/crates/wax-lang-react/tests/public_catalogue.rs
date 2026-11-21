use std::path::Path;
use wax_contract::MatchStatus;
use wax_lang_api::{ScanRequest, ScanRequestType, WIRE_API_VERSION};
use wax_lang_react::ReactLanguage;

#[test]
fn pinned_shadcn_pilot_requires_exact_local_import_alias_and_keeps_unknown_provenance_candidate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/public-catalogue/shadcn");
    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: WIRE_API_VERSION,
        language_id: "react".try_into().unwrap(),
        repo_root: root.display().to_string(),
        snapshot_id: "public-shadcn".into(),
        config: serde_json::json!({"registry":"react.registry.json","roots":["consumer"]})
            .as_object()
            .unwrap()
            .clone(),
    };
    let facts = ReactLanguage::new().scan(&request).unwrap();
    let sites: Vec<_> = facts
        .usage_sites
        .iter()
        .filter(|site| site.symbol == "Button")
        .map(|site| (site.location.file.as_str(), site.match_status))
        .collect();
    assert_eq!(
        sites,
        vec![
            ("consumer/positive.tsx", MatchStatus::Resolved),
            ("consumer/unknown.tsx", MatchStatus::Candidate),
            ("consumer/wrong-package.tsx", MatchStatus::Unresolved),
        ]
    );
    assert_eq!(facts.counts.registry.component_count, 353);
    assert!(facts.design_system_tokens.is_empty());
    assert!(facts.token_sites.is_empty());
}
