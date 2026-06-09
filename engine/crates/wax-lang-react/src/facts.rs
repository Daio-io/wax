//! ScanFacts assembly for React scans.

use std::path::Path;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanStatus,
};
use wax_lang_api::{ScanRequest, build_version};

use crate::config::ReactScanConfig;
use crate::diagnostics::is_gap_diagnostic;
use crate::extract::{collect_usage_sites, discover_local_components};
use crate::files::ReactSourceFileCollection;
use crate::module_graph::build_react_module_graph;
use crate::registry::ReactRegistryIndex;
use crate::swc_parse::{
    ParsedReactModule, ReactParseError, ReactParseOutcome, SWC_PARSER_VERSION,
    parse_react_source_file,
};

/// Builds scaffold facts for contributor smoke when React scan config is empty.
#[must_use]
pub fn scaffold_facts(request: &ScanRequest, react_language_id: LanguageId) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: react_language_metadata(react_language_id),
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: ScanStatus::Partial,
        design_system_components: Vec::new(),
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "react_scaffold".to_owned(),
            message: "React extraction is scaffolded; configure registry and roots to scan."
                .to_owned(),
            location: None,
        }],
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned: 0,
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

/// Assembles configured React scan facts from registry, source files, and parsed modules.
pub fn configured_scan_facts(
    request: &ScanRequest,
    react_language_id: LanguageId,
    registry: ReactRegistryIndex,
    collection: ReactSourceFileCollection,
    repo_root: &Path,
    config: &ReactScanConfig,
) -> Result<ScanFacts, ReactParseError> {
    let ReactSourceFileCollection {
        files,
        root_diagnostics,
    } = collection;
    let mut diagnostics = root_diagnostics;
    let mut files_scanned = 0_u32;
    let mut parsed_modules = Vec::<ParsedReactModule>::new();

    for relative_path in &files {
        files_scanned = files_scanned.saturating_add(1);
        match parse_react_source_file(repo_root, relative_path)? {
            ReactParseOutcome::Parsed(parsed) => parsed_modules.push(parsed),
            ReactParseOutcome::Failed(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    let file_collection = ReactSourceFileCollection {
        files,
        root_diagnostics: Vec::new(),
    };
    let graph_build = build_react_module_graph(
        repo_root,
        &parsed_modules,
        &file_collection,
        config,
        &registry,
    );
    diagnostics.extend(graph_build.diagnostics);

    let local_components = discover_local_components(&parsed_modules);
    let usage_extraction =
        collect_usage_sites(&parsed_modules, &graph_build.graph, config, &registry);
    diagnostics.extend(usage_extraction.diagnostics);

    let status = scan_status(&diagnostics);

    Ok(ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: react_language_metadata(react_language_id),
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status,
        design_system_components: registry.design_system_components,
        local_components,
        usage_sites: usage_extraction.usage_sites,
        diagnostics,
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned,
        },
        counts: CountSummary {
            design_system_component_count: 0,
            local_component_count: 0,
            usage_site_count: 0,
            resolved_count: 0,
            candidate_count: 0,
        },
    })
}

fn react_language_metadata(react_language_id: LanguageId) -> LanguageMetadata {
    LanguageMetadata {
        id: react_language_id,
        version: build_version().to_owned(),
        ecosystem: "react".to_owned(),
        parser_name: "swc".to_owned(),
        parser_version: SWC_PARSER_VERSION.to_owned(),
    }
}

fn scan_status(diagnostics: &[Diagnostic]) -> ScanStatus {
    let has_gaps = diagnostics
        .iter()
        .any(|diagnostic| is_gap_diagnostic(&diagnostic.code));

    if has_gaps {
        ScanStatus::Partial
    } else {
        ScanStatus::Complete
    }
}

#[cfg(test)]
mod tests {
    use super::scan_status;
    use crate::diagnostics::DS_USAGE_UNRESOLVED;
    use wax_contract::{Diagnostic, DiagnosticSeverity, ScanStatus};

    #[test]
    fn scan_status_is_partial_for_resolver_gaps() {
        let diagnostics = vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: DS_USAGE_UNRESOLVED.to_owned(),
            message: "unresolved".to_owned(),
            location: None,
        }];

        assert_eq!(scan_status(&diagnostics), ScanStatus::Partial);
    }

    #[test]
    fn scan_status_is_complete_without_gap_diagnostics() {
        assert_eq!(scan_status(&[]), ScanStatus::Complete);
    }
}
