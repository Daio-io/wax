//! SwiftUI language pack implementation.

#![deny(missing_docs)]

mod component_detect;
pub mod discover;
mod swift_ast;
mod tree_sitter_scan;

use std::path::Path;

pub use discover::{DiscoverRegistryResult, SwiftDiscoverError, discover_registry_symbols};
use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::{DiscoverRequest, DiscoveredRegistrySymbol, ScanRequest, build_version};

/// Parser version bundled through the `tree-sitter-swift` dependency.
pub const TREE_SITTER_SWIFT_GRAMMAR_VERSION: &str = "0.6.0";
use tree_sitter_scan::TreeSitterScanError;
pub use tree_sitter_scan::{SwiftConfigMode, SwiftScanConfig};

/// Result of a Swift registry symbol discovery request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverSymbolsResult {
    /// Discovered design-system symbols with optional package identity.
    pub components: Vec<DiscoveredRegistrySymbol>,
    /// Non-fatal diagnostics emitted during discovery.
    pub diagnostics: Vec<Diagnostic>,
}

impl DiscoverSymbolsResult {
    /// Returns discovered symbol names in stable order.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        DiscoveredRegistrySymbol::symbol_names(&self.components)
    }
}

/// Errors returned by [`SwiftLanguage::scan`].
#[derive(Debug)]
pub enum SwiftScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
    /// Swift scan config was present but invalid.
    InvalidConfig(String),
    /// Tree-sitter parser failed to initialise.
    ParserInitFailed(String),
    /// Configured design-system registry file does not exist.
    RegistryNotFound(String),
    /// Tree-sitter scanner failed before facts could be assembled.
    Scanner(TreeSitterScanError),
}

impl std::fmt::Display for SwiftScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "swift facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid swift scan config: {reason}"),
            Self::ParserInitFailed(reason) => write!(f, "parser init failed: {reason}"),
            Self::RegistryNotFound(reason) => write!(f, "swift registry not found: {reason}"),
            Self::Scanner(err) => write!(f, "swift scan failed: {err}"),
        }
    }
}

impl std::error::Error for SwiftScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_)
            | Self::InvalidConfig(_)
            | Self::ParserInitFailed(_)
            | Self::RegistryNotFound(_) => None,
            Self::InvalidFacts(err) => Some(err),
            Self::Scanner(err) => Some(err),
        }
    }
}

/// SwiftUI language extractor.
#[derive(Debug, Default)]
pub struct SwiftLanguage;

impl SwiftLanguage {
    /// Creates a SwiftUI language extractor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Executes a Swift scan for the provided request.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, SwiftScanError> {
        let swift_language_id =
            LanguageId::try_from("swift").expect("hardcoded swift id must be valid");

        if request.language_id != swift_language_id {
            return Err(SwiftScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = match tree_sitter_scan::parse_swift_scan_config(&request.config)
            .map_err(map_scan_error)?
        {
            SwiftConfigMode::Scaffold => scaffold_facts(request, swift_language_id),
            SwiftConfigMode::Configured(scan_config) => {
                let repo_root = Path::new(&request.repo_root);
                let result = tree_sitter_scan::scan_repository(repo_root, &scan_config)
                    .map_err(map_scan_error)?;
                facts_from_scan(request, result, swift_language_id)
            }
        };
        facts
            .recompute_counts()
            .map_err(SwiftScanError::InvalidFacts)?;
        facts.validate().map_err(SwiftScanError::InvalidFacts)?;
        Ok(facts)
    }

    /// Discovers likely public SwiftUI design-system component symbols.
    pub fn discover(
        &self,
        request: &DiscoverRequest,
    ) -> Result<DiscoverSymbolsResult, SwiftDiscoverError> {
        let swift_language_id =
            LanguageId::try_from("swift").expect("hardcoded swift id must be valid");

        if request.language_id != swift_language_id {
            return Err(SwiftDiscoverError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let repo_root = Path::new(&request.repo_root);
        let roots = request
            .roots
            .iter()
            .map(|root| repo_root.join(root))
            .collect::<Vec<_>>();
        let result = discover_registry_symbols(repo_root, &roots)?;

        Ok(DiscoverSymbolsResult {
            components: result.components,
            diagnostics: result.diagnostics,
        })
    }
}

fn map_scan_error(err: TreeSitterScanError) -> SwiftScanError {
    match err {
        TreeSitterScanError::ConfigInvalid { reason } => SwiftScanError::InvalidConfig(reason),
        TreeSitterScanError::ParserInitFailed { reason } => {
            SwiftScanError::ParserInitFailed(reason)
        }
        TreeSitterScanError::RegistryNotFound { path, reason } => {
            SwiftScanError::RegistryNotFound(format!(
                "design-system registry not found at {}: {reason}",
                path.display()
            ))
        }
        other => SwiftScanError::Scanner(other),
    }
}

fn facts_from_scan(
    request: &ScanRequest,
    result: tree_sitter_scan::TreeSitterScanResult,
    language_id: LanguageId,
) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: language_id,
            version: build_version().to_owned(),
            ecosystem: "swiftui".to_owned(),
            parser_name: "tree-sitter-swift".to_owned(),
            parser_version: TREE_SITTER_SWIFT_GRAMMAR_VERSION.to_owned(),
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

fn scaffold_facts(request: &ScanRequest, language_id: LanguageId) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: language_id,
            version: build_version().to_owned(),
            ecosystem: "swiftui".to_owned(),
            parser_name: "tree-sitter-swift".to_owned(),
            parser_version: TREE_SITTER_SWIFT_GRAMMAR_VERSION.to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: ScanStatus::Partial,
        design_system_components: Vec::new(),
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "swift_scaffold".to_owned(),
            message: "SwiftUI extraction is scaffolded; configure registry and roots to scan."
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
