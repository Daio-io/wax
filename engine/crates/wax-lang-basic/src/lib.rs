//! Generic text line-scanner language pack.

#![deny(missing_docs)]

mod line_scan;

use std::path::Path;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::ScanRequest;

pub use line_scan::{
    BasicConfigMode, BasicScanConfig, LineScanError, LineScanResult, parse_basic_scan_config,
    scan_repository,
};

/// Errors returned by [`BasicLanguage::scan`].
#[derive(Debug)]
pub enum BasicScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
    /// Basic scan config was present but invalid.
    InvalidConfig(String),
    /// Line scanner failed before facts could be assembled.
    LineScan(LineScanError),
}

impl std::fmt::Display for BasicScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid basic language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "basic facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid basic scan config: {reason}"),
            Self::LineScan(err) => write!(f, "basic line scan failed: {err}"),
        }
    }
}

impl std::error::Error for BasicScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::InvalidConfig(_) => None,
            Self::InvalidFacts(err) => Some(err),
            Self::LineScan(err) => Some(err),
        }
    }
}

/// Generic text line-scanner language extractor.
#[derive(Debug, Default)]
pub struct BasicLanguage;

impl BasicLanguage {
    /// Creates a basic language extractor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Executes a basic scan for the provided request.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, BasicScanError> {
        let basic_language_id =
            LanguageId::try_from("basic").expect("hardcoded basic id must be valid");

        if request.language_id != basic_language_id {
            return Err(BasicScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = match parse_basic_scan_config(&request.config)
            .map_err(map_line_scan_error)?
        {
            BasicConfigMode::Scaffold => scaffold_facts(request),
            BasicConfigMode::Configured(scan_config) => {
                let repo_root = Path::new(&request.repo_root);
                let scan = scan_repository(repo_root, &scan_config).map_err(map_line_scan_error)?;
                facts_from_scan(request, scan)
            }
        };

        facts
            .recompute_counts()
            .map_err(BasicScanError::InvalidFacts)?;
        facts.validate().map_err(BasicScanError::InvalidFacts)?;

        Ok(facts)
    }
}

fn map_line_scan_error(err: LineScanError) -> BasicScanError {
    match err {
        LineScanError::ConfigInvalid { reason } => BasicScanError::InvalidConfig(reason),
        LineScanError::RegistryInvalid { path, reason } => BasicScanError::InvalidConfig(format!(
            "invalid design-system registry at {}: {reason}",
            path.display()
        )),
        other => BasicScanError::LineScan(other),
    }
}

fn facts_from_scan(request: &ScanRequest, scan: LineScanResult) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: LanguageId::try_from("basic").expect("hardcoded basic id must be valid"),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ecosystem: "basic".to_owned(),
            parser_name: "text-line-scanner".to_owned(),
            parser_version: "0.1.0".to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: scan.status,
        design_system_components: scan.design_system_components,
        local_components: scan.local_components,
        usage_sites: scan.usage_sites,
        diagnostics: scan.diagnostics,
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned: scan.files_scanned,
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
            id: LanguageId::try_from("basic").expect("hardcoded basic id must be valid"),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ecosystem: "basic".to_owned(),
            parser_name: "text-line-scanner".to_owned(),
            parser_version: "0.1.0".to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: ScanStatus::Partial,
        design_system_components: Vec::new(),
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "basic_scaffold".to_owned(),
            message: "Basic text scanner is scaffolded; configure design_system_registry and roots to scan."
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
