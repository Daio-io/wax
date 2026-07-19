use std::collections::BTreeMap;

use time::macros::datetime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, IdentityStability, LanguageId, LanguageMetadata,
    MatchStatus, MergedScan, Metrics, ParentScope, RepoSummary, SCHEMA_VERSION, ScanFacts,
    ScanStatus, SourceLocation, SymbolKind, SymbolUsageSummary, TokenInferenceReport, UsageSite,
};

fn scan_facts_schema() -> jsonschema::Validator {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/scan-facts.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn assert_schema_rejects(value: &serde_json::Value) {
    let validator = scan_facts_schema();
    let errors = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(
        !errors.is_empty(),
        "expected schema rejection, but value was valid: {errors:?}"
    );
}

fn empty_counts() -> CountSummary {
    CountSummary::default()
}

fn minimal_facts() -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("compose").unwrap(),
            version: "0.0.0".into(),
            ecosystem: "jetpack-compose".into(),
            parser_name: "tree-sitter-kotlin".into(),
            parser_version: "0.3.8".into(),
        },
        snapshot_id: "test-snapshot".into(),
        scanned_at: datetime!(2026-05-16 12:00 UTC),
        status: ScanStatus::Complete,
        design_system_components: vec![],
        local_components: vec![],
        usage_sites: vec![
            UsageSite {
                id: "a:1:Button:resolved".into(),
                location: SourceLocation {
                    file: "a.kt".into(),
                    line: 1,
                    column: Some(5),
                },
                symbol: "Button".into(),
                qualified_symbol: None,
                match_status: MatchStatus::Resolved,
                registry_symbol: Some("com.ds.Button".into()),
                local_definition_id: None,
                parent: None,
            },
            UsageSite {
                id: "a:2:Card:candidate".into(),
                location: SourceLocation {
                    file: "a.kt".into(),
                    line: 2,
                    column: None,
                },
                symbol: "Card".into(),
                qualified_symbol: None,
                match_status: MatchStatus::Candidate,
                registry_symbol: Some("com.ds.Card".into()),
                local_definition_id: None,
                parent: None,
            },
        ],
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "W001".into(),
            message: "example".into(),
            location: None,
        }],
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            parse_extract_ms: 12,
            files_scanned: 1,
        },
        counts: empty_counts(),
        symbol_usage_summary: vec![],
        design_system_tokens: vec![],
        token_sites: vec![],
        hardcoded_style_sites: vec![],
        token_usage_summary: vec![],
    }
}

#[test]
fn scan_facts_roundtrip() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(facts, back);
    assert_eq!(back.metrics.invocation_adoption_ratio, Some(1.0));
    assert_eq!(back.metrics.registry_resolution_ratio, Some(0.5));
    assert_eq!(back.counts.raw_invocations.total, 2);
    assert_eq!(back.counts.raw_invocations.resolved, 1);
    assert_eq!(back.counts.raw_invocations.candidate, 1);
}

#[test]
fn schema_v2_local_usage_and_symbol_summary_roundtrip() {
    let mut facts = minimal_facts();
    facts.local_components.push(wax_contract::LocalComponent {
        id: "local.compose:com.example.EpisodeCard".into(),
        symbol: "EpisodeCard".into(),
        qualified_symbol: Some("com.example.EpisodeCard".into()),
        identity_basis: Some("package_qualified_symbol".into()),
        identity_stability: Some(IdentityStability::Semantic),
        location: SourceLocation {
            file: "src/EpisodeCard.kt".into(),
            line: 1,
            column: Some(1),
        },
    });
    facts.usage_sites = vec![UsageSite {
        id: "usage.compose:src/Discover.kt:4:5:EpisodeCard".into(),
        location: SourceLocation {
            file: "src/Discover.kt".into(),
            line: 4,
            column: Some(5),
        },
        symbol: "EpisodeCard".into(),
        qualified_symbol: Some("com.example.EpisodeCard".into()),
        match_status: MatchStatus::Local,
        registry_symbol: None,
        local_definition_id: Some("local.compose:com.example.EpisodeCard".into()),
        parent: Some(ParentScope {
            parent_id: "compose:composable:com.example.DiscoverScreen".into(),
            symbol: "DiscoverScreen".into(),
            qualified_symbol: Some("com.example.DiscoverScreen".into()),
            scope_kind: "composable".into(),
            identity_basis: "package_qualified_symbol".into(),
            identity_stability: IdentityStability::Semantic,
            location: Some(SourceLocation {
                file: "src/Discover.kt".into(),
                line: 2,
                column: Some(1),
            }),
        }),
    }];
    facts.symbol_usage_summary = vec![SymbolUsageSummary {
        symbol_id: "compose:local:com.example.EpisodeCard".into(),
        symbol: "EpisodeCard".into(),
        qualified_symbol: Some("com.example.EpisodeCard".into()),
        symbol_kind: SymbolKind::Local,
        match_status: MatchStatus::Local,
        registry_symbol: None,
        local_definition_id: Some("local.compose:com.example.EpisodeCard".into()),
        identity_basis: "package_qualified_symbol".into(),
        identity_stability: IdentityStability::Semantic,
        raw_invocation_count: 1,
        parent_scope_count: 1,
        file_count: 1,
        parent_scopes: vec![],
        parent_scope_limit: Some(0),
        parent_scopes_truncated: true,
    }];
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(back.usage_sites[0].match_status, MatchStatus::Local);
    assert_eq!(back.symbol_usage_summary.len(), 1);
    assert!(back.symbol_usage_summary[0].parent_scopes_truncated);

    let value = serde_json::to_value(&back).unwrap();
    assert!(scan_facts_schema().is_valid(&value));
}

#[test]
fn accepts_explicit_null_parent_scope_limit() {
    let mut facts = minimal_facts();
    facts.symbol_usage_summary = vec![SymbolUsageSummary {
        symbol_id: "compose:registry:com.ds.Button".into(),
        symbol: "Button".into(),
        qualified_symbol: None,
        symbol_kind: SymbolKind::Registry,
        match_status: MatchStatus::Resolved,
        registry_symbol: Some("com.ds.Button".into()),
        local_definition_id: None,
        identity_basis: "registry_id".into(),
        identity_stability: IdentityStability::Semantic,
        raw_invocation_count: 1,
        parent_scope_count: 0,
        file_count: 1,
        parent_scopes: vec![],
        parent_scope_limit: None,
        parent_scopes_truncated: false,
    }];
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert!(scan_facts_schema().is_valid(&value));
    let back = wax_contract::scan_facts_from_json(&value.to_string()).unwrap();
    assert_eq!(back.symbol_usage_summary[0].parent_scope_limit, None);
}

#[test]
fn serialized_scan_facts_validate_against_schema() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    let validator = scan_facts_schema();

    assert!(validator.is_valid(&value));
}

#[test]
fn schema_rejects_values_outside_integer_bounds() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut value = serde_json::to_value(&facts).unwrap();
    value["usage_sites"][0]["location"]["line"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["usage_sites"][0]["location"]["column"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["metrics"]["files_scanned"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["metrics"]["parse_extract_ms"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);

    let mut value = serde_json::to_value(&facts).unwrap();
    value["counts"]["raw_invocations"]["total"] = serde_json::json!(4_294_967_296_u64);
    assert_schema_rejects(&value);
}

#[test]
fn rejects_invalid_language_id() {
    assert!(LanguageId::try_from("Compose").is_err());
    assert!(LanguageId::try_from("1compose").is_err());
    assert!(LanguageId::try_from("").is_err());
}

#[test]
fn rejects_invalid_language_id_from_json() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["language"]["id"] = serde_json::json!("Compose");

    assert_schema_rejects(&value);
    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_unsupported_schema_version() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["schema_version"] = serde_json::json!(999);

    let err = wax_contract::scan_facts_from_json(&value.to_string()).unwrap_err();

    assert!(matches!(
        err,
        wax_contract::ScanFactsError::UnsupportedSchemaVersion {
            found: 999,
            supported: SCHEMA_VERSION
        }
    ));
}

#[test]
fn rejects_v1_schema_version() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["schema_version"] = serde_json::json!(1);

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_zero_line_and_column() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut zero_line = serde_json::to_value(&facts).unwrap();
    zero_line["usage_sites"][0]["location"]["line"] = serde_json::json!(0);
    assert!(wax_contract::scan_facts_from_json(&zero_line.to_string()).is_err());

    let mut zero_column = serde_json::to_value(&facts).unwrap();
    zero_column["usage_sites"][0]["location"]["column"] = serde_json::json!(0);
    assert!(wax_contract::scan_facts_from_json(&zero_column.to_string()).is_err());
}

#[test]
fn rejects_empty_required_strings() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["language"]["parser_name"] = serde_json::json!("");

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_explicit_null_for_optional_schema_fields() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["usage_sites"][1]["location"]["column"] = serde_json::Value::Null;

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_inconsistent_counts_and_metrics() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut stale_counts = serde_json::to_value(&facts).unwrap();
    stale_counts["counts"]["raw_invocations"]["resolved"] = serde_json::json!(0);
    assert!(wax_contract::scan_facts_from_json(&stale_counts.to_string()).is_err());

    let mut stale_ratio = serde_json::to_value(&facts).unwrap();
    stale_ratio["metrics"]["invocation_adoption_ratio"] = serde_json::json!(0.25);
    assert!(wax_contract::scan_facts_from_json(&stale_ratio.to_string()).is_err());
}

#[test]
fn rejects_parse_extract_ms_above_contract_max() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    facts.metrics.parse_extract_ms = 4_294_967_296;

    let json = serde_json::to_string(&facts).unwrap();

    assert!(wax_contract::scan_facts_from_json(&json).is_err());
}

#[test]
fn requires_registry_symbol_for_resolved_and_candidate_usage() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut resolved_missing = serde_json::to_value(&facts).unwrap();
    resolved_missing["usage_sites"][0]
        .as_object_mut()
        .unwrap()
        .remove("registry_symbol");
    assert_schema_rejects(&resolved_missing);
    assert!(wax_contract::scan_facts_from_json(&resolved_missing.to_string()).is_err());

    let mut candidate_missing = serde_json::to_value(&facts).unwrap();
    candidate_missing["usage_sites"][1]
        .as_object_mut()
        .unwrap()
        .remove("registry_symbol");
    assert_schema_rejects(&candidate_missing);
    assert!(wax_contract::scan_facts_from_json(&candidate_missing.to_string()).is_err());
}

#[test]
fn requires_local_definition_id_for_local_usage() {
    let mut facts = minimal_facts();
    facts.usage_sites = vec![UsageSite {
        id: "local:1".into(),
        location: SourceLocation {
            file: "a.kt".into(),
            line: 1,
            column: None,
        },
        symbol: "EpisodeCard".into(),
        qualified_symbol: None,
        match_status: MatchStatus::Local,
        registry_symbol: None,
        local_definition_id: None,
        parent: None,
    }];
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_registry_symbol_for_unresolved_usage() {
    let mut facts = minimal_facts();
    facts.usage_sites[0].match_status = MatchStatus::Unresolved;
    facts.usage_sites[0].registry_symbol = None;
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_ok());

    facts.usage_sites[0].registry_symbol = Some("com.ds.Button".into());
    let value = serde_json::to_value(&facts).unwrap();
    assert_schema_rejects(&value);
    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn rejects_registry_symbol_for_local_usage() {
    let mut facts = minimal_facts();
    facts.usage_sites = vec![UsageSite {
        id: "local:1".into(),
        location: SourceLocation {
            file: "a.kt".into(),
            line: 1,
            column: None,
        },
        symbol: "EpisodeCard".into(),
        qualified_symbol: None,
        match_status: MatchStatus::Local,
        registry_symbol: Some("com.ds.EpisodeCard".into()),
        local_definition_id: Some("local.compose:EpisodeCard".into()),
        parent: None,
    }];
    facts.recompute_counts().unwrap();
    assert!(wax_contract::scan_facts_from_json(&serde_json::to_string(&facts).unwrap()).is_err());
}

#[test]
fn token_facts_roundtrip_and_validate_against_schema() {
    let mut facts = minimal_facts();
    facts.design_system_tokens = vec![
        wax_contract::DesignSystemToken {
            id: "color.primary".into(),
            key: "Theme.colors.primary".into(),
            category: wax_contract::TokenCategory::Color,
            aliases: vec!["AppColors.Primary".into()],
            value: None,
        },
        wax_contract::DesignSystemToken {
            id: "space.medium".into(),
            key: "Spacing.Medium".into(),
            category: wax_contract::TokenCategory::Spacing,
            aliases: vec![],
            value: None,
        },
    ];
    facts.token_sites = vec![wax_contract::TokenSite {
        id: "token.compose:src/Screen.kt:8:13:color.primary".into(),
        location: SourceLocation {
            file: "src/Screen.kt".into(),
            line: 8,
            column: Some(13),
        },
        token_id: "color.primary".into(),
        key: "AppColors.Primary".into(),
        category: wax_contract::TokenCategory::Color,
        parent: Some(ParentScope {
            parent_id: "compose:composable:com.example.Screen".into(),
            symbol: "Screen".into(),
            qualified_symbol: Some("com.example.Screen".into()),
            scope_kind: "composable".into(),
            identity_basis: "package_qualified_symbol".into(),
            identity_stability: IdentityStability::Semantic,
            location: Some(SourceLocation {
                file: "src/Screen.kt".into(),
                line: 4,
                column: Some(1),
            }),
        }),
    }];
    facts.hardcoded_style_sites = vec![wax_contract::HardcodedStyleSite {
        id: "hardcoded.compose:src/Screen.kt:9:22:spacing".into(),
        location: SourceLocation {
            file: "src/Screen.kt".into(),
            line: 9,
            column: Some(22),
        },
        value: "8.dp".into(),
        category: wax_contract::TokenCategory::Spacing,
        context: wax_contract::StyleContext::Unknown,
        parent: None,
    }];
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(back.design_system_tokens.len(), 2);
    assert_eq!(back.token_sites.len(), 1);
    assert_eq!(back.hardcoded_style_sites.len(), 1);
    assert_eq!(back.counts.tokens.configured_token_count, 2);
    assert_eq!(back.counts.tokens.used_token_count, 1);
    assert_eq!(back.counts.tokens.token_reference_site_count, 1);
    assert_eq!(back.counts.tokens.hardcoded_style_candidate_count, 1);
    assert_eq!(back.counts.tokens.token_references_by_category.color, 1);
    assert_eq!(back.counts.tokens.hardcoded_by_category.spacing, 1);

    let value = serde_json::to_value(&back).unwrap();
    assert!(scan_facts_schema().is_valid(&value));
}

#[test]
fn token_site_must_reference_known_token_id() {
    let mut facts = minimal_facts();
    facts.token_sites = vec![wax_contract::TokenSite {
        id: "token.react:src/App.tsx:1:1:missing".into(),
        location: SourceLocation {
            file: "src/App.tsx".into(),
            line: 1,
            column: Some(1),
        },
        token_id: "missing".into(),
        key: "theme.colors.missing".into(),
        category: wax_contract::TokenCategory::Color,
        parent: None,
    }];

    let err = facts
        .recompute_counts()
        .expect_err("unknown token id must fail");
    assert!(err.to_string().contains("token_id"));
}

#[test]
fn token_site_key_must_match_key_or_alias() {
    let mut facts = minimal_facts();
    facts.design_system_tokens = vec![wax_contract::DesignSystemToken {
        id: "color.primary".into(),
        key: "theme.colors.primary".into(),
        category: wax_contract::TokenCategory::Color,
        aliases: vec!["colors.primary".into()],
        value: None,
    }];
    facts.token_sites = vec![wax_contract::TokenSite {
        id: "token.react:src/App.tsx:1:1:color.primary".into(),
        location: SourceLocation {
            file: "src/App.tsx".into(),
            line: 1,
            column: Some(1),
        },
        token_id: "color.primary".into(),
        key: "wrong.primary".into(),
        category: wax_contract::TokenCategory::Color,
        parent: None,
    }];

    let err = facts
        .recompute_counts()
        .expect_err("wrong matched key must fail");
    assert!(err.to_string().contains("key"));
}

#[test]
fn hardcoded_style_site_requires_non_empty_value() {
    let mut facts = minimal_facts();
    facts.hardcoded_style_sites = vec![wax_contract::HardcodedStyleSite {
        id: "hardcoded.react:src/App.tsx:1:1:color".into(),
        location: SourceLocation {
            file: "src/App.tsx".into(),
            line: 1,
            column: Some(1),
        },
        value: "".into(),
        category: wax_contract::TokenCategory::Color,
        context: wax_contract::StyleContext::Unknown,
        parent: None,
    }];

    let err = facts
        .recompute_counts()
        .expect_err("empty hardcoded value must fail");
    assert!(err.to_string().contains("value"));
}

#[test]
fn schema_rejects_invalid_symbol_summary_linkage() {
    let mut facts = minimal_facts();
    facts.symbol_usage_summary = vec![SymbolUsageSummary {
        symbol_id: "compose:local:EpisodeCard".into(),
        symbol: "EpisodeCard".into(),
        qualified_symbol: None,
        symbol_kind: SymbolKind::Local,
        match_status: MatchStatus::Resolved,
        registry_symbol: Some("com.ds.EpisodeCard".into()),
        local_definition_id: None,
        identity_basis: "local_definition_id".into(),
        identity_stability: IdentityStability::Semantic,
        raw_invocation_count: 1,
        parent_scope_count: 0,
        file_count: 1,
        parent_scopes: vec![],
        parent_scope_limit: None,
        parent_scopes_truncated: false,
    }];
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert_schema_rejects(&value);
    assert!(wax_contract::scan_facts_from_json(&value.to_string()).is_err());
}

#[test]
fn merged_scan_rejects_stale_repo_summary() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut languages = BTreeMap::new();
    languages.insert(LanguageId::try_from("compose").unwrap(), facts.clone());

    let mut merged = MergedScan {
        schema_version: SCHEMA_VERSION,
        recorded_at: datetime!(2026-05-16 12:00 UTC),
        repo_summary: RepoSummary {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            counts: facts.counts.clone(),
            metrics: Metrics {
                invocation_adoption_ratio: facts.metrics.invocation_adoption_ratio,
                registry_resolution_ratio: facts.metrics.registry_resolution_ratio,
                parse_extract_ms: facts.metrics.parse_extract_ms,
                files_scanned: facts.metrics.files_scanned,
            },
        },
        symbol_usage_summary: vec![],
        token_usage_summary: vec![],
        token_inference: TokenInferenceReport::empty(2.0),
        languages,
    };

    merged.validate().unwrap();

    merged.repo_summary.counts.raw_invocations.resolved = 0;
    assert!(merged.validate().is_err());

    merged.repo_summary.counts = facts.counts.clone();
    merged.repo_summary.languages = vec![LanguageId::try_from("react").unwrap()];
    assert!(merged.validate().is_err());
}

#[test]
fn accepts_zero_usage_sites_with_null_ratios() {
    let mut facts = minimal_facts();
    facts.usage_sites.clear();
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();

    assert_eq!(back.metrics.invocation_adoption_ratio, None);
    assert_eq!(back.metrics.registry_resolution_ratio, None);
    assert_eq!(back.counts.raw_invocations.total, 0);
}

#[test]
fn rejects_v2_schema_version() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["schema_version"] = serde_json::json!(2);

    let err = wax_contract::scan_facts_from_json(&value.to_string()).unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::UnsupportedSchemaVersion {
            found: 2,
            supported: 3
        }
    ));
}

#[test]
fn rejects_missing_invocation_adoption_ratio() {
    let mut facts = minimal_facts();
    facts.usage_sites.clear();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["metrics"]
        .as_object_mut()
        .unwrap()
        .remove("invocation_adoption_ratio");

    assert_schema_rejects(&value);
    let err = wax_contract::scan_facts_from_json(&value.to_string()).unwrap_err();

    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "metrics.invocation_adoption_ratio"
    ));
}

#[test]
fn all_candidate_usage_has_zero_adoption() {
    let mut facts = minimal_facts();
    facts.usage_sites[0].match_status = MatchStatus::Candidate;
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();

    assert_eq!(back.metrics.invocation_adoption_ratio, None);
    assert_eq!(back.counts.raw_invocations.resolved, 0);
    assert_eq!(back.counts.raw_invocations.candidate, 2);
    assert_eq!(back.counts.adoption.eligible_invocation_count, 0);
}

#[test]
fn scanned_at_serializes_as_rfc3339() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let value = serde_json::to_value(&facts).unwrap();

    assert_eq!(value["scanned_at"], "2026-05-16T12:00:00Z");
}

#[test]
fn accepts_non_utc_rfc3339_timestamp() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();
    let mut value = serde_json::to_value(&facts).unwrap();
    value["scanned_at"] = serde_json::json!("2026-05-16T14:00:00+02:00");

    let back = wax_contract::scan_facts_from_json(&value.to_string()).unwrap();

    assert_eq!(back.scanned_at, facts.scanned_at);
}

#[test]
fn rejects_symbol_summary_kind_status_mismatch() {
    let mut facts = minimal_facts();
    facts.symbol_usage_summary = vec![SymbolUsageSummary {
        symbol_id: "compose:local:EpisodeCard".into(),
        symbol: "EpisodeCard".into(),
        qualified_symbol: None,
        symbol_kind: SymbolKind::Local,
        match_status: MatchStatus::Resolved,
        registry_symbol: None,
        local_definition_id: Some("local.compose:EpisodeCard".into()),
        identity_basis: "package_qualified_symbol".into(),
        identity_stability: IdentityStability::Semantic,
        raw_invocation_count: 1,
        parent_scope_count: 0,
        file_count: 1,
        parent_scopes: vec![],
        parent_scope_limit: None,
        parent_scopes_truncated: false,
    }];
    facts.recompute_counts().unwrap();

    assert!(wax_contract::scan_facts_from_json(&serde_json::to_string(&facts).unwrap()).is_err());
}

#[test]
fn merged_scan_rejects_malformed_token_usage_summary() {
    let mut facts = minimal_facts();
    facts.recompute_counts().unwrap();

    let mut languages = BTreeMap::new();
    languages.insert(LanguageId::try_from("compose").unwrap(), facts.clone());

    let merged = MergedScan {
        schema_version: SCHEMA_VERSION,
        recorded_at: datetime!(2026-05-16 12:00 UTC),
        repo_summary: RepoSummary {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            counts: facts.counts.clone(),
            metrics: Metrics {
                invocation_adoption_ratio: facts.metrics.invocation_adoption_ratio,
                registry_resolution_ratio: facts.metrics.registry_resolution_ratio,
                parse_extract_ms: facts.metrics.parse_extract_ms,
                files_scanned: facts.metrics.files_scanned,
            },
        },
        symbol_usage_summary: vec![],
        token_usage_summary: vec![wax_contract::TokenUsageSummary {
            language: "compose".into(),
            token_id: "".into(),
            key: "Theme.colors.primary".into(),
            category: wax_contract::TokenCategory::Color,
            reference_count: 1,
            file_count: 1,
            parent_scope_count: 0,
        }],
        token_inference: TokenInferenceReport::empty(2.0),
        languages,
    };

    assert!(merged.validate().is_err());
}

#[test]
fn schema_v3_token_inference_roundtrips() {
    use wax_contract::{
        HardcodedStyleInference, HardcodedStyleSite, StyleContext, StyleContextCounts,
        TokenCategory, TokenCategoryCounts, TokenConfidenceCounts, TokenInferenceClassification,
        TokenInferenceConfidence, TokenInferenceCounts, TokenInferenceEvidence,
        TokenInferenceReport, TokenMatchKind, TokenReplacementSuggestion,
    };

    let token = wax_contract::DesignSystemToken {
        id: "spacing.s".into(),
        key: "Spacing.s".into(),
        category: TokenCategory::Spacing,
        aliases: vec![],
        value: Some("4.dp".into()),
    };
    let site = HardcodedStyleSite {
        id: "hardcoded.compose:src/Card.kt:8:20:spacing".into(),
        location: SourceLocation {
            file: "src/Card.kt".into(),
            line: 8,
            column: Some(20),
        },
        value: "4.dp".into(),
        category: TokenCategory::Spacing,
        context: StyleContext::Padding,
        parent: None,
    };
    let inference = HardcodedStyleInference {
        language: LanguageId::try_from("compose").unwrap(),
        site_id: site.id.clone(),
        classification: TokenInferenceClassification::Exact,
        confidence: Some(TokenInferenceConfidence::VeryHigh),
        suggestions: vec![TokenReplacementSuggestion {
            token_id: token.id.clone(),
            token_key: token.key.clone(),
            canonical_value: "4.dp".into(),
            match_kind: TokenMatchKind::Exact,
            distance: Some(0.0),
            normalized_unit: Some("dp".into()),
        }],
        evidence: vec![
            TokenInferenceEvidence::ExactValue,
            TokenInferenceEvidence::ClearUsageContext,
        ],
    };

    let mut facts = minimal_facts();
    facts.design_system_tokens = vec![token];
    facts.hardcoded_style_sites = vec![site];
    facts.recompute_counts().unwrap();

    let value = serde_json::to_value(&facts).unwrap();
    assert!(scan_facts_schema().is_valid(&value));
    let back_facts =
        wax_contract::scan_facts_from_json(&serde_json::to_string(&facts).unwrap()).unwrap();
    assert_eq!(
        back_facts.design_system_tokens[0].value.as_deref(),
        Some("4.dp")
    );
    assert_eq!(
        back_facts.hardcoded_style_sites[0].context,
        StyleContext::Padding
    );

    let mut languages = BTreeMap::new();
    languages.insert(LanguageId::try_from("compose").unwrap(), facts.clone());
    let merged = MergedScan {
        schema_version: SCHEMA_VERSION,
        recorded_at: datetime!(2026-05-16 12:00 UTC),
        repo_summary: RepoSummary {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            counts: facts.counts.clone(),
            metrics: Metrics {
                invocation_adoption_ratio: facts.metrics.invocation_adoption_ratio,
                registry_resolution_ratio: facts.metrics.registry_resolution_ratio,
                parse_extract_ms: facts.metrics.parse_extract_ms,
                files_scanned: facts.metrics.files_scanned,
            },
        },
        symbol_usage_summary: vec![],
        token_usage_summary: vec![],
        token_inference: TokenInferenceReport {
            numeric_tolerance: 2.0,
            counts: TokenInferenceCounts {
                hardcoded_observation_count: 1,
                assessed_observation_count: 1,
                exact_replacement_candidate_count: 1,
                near_replacement_candidate_count: 0,
                unmatched_observation_count: 0,
                unassessed_observation_count: 0,
                candidates_by_confidence: TokenConfidenceCounts {
                    very_high: 1,
                    high: 0,
                    medium: 0,
                    low: 0,
                },
                candidates_by_category: TokenCategoryCounts {
                    color: 0,
                    spacing: 1,
                    typography: 0,
                    radius: 0,
                    elevation: 0,
                    unknown: 0,
                },
                candidates_by_context: StyleContextCounts {
                    padding: 1,
                    margin: 0,
                    gap: 0,
                    width: 0,
                    height: 0,
                    size: 0,
                    radius: 0,
                    color: 0,
                    typography: 0,
                    elevation: 0,
                    unknown: 0,
                },
            },
            sites: vec![inference],
        },
        languages,
    };

    merged.validate().unwrap();
    let json = serde_json::to_string(&merged).unwrap();
    let back: MergedScan = serde_json::from_str(&json).unwrap();
    back.validate().unwrap();
    assert_eq!(back, merged);
}

fn inference_merged_with_site(
    classification: wax_contract::TokenInferenceClassification,
    confidence: Option<wax_contract::TokenInferenceConfidence>,
    suggestions: Vec<wax_contract::TokenReplacementSuggestion>,
) -> MergedScan {
    use wax_contract::{
        HardcodedStyleInference, HardcodedStyleSite, StyleContext, StyleContextCounts,
        TokenCategory, TokenCategoryCounts, TokenConfidenceCounts, TokenInferenceCounts,
        TokenInferenceEvidence, TokenInferenceReport,
    };

    let token = wax_contract::DesignSystemToken {
        id: "spacing.s".into(),
        key: "Spacing.s".into(),
        category: TokenCategory::Spacing,
        aliases: vec![],
        value: Some("4.dp".into()),
    };
    let site = HardcodedStyleSite {
        id: "hardcoded.compose:src/Card.kt:8:20:spacing".into(),
        location: SourceLocation {
            file: "src/Card.kt".into(),
            line: 8,
            column: Some(20),
        },
        value: "4.dp".into(),
        category: TokenCategory::Spacing,
        context: StyleContext::Padding,
        parent: None,
    };
    let mut facts = minimal_facts();
    facts.design_system_tokens = vec![token];
    let site_id = site.id.clone();
    facts.hardcoded_style_sites = vec![site];
    facts.recompute_counts().unwrap();

    let is_candidate = matches!(
        classification,
        wax_contract::TokenInferenceClassification::Exact
            | wax_contract::TokenInferenceClassification::Near
    );
    let exact = u32::from(classification == wax_contract::TokenInferenceClassification::Exact);
    let near = u32::from(classification == wax_contract::TokenInferenceClassification::Near);
    let unmatched =
        u32::from(classification == wax_contract::TokenInferenceClassification::Unmatched);
    let unassessed =
        u32::from(classification == wax_contract::TokenInferenceClassification::Unassessed);
    let assessed = exact + near + unmatched;

    let mut by_confidence = TokenConfidenceCounts::default();
    if let Some(level) = confidence {
        match level {
            wax_contract::TokenInferenceConfidence::VeryHigh => by_confidence.very_high = 1,
            wax_contract::TokenInferenceConfidence::High => by_confidence.high = 1,
            wax_contract::TokenInferenceConfidence::Medium => by_confidence.medium = 1,
            wax_contract::TokenInferenceConfidence::Low => by_confidence.low = 1,
        }
    }
    let by_category = if is_candidate {
        TokenCategoryCounts {
            spacing: 1,
            ..TokenCategoryCounts::default()
        }
    } else {
        TokenCategoryCounts::default()
    };
    let by_context = if is_candidate {
        StyleContextCounts {
            padding: 1,
            ..StyleContextCounts::default()
        }
    } else {
        StyleContextCounts::default()
    };

    let mut languages = BTreeMap::new();
    languages.insert(LanguageId::try_from("compose").unwrap(), facts.clone());
    MergedScan {
        schema_version: SCHEMA_VERSION,
        recorded_at: datetime!(2026-05-16 12:00 UTC),
        repo_summary: RepoSummary {
            languages: vec![LanguageId::try_from("compose").unwrap()],
            counts: facts.counts.clone(),
            metrics: Metrics {
                invocation_adoption_ratio: facts.metrics.invocation_adoption_ratio,
                registry_resolution_ratio: facts.metrics.registry_resolution_ratio,
                parse_extract_ms: facts.metrics.parse_extract_ms,
                files_scanned: facts.metrics.files_scanned,
            },
        },
        symbol_usage_summary: vec![],
        token_usage_summary: vec![],
        token_inference: TokenInferenceReport {
            numeric_tolerance: 2.0,
            counts: TokenInferenceCounts {
                hardcoded_observation_count: 1,
                assessed_observation_count: assessed,
                exact_replacement_candidate_count: exact,
                near_replacement_candidate_count: near,
                unmatched_observation_count: unmatched,
                unassessed_observation_count: unassessed,
                candidates_by_confidence: by_confidence,
                candidates_by_category: by_category,
                candidates_by_context: by_context,
            },
            sites: vec![HardcodedStyleInference {
                language: LanguageId::try_from("compose").unwrap(),
                site_id,
                classification,
                confidence,
                suggestions,
                evidence: if classification
                    == wax_contract::TokenInferenceClassification::Unassessed
                {
                    vec![TokenInferenceEvidence::MissingCanonicalValues]
                } else {
                    vec![]
                },
            }],
        },
        languages,
    }
}

#[test]
fn token_inference_rejects_empty_report_when_raw_sites_exist() {
    let mut merged = inference_merged_with_site(
        wax_contract::TokenInferenceClassification::Unassessed,
        None,
        vec![],
    );
    merged.token_inference = TokenInferenceReport::empty(2.0);
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites" || field == "token_inference.counts.hardcoded_observation_count"
    ));
}

#[test]
fn token_inference_rejects_duplicate_raw_site_ids() {
    let mut merged = inference_merged_with_site(
        wax_contract::TokenInferenceClassification::Unassessed,
        None,
        vec![],
    );
    let facts = merged
        .languages
        .get_mut(&LanguageId::try_from("compose").unwrap())
        .unwrap();
    let duplicate = facts.hardcoded_style_sites[0].clone();
    facts.hardcoded_style_sites.push(duplicate);
    facts.recompute_counts().unwrap();
    merged.repo_summary.counts = facts.counts.clone();
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field.contains("hardcoded_style_sites")
    ));
}

#[test]
fn token_inference_rejects_duplicate_inference_rows() {
    let mut merged = inference_merged_with_site(
        wax_contract::TokenInferenceClassification::Unassessed,
        None,
        vec![],
    );
    let row = merged.token_inference.sites[0].clone();
    merged.token_inference.sites.push(row);
    merged.token_inference.counts.hardcoded_observation_count = 2;
    merged.token_inference.counts.unassessed_observation_count = 2;
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites[1].site_id"
    ));
}

#[test]
fn token_inference_rejects_extra_inference_row() {
    let mut merged = inference_merged_with_site(
        wax_contract::TokenInferenceClassification::Unassessed,
        None,
        vec![],
    );
    merged.token_inference.sites[0].site_id = "missing".into();
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites[0].site_id"
    ));
}

#[test]
fn token_inference_rejects_exact_without_confidence() {
    use wax_contract::{TokenInferenceClassification, TokenMatchKind, TokenReplacementSuggestion};
    let mut merged = inference_merged_with_site(
        TokenInferenceClassification::Exact,
        Some(wax_contract::TokenInferenceConfidence::VeryHigh),
        vec![TokenReplacementSuggestion {
            token_id: "spacing.s".into(),
            token_key: "Spacing.s".into(),
            canonical_value: "4.dp".into(),
            match_kind: TokenMatchKind::Exact,
            distance: Some(0.0),
            normalized_unit: Some("dp".into()),
        }],
    );
    merged.token_inference.sites[0].confidence = None;
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites[0].confidence"
    ));
}

#[test]
fn token_inference_rejects_unassessed_with_suggestions() {
    use wax_contract::{TokenInferenceClassification, TokenMatchKind, TokenReplacementSuggestion};
    let mut merged =
        inference_merged_with_site(TokenInferenceClassification::Unassessed, None, vec![]);
    merged.token_inference.sites[0].suggestions = vec![TokenReplacementSuggestion {
        token_id: "spacing.s".into(),
        token_key: "Spacing.s".into(),
        canonical_value: "4.dp".into(),
        match_kind: TokenMatchKind::Exact,
        distance: None,
        normalized_unit: None,
    }];
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites[0].suggestions"
    ));
}

#[test]
fn token_inference_rejects_unknown_suggestion_token() {
    use wax_contract::{
        TokenInferenceClassification, TokenInferenceConfidence, TokenMatchKind,
        TokenReplacementSuggestion,
    };
    let merged = inference_merged_with_site(
        TokenInferenceClassification::Exact,
        Some(TokenInferenceConfidence::VeryHigh),
        vec![TokenReplacementSuggestion {
            token_id: "missing".into(),
            token_key: "Missing".into(),
            canonical_value: "4.dp".into(),
            match_kind: TokenMatchKind::Exact,
            distance: Some(0.0),
            normalized_unit: Some("dp".into()),
        }],
    );
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites[0].suggestions[0].token_id"
    ));
}

#[test]
fn token_inference_rejects_near_distance_zero() {
    use wax_contract::{
        TokenInferenceClassification, TokenInferenceConfidence, TokenMatchKind,
        TokenReplacementSuggestion,
    };
    let merged = inference_merged_with_site(
        TokenInferenceClassification::Near,
        Some(TokenInferenceConfidence::Medium),
        vec![TokenReplacementSuggestion {
            token_id: "spacing.s".into(),
            token_key: "Spacing.s".into(),
            canonical_value: "4.dp".into(),
            match_kind: TokenMatchKind::Near,
            distance: Some(0.0),
            normalized_unit: Some("dp".into()),
        }],
    );
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.sites[0].suggestions[0].distance"
    ));
}

#[test]
fn token_inference_rejects_count_mismatch() {
    let mut merged = inference_merged_with_site(
        wax_contract::TokenInferenceClassification::Unassessed,
        None,
        vec![],
    );
    merged.token_inference.counts.hardcoded_observation_count = 99;
    let err = merged.validate().unwrap_err();
    assert!(matches!(
        err,
        wax_contract::ScanFactsError::ContractViolation { field, .. }
            if field == "token_inference.counts.hardcoded_observation_count"
    ));
}
