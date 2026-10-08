//! Graph-data artifact builder for `wax scan --output graph-data=...`.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use wax_contract::{MatchStatus, MergedScan};
use wax_lang_api::normalize_repo_relative_path;

use super::scan_summary::{SUMMARY_LIMIT_CATEGORY, SUMMARY_LIMIT_MODULE, SUMMARY_LIMIT_OWNERSHIP};

/// Versioned scan graph for charting and local report UIs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanGraph {
    /// Graph schema version.
    pub schema_version: u32,
    /// Generation metadata and known data gaps.
    pub metadata: GraphMetadata,
    /// Graph nodes in deterministic id order.
    pub nodes: Vec<GraphNode>,
    /// Usage edges between files and design-system components.
    pub edges: Vec<GraphEdge>,
    /// Named metric values scoped to repo or language.
    pub metrics: Vec<GraphMetric>,
}

/// Graph generation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphMetadata {
    /// Repo-relative path to the merged scan when possible.
    pub source_scan_path: String,
    /// RFC3339 generation timestamp.
    pub generated_at: String,
    /// Known data-gap warnings (always non-empty for v1).
    pub limits: Vec<String>,
}

/// One node in the scan graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    /// Stable node id (`lang:…`, `ds:…`, `local:…`, or `file:…`).
    pub id: String,
    /// Node kind.
    pub kind: String,
    /// Human-readable label.
    pub label: String,
    /// Language id when the node is language-scoped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// One edge in the scan graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    /// Source node id.
    pub from: String,
    /// Destination node id.
    pub to: String,
    /// Edge kind (`usage` for v1).
    pub kind: String,
    /// Match status for usage edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_status: Option<String>,
}

/// One named metric value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphMetric {
    /// Metric name.
    pub name: String,
    /// Scope id (`repo` or `language:{id}`).
    pub scope: String,
    /// Metric value.
    pub value: f64,
}

/// Builds a schema-version-1 scan graph from merged scan facts.
///
/// This is a pure transform: it performs no I/O and cannot fail.
#[must_use]
pub fn build_scan_graph(merged: &MergedScan, scan_path: &Path) -> ScanGraph {
    let mut nodes_by_id: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut edges = Vec::new();
    let mut seen_edges: BTreeSet<(String, String, String, Option<String>)> = BTreeSet::new();

    for (language_id, facts) in &merged.languages {
        let lang = language_id.as_str();
        insert_node(
            &mut nodes_by_id,
            GraphNode {
                id: format!("lang:{lang}"),
                kind: "language".to_owned(),
                label: lang.to_owned(),
                language: Some(lang.to_owned()),
            },
        );

        for component in &facts.design_system_components {
            insert_node(
                &mut nodes_by_id,
                GraphNode {
                    id: format!("ds:{lang}:{}", component.registry_symbol),
                    kind: "design_system".to_owned(),
                    label: component.registry_symbol.clone(),
                    language: Some(lang.to_owned()),
                },
            );
        }

        for component in &facts.local_components {
            insert_node(
                &mut nodes_by_id,
                GraphNode {
                    id: format!("local:{lang}:{}", component.id),
                    kind: "local".to_owned(),
                    label: component.symbol.clone(),
                    language: Some(lang.to_owned()),
                },
            );
        }

        for site in &facts.usage_sites {
            let file_id = format!("file:{}", site.location.file);
            insert_node(
                &mut nodes_by_id,
                GraphNode {
                    id: file_id.clone(),
                    kind: "file".to_owned(),
                    label: site.location.file.clone(),
                    language: None,
                },
            );

            let target_id = match (
                site.match_status,
                site.registry_symbol.as_deref(),
                site.local_definition_id.as_deref(),
            ) {
                (MatchStatus::Resolved | MatchStatus::Candidate, Some(reg), _) => {
                    let ds_id = format!("ds:{lang}:{reg}");
                    insert_node(
                        &mut nodes_by_id,
                        GraphNode {
                            id: ds_id.clone(),
                            kind: "design_system".to_owned(),
                            label: reg.to_owned(),
                            language: Some(lang.to_owned()),
                        },
                    );
                    Some(ds_id)
                }
                (MatchStatus::Local, _, Some(local_id)) => {
                    let local_node_id = format!("local:{lang}:{local_id}");
                    insert_node(
                        &mut nodes_by_id,
                        GraphNode {
                            id: local_node_id.clone(),
                            kind: "local".to_owned(),
                            label: site.symbol.clone(),
                            language: Some(lang.to_owned()),
                        },
                    );
                    Some(local_node_id)
                }
                _ => None,
            };

            if let Some(to_id) = target_id {
                let match_status = Some(match_status_label(site.match_status).to_owned());
                let key = (
                    file_id.clone(),
                    to_id.clone(),
                    "usage".to_owned(),
                    match_status.clone(),
                );
                if seen_edges.insert(key) {
                    edges.push(GraphEdge {
                        from: file_id,
                        to: to_id,
                        kind: "usage".to_owned(),
                        match_status,
                    });
                }
            }
        }
    }

    let mut metrics = Vec::new();
    if let Some(value) = merged.repo_summary.metrics.invocation_adoption_ratio {
        metrics.push(GraphMetric {
            name: "invocation_adoption_ratio".to_owned(),
            scope: "repo".to_owned(),
            value,
        });
    }
    for (language_id, facts) in &merged.languages {
        if let Some(value) = facts.metrics.invocation_adoption_ratio {
            metrics.push(GraphMetric {
                name: "invocation_adoption_ratio".to_owned(),
                scope: format!("language:{}", language_id.as_str()),
                value,
            });
        }
    }

    ScanGraph {
        schema_version: 1,
        metadata: GraphMetadata {
            source_scan_path: portable_scan_path(scan_path),
            generated_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
            limits: vec![
                SUMMARY_LIMIT_MODULE.to_owned(),
                SUMMARY_LIMIT_CATEGORY.to_owned(),
                SUMMARY_LIMIT_OWNERSHIP.to_owned(),
            ],
        },
        nodes: nodes_by_id.into_values().collect(),
        edges,
        metrics,
    }
}

fn insert_node(nodes_by_id: &mut BTreeMap<String, GraphNode>, node: GraphNode) {
    nodes_by_id.entry(node.id.clone()).or_insert(node);
}

fn match_status_label(status: MatchStatus) -> &'static str {
    match status {
        MatchStatus::Resolved => "resolved",
        MatchStatus::Candidate => "candidate",
        MatchStatus::Local => "local",
        MatchStatus::Unresolved => "unresolved",
    }
}

fn portable_scan_path(scan_path: &Path) -> String {
    // Prefer a repo-relative display when the path already looks relative.
    if scan_path.is_relative() {
        return normalize_repo_relative_path(scan_path);
    }
    // Absolute destinations keep the final path segments after `.wax/` when present.
    let normalized = normalize_repo_relative_path(scan_path);
    if let Some(index) = normalized.find(".wax/") {
        return normalized[index..].to_owned();
    }
    scan_path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use time::OffsetDateTime;
    use wax_contract::{
        AdoptionCounts, CalleeOrigin, CountSummary, DefinitionCounts, LanguageId, LanguageMetadata,
        LocalComponent, Metrics, ParentScopeCounts, RawInvocationCounts, RegistryCounts,
        RepoSummary, ResolutionEvidence, ResolutionEvidenceKind, SCHEMA_VERSION, ScanFacts,
        ScanStatus, SourceLocation, UsageSite,
    };

    #[test]
    fn build_scan_graph_includes_candidate_usage_edges() {
        let merged = merged_with_sites(vec![usage(
            "site-candidate",
            "src/b.kt",
            MatchStatus::Candidate,
            Some("button"),
        )]);

        let graph = build_scan_graph(&merged, Path::new(".wax/out/scan-merged.json"));

        assert!(
            graph.edges.iter().any(|edge| {
                edge.from == "file:src/b.kt"
                    && edge.to == "ds:compose:button"
                    && edge.kind == "usage"
                    && edge.match_status.as_deref() == Some("candidate")
            }),
            "expected candidate usage edge, got: {:?}",
            graph.edges
        );
    }

    #[test]
    fn build_scan_graph_includes_local_usage_edges() {
        let merged = merged_with_sites(vec![usage_local(
            "site-local",
            "src/local.kt",
            Some("local-1"),
        )]);

        let graph = build_scan_graph(&merged, Path::new(".wax/out/scan-merged.json"));

        assert!(
            graph.edges.iter().any(|edge| {
                edge.from == "file:src/local.kt"
                    && edge.to == "local:compose:local-1"
                    && edge.kind == "usage"
                    && edge.match_status.as_deref() == Some("local")
            }),
            "expected local usage edge, got: {:?}",
            graph.edges
        );
    }

    #[test]
    fn build_scan_graph_skips_local_without_definition_id_and_unresolved() {
        let merged = merged_with_sites(vec![
            usage_local("site-local-orphan", "src/orphan.kt", None),
            usage(
                "site-unresolved",
                "src/unknown.kt",
                MatchStatus::Unresolved,
                None,
            ),
        ]);

        let graph = build_scan_graph(&merged, Path::new(".wax/out/scan-merged.json"));

        assert!(
            graph.edges.is_empty(),
            "Local without local_definition_id and Unresolved must not emit usage edges: {:?}",
            graph.edges
        );
        assert!(!graph.metadata.limits.is_empty());
    }

    fn usage(
        id: &str,
        file: &str,
        match_status: MatchStatus,
        registry_symbol: Option<&str>,
    ) -> UsageSite {
        UsageSite {
            id: id.to_owned(),
            location: SourceLocation {
                file: file.to_owned(),
                line: 1,
                column: None,
                root_group: None,
            },
            symbol: "Button".to_owned(),
            qualified_symbol: None,
            callee_origin: CalleeOrigin::Unknown,
            resolution_evidence: ResolutionEvidence {
                kind: ResolutionEvidenceKind::RegistryImportMissing,
                package: None,
            },
            match_status,
            registry_symbol: registry_symbol.map(str::to_owned),
            local_definition_id: None,
            parent: None,
        }
    }

    fn usage_local(id: &str, file: &str, local_definition_id: Option<&str>) -> UsageSite {
        UsageSite {
            id: id.to_owned(),
            location: SourceLocation {
                file: file.to_owned(),
                line: 2,
                column: None,
                root_group: None,
            },
            symbol: "LocalButton".to_owned(),
            qualified_symbol: None,
            callee_origin: CalleeOrigin::Local,
            resolution_evidence: ResolutionEvidence {
                kind: ResolutionEvidenceKind::LocalSameFile,
                package: None,
            },
            match_status: MatchStatus::Local,
            registry_symbol: None,
            local_definition_id: local_definition_id.map(str::to_owned),
            parent: None,
        }
    }

    fn merged_with_sites(usage_sites: Vec<UsageSite>) -> MergedScan {
        let language_id = LanguageId::from_str("compose").unwrap();
        MergedScan {
            schema_version: SCHEMA_VERSION,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            repo_summary: RepoSummary {
                languages: vec![language_id.clone()],
                counts: CountSummary {
                    registry: RegistryCounts::default(),
                    definitions: DefinitionCounts::default(),
                    raw_invocations: RawInvocationCounts::default(),
                    adoption: AdoptionCounts::default(),
                    parent_scopes: ParentScopeCounts::default(),
                    invocation_origins: Default::default(),
                    tokens: Default::default(),
                },
                metrics: Metrics {
                    invocation_adoption_ratio: Some(0.5),
                    registry_resolution_ratio: None,
                    parse_extract_ms: 0,
                    files_scanned: 1,
                },
            },
            symbol_usage_summary: vec![],
            token_usage_summary: vec![],
            scan_scope: Default::default(),
            token_inference: wax_contract::TokenInferenceReport::empty(2.0),
            root_groups: vec![],
            root_group_summary: vec![],
            languages: BTreeMap::from([(
                language_id.clone(),
                ScanFacts {
                    schema_version: SCHEMA_VERSION,
                    language: LanguageMetadata {
                        id: language_id,
                        version: "0.1.0".to_owned(),
                        ecosystem: "test".to_owned(),
                        parser_name: "fixture".to_owned(),
                        parser_version: "1.0.0".to_owned(),
                    },
                    snapshot_id: "snap".to_owned(),
                    scanned_at: OffsetDateTime::UNIX_EPOCH,
                    status: ScanStatus::Complete,
                    design_system_components: vec![],
                    local_components: vec![LocalComponent {
                        id: "local-1".to_owned(),
                        symbol: "LocalButton".to_owned(),
                        qualified_symbol: None,
                        identity_basis: None,
                        identity_stability: None,
                        location: SourceLocation {
                            file: "src/local.kt".to_owned(),
                            line: 1,
                            column: None,
                            root_group: None,
                        },
                    }],
                    usage_sites,
                    diagnostics: vec![],
                    metrics: Metrics {
                        invocation_adoption_ratio: Some(1.0),
                        registry_resolution_ratio: None,
                        parse_extract_ms: 0,
                        files_scanned: 1,
                    },
                    counts: CountSummary::default(),
                    symbol_usage_summary: vec![],
                    design_system_tokens: vec![],
                    token_sites: vec![],
                    hardcoded_style_sites: vec![],
                    token_usage_summary: vec![],
                },
            )]),
        }
    }
}
