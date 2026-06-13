//! SwiftUI language pack implementation.

#![deny(missing_docs)]

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::{DiscoverRequest, ScanRequest, build_version};

/// Parser version bundled through the `tree-sitter-swift` dependency.
pub const TREE_SITTER_SWIFT_GRAMMAR_VERSION: &str = "0.7.3";

/// Result of a Swift registry symbol discovery request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverSymbolsResult {
    /// Discovered design-system symbol names.
    pub symbols: Vec<String>,
    /// Non-fatal diagnostics emitted during discovery.
    pub diagnostics: Vec<Diagnostic>,
}

/// Errors returned by [`SwiftLanguage::scan`].
#[derive(Debug)]
pub enum SwiftScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
}

impl std::fmt::Display for SwiftScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "swift facts validation failed: {err}"),
        }
    }
}

impl std::error::Error for SwiftScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) => None,
            Self::InvalidFacts(err) => Some(err),
        }
    }
}

/// Errors returned by [`SwiftLanguage::discover`].
#[derive(Debug)]
pub enum SwiftDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
}

impl std::fmt::Display for SwiftDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
        }
    }
}

impl std::error::Error for SwiftDiscoverError {}

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

        let mut facts = scaffold_facts(request, swift_language_id);
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

        Ok(DiscoverSymbolsResult {
            symbols: Vec::new(),
            diagnostics: Vec::new(),
        })
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
