use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageMetadata, MatchStatus, Metrics,
    ScanFacts, ScanStatus, UsageSite, SCHEMA_VERSION,
};

fn minimal_facts() -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: "compose".into(),
            version: "0.0.0".into(),
            ecosystem: "jetpack-compose".into(),
            parser: "tree-sitter-kotlin".into(),
        },
        snapshot_id: "test-snapshot".into(),
        status: ScanStatus::Complete,
        design_system_components: vec![],
        local_components: vec![],
        usage_sites: vec![UsageSite {
            id: "a:1:Button:resolved".into(),
            file: "a.kt".into(),
            line: 1,
            symbol: "Button".into(),
            match_status: MatchStatus::Resolved,
            registry_symbol: Some("com.ds.Button".into()),
        }],
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "W001".into(),
            message: "example".into(),
        }],
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 12,
            files_scanned: 1,
        },
        counts: CountSummary {
            design_system_component_count: 0,
            local_component_count: 0,
            usage_site_count: 0,
            resolved_count: 0,
            candidate_count: 0,
        },
    }
}

#[test]
fn scan_facts_roundtrip() {
    let mut facts = minimal_facts();
    facts.recompute_counts();
    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(facts.language.id, back.language.id);
    assert_eq!(facts.counts.resolved_count, 1);
    assert_eq!(back.metrics.adoption_coverage_ratio, Some(1.0));
}

#[test]
fn rejects_wrong_schema_version() {
    let mut facts = minimal_facts();
    facts.schema_version = 999;
    let json = serde_json::to_string(&facts).unwrap();
    assert!(wax_contract::scan_facts_from_json(&json).is_err());
}
