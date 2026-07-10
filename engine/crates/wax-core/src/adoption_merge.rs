//! Adoption Metrics v2 merge and summary generation.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, DesignSystemToken, IdentityStability, LanguageId, MatchStatus, MergedScan,
    Metrics, RepoSummary, SCHEMA_VERSION, ScanFacts, ScanFactsError, SymbolKind,
    SymbolParentScopeSummary, SymbolUsageSummary, TokenCategoryCounts, TokenUsageSummary,
};

/// Default parent scope row limit when config omits an explicit value.
const DEFAULT_PARENT_SCOPE_LIMIT: Option<u32> = None;

/// Recomputes derived counters, metrics, and symbol summaries for one language scan.
pub fn recompute_derived_scan_facts(
    facts: &mut ScanFacts,
    language_id: &LanguageId,
) -> Result<(), ScanFactsError> {
    recompute_derived_scan_facts_with_parent_scope_limit(
        facts,
        language_id,
        DEFAULT_PARENT_SCOPE_LIMIT,
    )
}

/// Recomputes derived scan facts using an explicit parent-scope row limit.
pub fn recompute_derived_scan_facts_with_parent_scope_limit(
    facts: &mut ScanFacts,
    language_id: &LanguageId,
    parent_scope_limit: Option<u32>,
) -> Result<(), ScanFactsError> {
    facts.recompute_counts()?;
    facts.symbol_usage_summary =
        build_symbol_usage_summaries(facts, language_id, parent_scope_limit)?;
    facts.token_usage_summary = build_token_usage_summaries(facts);
    Ok(())
}

/// Builds a merged scan with repo-level counters and symbol summaries.
pub fn merge_language_scans(
    languages: BTreeMap<LanguageId, ScanFacts>,
) -> Result<MergedScan, ScanFactsError> {
    merge_language_scans_with_parent_scope_limit(languages, DEFAULT_PARENT_SCOPE_LIMIT)
}

/// Builds a merged scan using an explicit parent-scope row limit.
pub fn merge_language_scans_with_parent_scope_limit(
    languages: BTreeMap<LanguageId, ScanFacts>,
    parent_scope_limit: Option<u32>,
) -> Result<MergedScan, ScanFactsError> {
    let mut merged_languages = BTreeMap::new();
    for (language_id, mut facts) in languages {
        recompute_derived_scan_facts_with_parent_scope_limit(
            &mut facts,
            &language_id,
            parent_scope_limit,
        )?;
        merged_languages.insert(language_id, facts);
    }

    let language_ids = merged_languages.keys().cloned().collect::<Vec<_>>();
    let repo_counts = sum_count_summaries(merged_languages.values().map(|facts| &facts.counts));
    let repo_metrics = metrics_from_counts(&repo_counts, merged_parse_metrics(&merged_languages));
    let symbol_usage_summary = merge_symbol_usage_summaries(&merged_languages);
    let token_usage_summary = merge_token_usage_summaries(&merged_languages);

    Ok(MergedScan {
        schema_version: SCHEMA_VERSION,
        recorded_at: OffsetDateTime::now_utc(),
        repo_summary: RepoSummary {
            languages: language_ids,
            counts: repo_counts,
            metrics: repo_metrics,
        },
        symbol_usage_summary,
        token_usage_summary,
        languages: merged_languages,
    })
}

fn merged_parse_metrics(languages: &BTreeMap<LanguageId, ScanFacts>) -> (u64, u32) {
    languages
        .values()
        .fold((0_u64, 0_u32), |(parse_ms, files), facts| {
            (
                parse_ms.saturating_add(facts.metrics.parse_extract_ms),
                files.saturating_add(facts.metrics.files_scanned),
            )
        })
}

fn metrics_from_counts(
    counts: &CountSummary,
    (parse_extract_ms, files_scanned): (u64, u32),
) -> Metrics {
    let invocation_adoption_ratio = if counts.adoption.eligible_invocation_count == 0 {
        None
    } else {
        Some(
            f64::from(counts.adoption.adopted_invocation_count)
                / f64::from(counts.adoption.eligible_invocation_count),
        )
    };
    let registry_resolution_ratio = if counts.raw_invocations.total == 0 {
        None
    } else {
        Some(f64::from(counts.raw_invocations.resolved) / f64::from(counts.raw_invocations.total))
    };
    let token_denominator = counts
        .tokens
        .token_reference_site_count
        .saturating_add(counts.tokens.hardcoded_style_candidate_count);
    let token_reference_ratio = if token_denominator == 0 {
        None
    } else {
        Some(f64::from(counts.tokens.token_reference_site_count) / f64::from(token_denominator))
    };

    Metrics {
        invocation_adoption_ratio,
        registry_resolution_ratio,
        token_reference_ratio,
        parse_extract_ms,
        files_scanned,
    }
}

pub(crate) fn sum_count_summaries<'a>(
    summaries: impl Iterator<Item = &'a CountSummary>,
) -> CountSummary {
    summaries.fold(CountSummary::default(), |mut total, counts| {
        total.registry.component_count = total
            .registry
            .component_count
            .saturating_add(counts.registry.component_count);
        total.registry.used_component_count = total
            .registry
            .used_component_count
            .saturating_add(counts.registry.used_component_count);
        total.registry.resolved_raw_invocation_count = total
            .registry
            .resolved_raw_invocation_count
            .saturating_add(counts.registry.resolved_raw_invocation_count);
        total.registry.candidate_raw_invocation_count = total
            .registry
            .candidate_raw_invocation_count
            .saturating_add(counts.registry.candidate_raw_invocation_count);

        total.definitions.local_definition_count = total
            .definitions
            .local_definition_count
            .saturating_add(counts.definitions.local_definition_count);
        total.definitions.invoked_local_definition_count = total
            .definitions
            .invoked_local_definition_count
            .saturating_add(counts.definitions.invoked_local_definition_count);
        total.definitions.unused_local_definition_count = total
            .definitions
            .unused_local_definition_count
            .saturating_add(counts.definitions.unused_local_definition_count);

        total.raw_invocations.total = total
            .raw_invocations
            .total
            .saturating_add(counts.raw_invocations.total);
        total.raw_invocations.resolved = total
            .raw_invocations
            .resolved
            .saturating_add(counts.raw_invocations.resolved);
        total.raw_invocations.local = total
            .raw_invocations
            .local
            .saturating_add(counts.raw_invocations.local);
        total.raw_invocations.candidate = total
            .raw_invocations
            .candidate
            .saturating_add(counts.raw_invocations.candidate);
        total.raw_invocations.unresolved = total
            .raw_invocations
            .unresolved
            .saturating_add(counts.raw_invocations.unresolved);

        total.adoption.eligible_invocation_count = total
            .adoption
            .eligible_invocation_count
            .saturating_add(counts.adoption.eligible_invocation_count);
        total.adoption.adopted_invocation_count = total
            .adoption
            .adopted_invocation_count
            .saturating_add(counts.adoption.adopted_invocation_count);
        total.adoption.non_adopted_invocation_count = total
            .adoption
            .non_adopted_invocation_count
            .saturating_add(counts.adoption.non_adopted_invocation_count);

        total.parent_scopes.total = total
            .parent_scopes
            .total
            .saturating_add(counts.parent_scopes.total);
        total.parent_scopes.with_resolved_invocations = total
            .parent_scopes
            .with_resolved_invocations
            .saturating_add(counts.parent_scopes.with_resolved_invocations);
        total.parent_scopes.with_local_invocations = total
            .parent_scopes
            .with_local_invocations
            .saturating_add(counts.parent_scopes.with_local_invocations);
        total.parent_scopes.with_unresolved_invocations = total
            .parent_scopes
            .with_unresolved_invocations
            .saturating_add(counts.parent_scopes.with_unresolved_invocations);

        total.tokens.configured_token_count = total
            .tokens
            .configured_token_count
            .saturating_add(counts.tokens.configured_token_count);
        total.tokens.used_token_count = total
            .tokens
            .used_token_count
            .saturating_add(counts.tokens.used_token_count);
        total.tokens.token_reference_site_count = total
            .tokens
            .token_reference_site_count
            .saturating_add(counts.tokens.token_reference_site_count);
        total.tokens.hardcoded_style_candidate_count = total
            .tokens
            .hardcoded_style_candidate_count
            .saturating_add(counts.tokens.hardcoded_style_candidate_count);
        total.tokens.parent_scopes_with_token_references = total
            .tokens
            .parent_scopes_with_token_references
            .saturating_add(counts.tokens.parent_scopes_with_token_references);
        total.tokens.parent_scopes_with_hardcoded_candidates = total
            .tokens
            .parent_scopes_with_hardcoded_candidates
            .saturating_add(counts.tokens.parent_scopes_with_hardcoded_candidates);
        add_token_category_counts(
            &mut total.tokens.token_references_by_category,
            &counts.tokens.token_references_by_category,
        );
        add_token_category_counts(
            &mut total.tokens.hardcoded_by_category,
            &counts.tokens.hardcoded_by_category,
        );

        total
    })
}

fn add_token_category_counts(total: &mut TokenCategoryCounts, counts: &TokenCategoryCounts) {
    total.color = total.color.saturating_add(counts.color);
    total.spacing = total.spacing.saturating_add(counts.spacing);
    total.typography = total.typography.saturating_add(counts.typography);
    total.radius = total.radius.saturating_add(counts.radius);
    total.elevation = total.elevation.saturating_add(counts.elevation);
    total.unknown = total.unknown.saturating_add(counts.unknown);
}

#[derive(Clone, Eq, PartialEq, Hash)]
struct SymbolGroupKey {
    match_status: MatchStatus,
    registry_symbol: Option<String>,
    local_definition_id: Option<String>,
    qualified_symbol: Option<String>,
    symbol: String,
}

fn symbol_group_key(site: &wax_contract::UsageSite) -> SymbolGroupKey {
    SymbolGroupKey {
        match_status: site.match_status,
        registry_symbol: site.registry_symbol.clone(),
        local_definition_id: site.local_definition_id.clone(),
        qualified_symbol: site.qualified_symbol.clone(),
        symbol: site.symbol.clone(),
    }
}

fn symbol_kind_for_status(status: MatchStatus) -> SymbolKind {
    match status {
        MatchStatus::Resolved => SymbolKind::Registry,
        MatchStatus::Local => SymbolKind::Local,
        MatchStatus::Candidate => SymbolKind::Candidate,
        MatchStatus::Unresolved => SymbolKind::Unresolved,
    }
}

fn build_symbol_id(language_id: &LanguageId, key: &SymbolGroupKey) -> String {
    if let Some(registry_symbol) = &key.registry_symbol {
        let kind = match key.match_status {
            MatchStatus::Candidate => "candidate",
            _ => "registry",
        };
        return format!("{}:{kind}:{}", language_id.as_str(), registry_symbol);
    }
    if let Some(local_definition_id) = &key.local_definition_id {
        let local_identity = normalized_local_identity(language_id, local_definition_id)
            .or(key.qualified_symbol.as_deref())
            .unwrap_or(local_definition_id);
        return format!("{}:local:{}", language_id.as_str(), local_identity);
    }
    if let Some(qualified_symbol) = &key.qualified_symbol {
        return format!("{}:symbol:{}", language_id.as_str(), qualified_symbol);
    }
    format!("{}:symbol:{}", language_id.as_str(), key.symbol)
}

fn normalized_local_identity<'a>(
    language_id: &LanguageId,
    local_definition_id: &'a str,
) -> Option<&'a str> {
    let language = language_id.as_str();
    let local_language_prefix = format!("local.{language}:");
    if let Some(identity) = local_definition_id.strip_prefix(&local_language_prefix) {
        return Some(identity);
    }

    let language_prefix = format!("{language}:");
    local_definition_id
        .strip_prefix(&language_prefix)
        .map(|identity| {
            identity
                .split_once(':')
                .map_or(identity, |(_, semantic_tail)| semantic_tail)
        })
}

fn identity_basis_for_key(key: &SymbolGroupKey) -> (&str, IdentityStability) {
    if key.registry_symbol.is_some() {
        ("registry_id", IdentityStability::Semantic)
    } else if key.local_definition_id.is_some() {
        ("local_definition_id", IdentityStability::Semantic)
    } else if key.qualified_symbol.is_some() {
        ("qualified_symbol", IdentityStability::Semantic)
    } else {
        ("source_symbol", IdentityStability::ScanLocal)
    }
}

pub(crate) fn build_symbol_usage_summaries(
    facts: &ScanFacts,
    language_id: &LanguageId,
    parent_scope_limit: Option<u32>,
) -> Result<Vec<SymbolUsageSummary>, ScanFactsError> {
    let mut grouped: HashMap<SymbolGroupKey, Vec<&wax_contract::UsageSite>> = HashMap::new();
    for site in &facts.usage_sites {
        grouped
            .entry(symbol_group_key(site))
            .or_default()
            .push(site);
    }

    let mut summaries = grouped
        .into_iter()
        .map(|(key, sites)| build_symbol_summary(language_id, &key, &sites, parent_scope_limit))
        .collect::<Result<Vec<_>, _>>()?;

    summaries.sort_by(|left, right| {
        right
            .raw_invocation_count
            .cmp(&left.raw_invocation_count)
            .then_with(|| left.symbol_kind.cmp(&right.symbol_kind))
            .then_with(|| left.symbol_id.cmp(&right.symbol_id))
    });
    Ok(summaries)
}

fn build_symbol_summary(
    language_id: &LanguageId,
    key: &SymbolGroupKey,
    sites: &[&wax_contract::UsageSite],
    parent_scope_limit: Option<u32>,
) -> Result<SymbolUsageSummary, ScanFactsError> {
    let (identity_basis, identity_stability) = identity_basis_for_key(key);
    let mut files = BTreeSet::new();
    let mut parent_groups: HashMap<String, (SymbolParentScopeSummary, u32)> = HashMap::new();

    for site in sites {
        files.insert(site.location.file.clone());
        if let Some(parent) = &site.parent {
            let entry = parent_groups
                .entry(parent.parent_id.clone())
                .or_insert_with(|| {
                    (
                        SymbolParentScopeSummary {
                            parent_id: parent.parent_id.clone(),
                            symbol: parent.symbol.clone(),
                            qualified_symbol: parent.qualified_symbol.clone(),
                            scope_kind: parent.scope_kind.clone(),
                            identity_basis: parent.identity_basis.clone(),
                            identity_stability: parent.identity_stability,
                            invocation_count: 0,
                            location: parent.location.clone(),
                        },
                        0,
                    )
                });
            entry.1 = entry.1.saturating_add(1);
        }
    }

    let mut parent_rows = parent_groups
        .into_iter()
        .map(|(parent_id, (mut summary, count))| {
            summary.invocation_count = count;
            (parent_id, summary)
        })
        .collect::<Vec<_>>();
    parent_rows.sort_by(|left, right| {
        right
            .1
            .invocation_count
            .cmp(&left.1.invocation_count)
            .then_with(|| left.0.cmp(&right.0))
    });

    let parent_scope_count = u32::try_from(parent_rows.len()).map_err(|_| {
        wax_contract::ScanFactsError::ContractViolation {
            field: "symbol_usage_summary.parent_scope_count".to_owned(),
            message: "parent scope count exceeds u32 maximum".to_owned(),
        }
    })?;

    let limit = parent_scope_limit;
    let (parent_scopes, parent_scopes_truncated) = match limit {
        Some(0) => (Vec::new(), parent_scope_count > 0),
        Some(max_rows) => {
            let truncated = parent_rows.len() > max_rows as usize;
            (
                parent_rows
                    .into_iter()
                    .take(max_rows as usize)
                    .map(|(_, summary)| summary)
                    .collect(),
                truncated,
            )
        }
        None => {
            let truncated = false;
            (
                parent_rows
                    .into_iter()
                    .map(|(_, summary)| summary)
                    .collect(),
                truncated,
            )
        }
    };

    Ok(SymbolUsageSummary {
        symbol_id: build_symbol_id(language_id, key),
        symbol: key.symbol.clone(),
        qualified_symbol: key.qualified_symbol.clone(),
        symbol_kind: symbol_kind_for_status(key.match_status),
        match_status: key.match_status,
        registry_symbol: key.registry_symbol.clone(),
        local_definition_id: key.local_definition_id.clone(),
        identity_basis: identity_basis.to_owned(),
        identity_stability,
        raw_invocation_count: u32::try_from(sites.len()).map_err(|_| {
            wax_contract::ScanFactsError::ContractViolation {
                field: "symbol_usage_summary.raw_invocation_count".to_owned(),
                message: "invocation count exceeds u32 maximum".to_owned(),
            }
        })?,
        parent_scope_count,
        file_count: u32::try_from(files.len()).map_err(|_| {
            wax_contract::ScanFactsError::ContractViolation {
                field: "symbol_usage_summary.file_count".to_owned(),
                message: "file count exceeds u32 maximum".to_owned(),
            }
        })?,
        parent_scopes,
        parent_scope_limit: limit,
        parent_scopes_truncated,
    })
}

fn merge_symbol_usage_summaries(
    languages: &BTreeMap<LanguageId, ScanFacts>,
) -> Vec<SymbolUsageSummary> {
    let mut merged = Vec::new();
    for facts in languages.values() {
        merged.extend(facts.symbol_usage_summary.clone());
    }
    merged.sort_by(|left, right| {
        right
            .raw_invocation_count
            .cmp(&left.raw_invocation_count)
            .then_with(|| left.symbol_kind.cmp(&right.symbol_kind))
            .then_with(|| left.symbol_id.cmp(&right.symbol_id))
    });
    merged
}

fn build_token_usage_summaries(facts: &ScanFacts) -> Vec<TokenUsageSummary> {
    let tokens = facts
        .design_system_tokens
        .iter()
        .map(|token| (token.id.clone(), token.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut grouped =
        BTreeMap::<String, (DesignSystemToken, u32, BTreeSet<String>, BTreeSet<String>)>::new();

    for site in &facts.token_sites {
        let Some(token) = tokens.get(&site.token_id) else {
            continue;
        };
        let entry = grouped
            .entry(site.token_id.clone())
            .or_insert_with(|| (token.clone(), 0, BTreeSet::new(), BTreeSet::new()));
        entry.1 = entry.1.saturating_add(1);
        entry.2.insert(site.location.file.clone());
        if let Some(parent) = &site.parent {
            entry.3.insert(parent.parent_id.clone());
        }
    }

    grouped
        .into_values()
        .map(
            |(token, reference_count, files, parents)| TokenUsageSummary {
                token_id: token.id,
                key: token.key,
                category: token.category,
                reference_count,
                file_count: u32::try_from(files.len()).unwrap_or(u32::MAX),
                parent_scope_count: u32::try_from(parents.len()).unwrap_or(u32::MAX),
            },
        )
        .collect()
}

fn merge_token_usage_summaries(
    languages: &BTreeMap<LanguageId, ScanFacts>,
) -> Vec<TokenUsageSummary> {
    let mut rows = BTreeMap::<String, TokenUsageSummary>::new();
    for (language_id, facts) in languages {
        for summary in &facts.token_usage_summary {
            let key = format!("{}:{}", language_id.as_str(), summary.token_id);
            rows.entry(key)
                .and_modify(|existing| {
                    existing.reference_count = existing
                        .reference_count
                        .saturating_add(summary.reference_count);
                    existing.file_count = existing.file_count.saturating_add(summary.file_count);
                    existing.parent_scope_count = existing
                        .parent_scope_count
                        .saturating_add(summary.parent_scope_count);
                })
                .or_insert_with(|| summary.clone());
        }
    }
    rows.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use wax_contract::{
        DesignSystemToken, HardcodedStyleSite, LanguageMetadata, ParentScope, ScanStatus,
        SourceLocation, TokenCategory, TokenSite, UsageSite,
    };

    fn usage_site(status: MatchStatus, symbol: &str, registry: Option<&str>) -> UsageSite {
        UsageSite {
            id: format!("id:{symbol}:{status:?}"),
            location: SourceLocation {
                file: "src/App.kt".into(),
                line: 1,
                column: Some(1),
            },
            symbol: symbol.into(),
            qualified_symbol: None,
            match_status: status,
            registry_symbol: registry.map(str::to_owned),
            local_definition_id: None,
            parent: None,
        }
    }

    fn language_facts(language_id: &str, sites: Vec<UsageSite>) -> ScanFacts {
        ScanFacts {
            schema_version: SCHEMA_VERSION,
            language: LanguageMetadata {
                id: LanguageId::try_from(language_id).unwrap(),
                version: "0.0.0".into(),
                ecosystem: "test".into(),
                parser_name: "test".into(),
                parser_version: "0.0.0".into(),
            },
            snapshot_id: "snap".into(),
            scanned_at: datetime!(2026-06-20 12:00 UTC),
            status: ScanStatus::Complete,
            design_system_components: vec![],
            local_components: vec![],
            usage_sites: sites,
            diagnostics: vec![],
            metrics: Metrics {
                invocation_adoption_ratio: None,
                registry_resolution_ratio: None,
                token_reference_ratio: None,
                parse_extract_ms: 1,
                files_scanned: 1,
            },
            counts: CountSummary::default(),
            symbol_usage_summary: vec![],
            design_system_tokens: vec![],
            token_sites: vec![],
            hardcoded_style_sites: vec![],
            token_usage_summary: vec![],
        }
    }

    #[test]
    fn merged_counts_sum_raw_invocations_and_recompute_ratios() {
        let compose = language_facts(
            "compose",
            (0..600)
                .map(|_| usage_site(MatchStatus::Resolved, "Button", Some("ds.button")))
                .chain((0..150).map(|_| UsageSite {
                    match_status: MatchStatus::Local,
                    local_definition_id: Some("local.compose:Card".into()),
                    symbol: "Card".into(),
                    ..usage_site(MatchStatus::Local, "Card", None)
                }))
                .chain((0..10).map(|_| usage_site(MatchStatus::Unresolved, "Unknown", None)))
                .collect(),
        );
        let swift = language_facts(
            "swift",
            (0..200)
                .map(|_| usage_site(MatchStatus::Resolved, "Button", Some("ds.button")))
                .chain((0..40).map(|_| UsageSite {
                    match_status: MatchStatus::Local,
                    local_definition_id: Some("local.swift:Card".into()),
                    symbol: "Card".into(),
                    ..usage_site(MatchStatus::Local, "Card", None)
                }))
                .chain((0..5).map(|_| usage_site(MatchStatus::Unresolved, "Unknown", None)))
                .collect(),
        );

        let mut languages = BTreeMap::new();
        languages.insert(LanguageId::try_from("compose").unwrap(), compose);
        languages.insert(LanguageId::try_from("swift").unwrap(), swift);

        let merged = merge_language_scans(languages).unwrap();
        let counts = &merged.repo_summary.counts.raw_invocations;

        assert_eq!(counts.total, 1005);
        assert_eq!(counts.resolved, 800);
        assert_eq!(counts.local, 190);
        assert_eq!(counts.unresolved, 15);

        let adoption = &merged
            .repo_summary
            .metrics
            .invocation_adoption_ratio
            .unwrap();
        assert!((adoption - (800.0 / 1005.0)).abs() <= 1e-12);

        let resolution = &merged
            .repo_summary
            .metrics
            .registry_resolution_ratio
            .unwrap();
        assert!((resolution - (800.0 / 1005.0)).abs() <= 1e-12);
    }

    #[test]
    fn merged_counts_sum_token_facts_and_build_summaries() {
        let mut compose = language_facts("compose", vec![]);
        compose.design_system_tokens = vec![DesignSystemToken {
            id: "color.primary".into(),
            key: "Theme.colors.primary".into(),
            category: TokenCategory::Color,
            aliases: vec![],
        }];
        compose.token_sites = vec![
            TokenSite {
                id: "token.compose:src/Screen.kt:1:1:color.primary".into(),
                location: SourceLocation {
                    file: "src/Screen.kt".into(),
                    line: 1,
                    column: Some(1),
                },
                token_id: "color.primary".into(),
                key: "Theme.colors.primary".into(),
                category: TokenCategory::Color,
                parent: Some(ParentScope {
                    parent_id: "compose:composable:com.example.Screen".into(),
                    symbol: "Screen".into(),
                    qualified_symbol: Some("com.example.Screen".into()),
                    scope_kind: "composable".into(),
                    identity_basis: "package_qualified_symbol".into(),
                    identity_stability: IdentityStability::Semantic,
                    location: None,
                }),
            },
            TokenSite {
                id: "token.compose:src/Screen.kt:2:1:color.primary".into(),
                location: SourceLocation {
                    file: "src/Other.kt".into(),
                    line: 2,
                    column: Some(1),
                },
                token_id: "color.primary".into(),
                key: "Theme.colors.primary".into(),
                category: TokenCategory::Color,
                parent: None,
            },
        ];
        compose.hardcoded_style_sites = vec![HardcodedStyleSite {
            id: "hardcoded.compose:src/Screen.kt:3:12:spacing".into(),
            location: SourceLocation {
                file: "src/Screen.kt".into(),
                line: 3,
                column: Some(12),
            },
            value: "8.dp".into(),
            category: TokenCategory::Spacing,
            parent: None,
        }];

        let mut languages = BTreeMap::new();
        languages.insert(LanguageId::try_from("compose").unwrap(), compose);

        let merged = merge_language_scans(languages).unwrap();
        let tokens = &merged.repo_summary.counts.tokens;

        assert_eq!(tokens.configured_token_count, 1);
        assert_eq!(tokens.used_token_count, 1);
        assert_eq!(tokens.token_reference_site_count, 2);
        assert_eq!(tokens.hardcoded_style_candidate_count, 1);
        assert_eq!(tokens.token_references_by_category.color, 2);
        assert_eq!(tokens.hardcoded_by_category.spacing, 1);
        assert_eq!(tokens.parent_scopes_with_token_references, 1);
        assert_eq!(
            merged.repo_summary.metrics.token_reference_ratio,
            Some(2.0 / 3.0)
        );

        let compose_facts = merged
            .languages
            .get(&LanguageId::try_from("compose").unwrap())
            .expect("compose facts");
        assert_eq!(compose_facts.token_usage_summary.len(), 1);
        let summary = &compose_facts.token_usage_summary[0];
        assert_eq!(summary.token_id, "color.primary");
        assert_eq!(summary.reference_count, 2);
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.parent_scope_count, 1);

        assert_eq!(merged.token_usage_summary.len(), 1);
        assert_eq!(merged.token_usage_summary[0].token_id, "color.primary");
    }

    #[test]
    fn local_symbol_ids_use_normalized_semantic_identity() {
        let compose = language_facts(
            "compose",
            vec![UsageSite {
                match_status: MatchStatus::Local,
                local_definition_id: Some("local.compose:com.example.Card".into()),
                qualified_symbol: Some("com.example.Card".into()),
                symbol: "Card".into(),
                ..usage_site(MatchStatus::Local, "Card", None)
            }],
        );
        let react = language_facts(
            "react",
            vec![UsageSite {
                match_status: MatchStatus::Local,
                local_definition_id: Some("react:component:src/App#Card".into()),
                qualified_symbol: Some("src/App#Card".into()),
                symbol: "Card".into(),
                ..usage_site(MatchStatus::Local, "Card", None)
            }],
        );

        let compose_summary =
            build_symbol_usage_summaries(&compose, &LanguageId::try_from("compose").unwrap(), None)
                .unwrap();
        let react_summary =
            build_symbol_usage_summaries(&react, &LanguageId::try_from("react").unwrap(), None)
                .unwrap();

        assert_eq!(
            compose_summary[0].symbol_id,
            "compose:local:com.example.Card"
        );
        assert_eq!(react_summary[0].symbol_id, "react:local:src/App#Card");
    }

    #[test]
    fn candidate_and_resolved_registry_summaries_have_distinct_ids() {
        let facts = language_facts(
            "compose",
            vec![
                usage_site(MatchStatus::Resolved, "Button", Some("ds.button")),
                usage_site(MatchStatus::Candidate, "Button", Some("ds.button")),
            ],
        );

        let summaries =
            build_symbol_usage_summaries(&facts, &LanguageId::try_from("compose").unwrap(), None)
                .unwrap();
        let ids = summaries
            .iter()
            .map(|summary| summary.symbol_id.as_str())
            .collect::<Vec<_>>();

        assert!(ids.contains(&"compose:registry:ds.button"));
        assert!(ids.contains(&"compose:candidate:ds.button"));
    }
}
