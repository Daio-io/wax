//! Compose language pack implementation.

#![deny(missing_docs)]

mod tree_sitter_scan;

use std::path::Path;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::ScanRequest;

use tree_sitter_scan::TreeSitterScanError;
pub use tree_sitter_scan::{ComposeConfigMode, ComposeScanConfig};

/// Errors returned by [`ComposeLanguage::scan`].
#[derive(Debug)]
pub enum ComposeScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
    /// Compose scan config was present but invalid.
    InvalidConfig(String),
    /// Tree-sitter parser failed to initialise.
    ParserInitFailed(String),
    /// Tree-sitter scanner failed before facts could be assembled.
    Scanner(TreeSitterScanError),
}

impl std::fmt::Display for ComposeScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid compose language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "compose facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid compose scan config: {reason}"),
            Self::ParserInitFailed(reason) => write!(f, "parser init failed: {reason}"),
            Self::Scanner(err) => write!(f, "compose scan failed: {err}"),
        }
    }
}

impl std::error::Error for ComposeScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::InvalidConfig(_) | Self::ParserInitFailed(_) => None,
            Self::InvalidFacts(err) => Some(err),
            Self::Scanner(err) => Some(err),
        }
    }
}

/// Compose language extractor backed by `tree-sitter-kotlin`.
#[derive(Debug, Default)]
pub struct ComposeLanguage;

impl ComposeLanguage {
    /// Creates a Compose language extractor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Executes a Compose scan for the provided request.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, ComposeScanError> {
        let compose_language_id =
            LanguageId::try_from("compose").expect("hardcoded compose id must be valid");

        if request.language_id != compose_language_id {
            return Err(ComposeScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = match tree_sitter_scan::parse_compose_scan_config(&request.config)
            .map_err(map_scan_error)?
        {
            ComposeConfigMode::Scaffold => scaffold_facts(request),
            ComposeConfigMode::Configured(scan_config) => {
                let repo_root = Path::new(&request.repo_root);
                let result = tree_sitter_scan::scan_repository(repo_root, &scan_config)
                    .map_err(map_scan_error)?;
                facts_from_scan(request, result)
            }
        };

        facts
            .recompute_counts()
            .map_err(ComposeScanError::InvalidFacts)?;
        facts.validate().map_err(ComposeScanError::InvalidFacts)?;

        Ok(facts)
    }
}

fn map_scan_error(err: TreeSitterScanError) -> ComposeScanError {
    match err {
        TreeSitterScanError::ConfigInvalid { reason } => ComposeScanError::InvalidConfig(reason),
        TreeSitterScanError::ParserInitFailed { reason } => {
            ComposeScanError::ParserInitFailed(reason)
        }
        other => ComposeScanError::Scanner(other),
    }
}

fn facts_from_scan(
    request: &ScanRequest,
    result: tree_sitter_scan::TreeSitterScanResult,
) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("compose").expect("hardcoded compose id must be valid"),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ecosystem: "compose".to_owned(),
            parser_name: "tree-sitter-kotlin".to_owned(),
            parser_version: tree_sitter_kotlin_version(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: result.status,
        design_system_components: result.design_system_components,
        local_components: result.local_components,
        usage_sites: result.usage_sites,
        diagnostics: result.diagnostics,
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned: result.files_scanned,
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

fn scaffold_facts(request: &ScanRequest) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("compose").expect("hardcoded compose id must be valid"),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ecosystem: "compose".to_owned(),
            parser_name: "tree-sitter-kotlin".to_owned(),
            parser_version: tree_sitter_kotlin_version(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: ScanStatus::Partial,
        design_system_components: Vec::new(),
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "compose_scaffold".to_owned(),
            message:
                "Compose extraction is scaffolded; configure design_system_registry and roots to scan."
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

fn tree_sitter_kotlin_version() -> String {
    tree_sitter_scan::TREE_SITTER_KOTLIN_GRAMMAR_VERSION.to_owned()
}
