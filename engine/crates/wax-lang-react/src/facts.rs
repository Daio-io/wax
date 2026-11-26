//! ScanFacts assembly for React scans.

use std::path::Path;
use std::time::Instant;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanStatus,
};
use wax_lang_api::{ScanRequest, build_version, parse_extract_millis};

use crate::config::ReactScanConfig;
use crate::css_token_extract::{collect_css_token_sites, css_parser};
use crate::diagnostics::is_gap_diagnostic;
use crate::extract::{collect_usage_sites, discover_local_components_in};
use crate::files::{ReactSourceFileCollection, is_css_file};
use crate::module_graph::build_react_module_graph;
use crate::registry::ReactRegistryIndex;
use crate::swc_parse::{
    ParsedReactModule, ReactParseError, ReactParseOutcome, SWC_PARSER_VERSION,
    parse_react_source_file,
};

/// Builds scaffold facts for contributor smoke when React scan config is empty.
#[must_use]
pub fn scaffold_facts(request: &ScanRequest, react_language_id: &LanguageId) -> ScanFacts {
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
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            parse_extract_ms: 0,
            files_scanned: 0,
        },
        counts: CountSummary::default(),
        symbol_usage_summary: vec![],
        design_system_tokens: vec![],
        token_sites: vec![],
        hardcoded_style_sites: vec![],
        token_usage_summary: vec![],
    }
}

/// Assembles configured React scan facts from registry, source files, and parsed modules.
///
/// # Errors
///
/// Returns [`ReactParseError`] for source I/O or fatal CSS parser failures.
pub fn configured_scan_facts(
    request: &ScanRequest,
    react_language_id: &LanguageId,
    registry: ReactRegistryIndex,
    collection: ReactSourceFileCollection,
    repo_root: &Path,
    config: &ReactScanConfig,
) -> Result<ScanFacts, ReactParseError> {
    let started = Instant::now();
    let files_scanned = u32::try_from(collection.files.len()).unwrap_or(u32::MAX);
    let (css_files, react_files): (Vec<_>, Vec<_>) =
        collection.files.iter().partition(|path| is_css_file(path));
    let mut parsed_modules = Vec::<ParsedReactModule>::new();
    let mut react_diagnostics = Vec::new();
    for relative_path in react_files {
        match parse_react_source_file(repo_root, relative_path)? {
            ReactParseOutcome::Parsed(parsed) => parsed_modules.push(parsed),
            ReactParseOutcome::Failed(diagnostic) => react_diagnostics.push(diagnostic),
        }
    }

    let graph_build =
        build_react_module_graph(repo_root, &parsed_modules, &collection, config, &registry);
    let mut diagnostics = collection.root_diagnostics;
    let mut css_sites = Vec::new();
    if !css_files.is_empty() {
        let mut parser = css_parser()?;
        for relative_path in css_files {
            let owned = graph_build.graph.owning_package(relative_path).is_some();
            if !owned && registry.css_tokens.is_empty() {
                continue;
            }
            collect_css_token_sites(
                &mut parser,
                repo_root,
                relative_path,
                (!owned).then_some(&registry.css_tokens),
                &mut css_sites,
                &mut diagnostics,
            )?;
        }
    }
    diagnostics.extend(react_diagnostics);
    diagnostics.extend(graph_build.diagnostics);

    let local_components = discover_local_components_in(
        parsed_modules
            .iter()
            .filter(|parsed| graph_build.graph.owning_package(&parsed.file).is_none()),
    );
    let mut usage_extraction = collect_usage_sites(
        &parsed_modules,
        &graph_build.graph,
        config,
        &registry,
        &local_components,
    );
    diagnostics.extend(usage_extraction.diagnostics);
    if !css_sites.is_empty() {
        usage_extraction.token_sites.extend(css_sites);
        usage_extraction.token_sites.sort_by(|left, right| {
            left.location
                .file
                .cmp(&right.location.file)
                .then(left.location.line.cmp(&right.location.line))
                .then(left.location.column.cmp(&right.location.column))
                .then(left.token_id.cmp(&right.token_id))
        });
    }

    let status = scan_status(&diagnostics);
    let parse_extract_ms = parse_extract_millis(started.elapsed(), files_scanned);

    Ok(ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: react_language_metadata(react_language_id),
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status,
        design_system_components: registry.components.into_components(),
        local_components,
        usage_sites: usage_extraction.usage_sites,
        diagnostics,
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            parse_extract_ms,
            files_scanned,
        },
        counts: CountSummary::default(),
        symbol_usage_summary: vec![],
        design_system_tokens: registry.design_system_tokens,
        token_sites: usage_extraction.token_sites,
        hardcoded_style_sites: usage_extraction.hardcoded_style_sites,
        token_usage_summary: vec![],
    })
}

fn react_language_metadata(react_language_id: &LanguageId) -> LanguageMetadata {
    LanguageMetadata {
        id: react_language_id.clone(),
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
