//! Compose language pack implementation.

#![deny(missing_docs)]

mod reference_scan;

use std::path::Path;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, LanguageId, LanguageMetadata, Metrics, SCHEMA_VERSION, ScanFacts,
    ScanFactsError, ScanStatus,
};
use wax_lang_api::ScanRequest;

pub use reference_scan::{
    ComposeConfigMode, ComposeScanConfig, ReferenceScanError, ReferenceScanResult,
};

/// Errors returned by [`ComposeLanguage::scan`].
#[derive(Debug)]
pub enum ComposeScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
    /// Compose scan config was present but invalid.
    InvalidConfig(String),
    /// Reference scanner failed before facts could be assembled.
    ReferenceScan(ReferenceScanError),
}

impl std::fmt::Display for ComposeScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid compose language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "compose facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid compose scan config: {reason}"),
            Self::ReferenceScan(err) => write!(f, "compose reference scan failed: {err}"),
        }
    }
}

impl std::error::Error for ComposeScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::InvalidConfig(_) => None,
            Self::InvalidFacts(err) => Some(err),
            Self::ReferenceScan(err) => Some(err),
        }
    }
}

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

        let mut facts = match reference_scan::parse_compose_scan_config(&request.config)
            .map_err(map_reference_error)?
        {
            reference_scan::ComposeConfigMode::Scaffold => scaffold_facts(request),
            reference_scan::ComposeConfigMode::Configured(scan_config) => {
                let repo_root = Path::new(&request.repo_root);
                let reference = reference_scan::scan_repository(repo_root, &scan_config)
                    .map_err(map_reference_error)?;
                facts_from_reference(request, reference)
            }
        };

        facts
            .recompute_counts()
            .map_err(ComposeScanError::InvalidFacts)?;
        facts.validate().map_err(ComposeScanError::InvalidFacts)?;

        Ok(facts)
    }
}

fn map_reference_error(err: ReferenceScanError) -> ComposeScanError {
    match err {
        ReferenceScanError::ConfigInvalid { reason } => ComposeScanError::InvalidConfig(reason),
        other => ComposeScanError::ReferenceScan(other),
    }
}

fn facts_from_reference(request: &ScanRequest, reference: ReferenceScanResult) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("compose").expect("hardcoded compose id must be valid"),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ecosystem: "compose".to_owned(),
            parser_name: "compose-reference-scanner".to_owned(),
            parser_version: "0.1.0".to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: reference.status,
        design_system_components: reference.design_system_components,
        local_components: reference.local_components,
        usage_sites: reference.usage_sites,
        diagnostics: reference.diagnostics,
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned: reference.files_scanned,
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
            message: "Compose extraction entrypoint is scaffolded; parser integration is pending."
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
