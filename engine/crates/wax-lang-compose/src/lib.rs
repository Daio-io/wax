//! Compose language pack implementation.

#![deny(missing_docs)]

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, LanguageId, LanguageMetadata, Metrics, SCHEMA_VERSION, ScanFacts,
    ScanStatus,
};
use wax_lang_api::ScanRequest;

/// Errors returned by [`ComposeLanguage::scan`].
#[derive(Debug)]
pub enum ComposeScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(String),
}

impl std::fmt::Display for ComposeScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid compose language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "compose facts validation failed: {err}"),
        }
    }
}

impl std::error::Error for ComposeScanError {}

/// Compose language extractor.
#[derive(Debug, Default)]
pub struct ComposeLanguage;

impl ComposeLanguage {
    /// Creates a compose language extractor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Executes a compose scan for the provided request.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, ComposeScanError> {
        let compose_language_id =
            LanguageId::try_from("compose").expect("hardcoded compose id must be valid");

        if request.language_id != compose_language_id {
            return Err(ComposeScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = ScanFacts {
            schema_version: SCHEMA_VERSION,
            language: LanguageMetadata {
                id: compose_language_id,
                version: "0.1.0".to_owned(),
                ecosystem: "compose".to_owned(),
                parser_name: "compose-parser".to_owned(),
                parser_version: "0.1.0".to_owned(),
            },
            snapshot_id: request.snapshot_id.clone(),
            scanned_at: OffsetDateTime::now_utc(),
            status: ScanStatus::Partial,
            design_system_components: Vec::new(),
            local_components: Vec::new(),
            usage_sites: Vec::new(),
            diagnostics: vec![Diagnostic {
                severity: wax_contract::DiagnosticSeverity::Info,
                code: "compose_scaffold".to_owned(),
                message:
                    "Compose extraction entrypoint is scaffolded; parser integration is pending."
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
        };

        facts
            .recompute_counts()
            .map_err(|err| ComposeScanError::InvalidFacts(err.to_string()))?;
        facts
            .validate()
            .map_err(|err| ComposeScanError::InvalidFacts(err.to_string()))?;

        Ok(facts)
    }
}
