//! Generic text line-scanner language pack.
//!
//! # Examples
//!
//! An empty config runs the pack in scaffold mode without reading repository files:
//!
//! ```
//! use wax_contract::ScanStatus;
//! use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType, WIRE_API_VERSION};
//! use wax_lang_basic::BasicLanguage;
//!
//! let request = ScanRequest {
//!     request_type: ScanRequestType::Scan,
//!     api_version: WIRE_API_VERSION,
//!     language_id: "basic".try_into()?,
//!     repo_root: ".".to_owned(),
//!     snapshot_id: "docs-basic".to_owned(),
//!     config: ScanConfig::new(),
//! };
//! let facts = BasicLanguage::new().scan(&request)?;
//!
//! assert_eq!(facts.status, ScanStatus::Partial);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![deny(missing_docs)]

mod line_scan;

use std::path::Path;

use thiserror::Error;
use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::{ScanRequest, build_version};

pub use line_scan::{
    BasicConfigMode, BasicScanConfig, LineScanError, LineScanResult, parse_basic_scan_config,
    scan_repository,
};

const BASIC_LANGUAGE_ID: &str = "basic";

/// Errors returned by [`BasicLanguage::scan`].
#[derive(Debug, Error)]
pub enum BasicScanError {
    /// The request contains an invalid language id.
    #[error("invalid basic language id: {0}")]
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    #[error("basic facts validation failed: {0}")]
    InvalidFacts(#[from] ScanFactsError),
    /// Basic scan config was present but invalid.
    #[error("invalid basic scan config: {0}")]
    InvalidConfig(String),
    /// Line scanner failed before facts could be assembled.
    #[error("basic line scan failed: {0}")]
    LineScan(#[source] LineScanError),
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
    ///
    /// # Errors
    ///
    /// Returns [`BasicScanError::InvalidLanguageId`] for a non-basic request,
    /// [`BasicScanError::InvalidConfig`] for invalid config or registry data,
    /// [`BasicScanError::LineScan`] for filesystem scan failures, or
    /// [`BasicScanError::InvalidFacts`] when produced facts violate the contract.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, BasicScanError> {
        let basic_language_id = parse_basic_language_id(BASIC_LANGUAGE_ID)?;

        if request.language_id != basic_language_id {
            return Err(BasicScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = match parse_basic_scan_config(&request.config)
            .map_err(map_line_scan_error)?
        {
            BasicConfigMode::Scaffold => scaffold_facts(request, &basic_language_id),
            BasicConfigMode::Configured(scan_config) => {
                let repo_root = Path::new(&request.repo_root);
                let scan = scan_repository(repo_root, &scan_config).map_err(map_line_scan_error)?;
                facts_from_scan(request, scan, &basic_language_id)
            }
        };

        facts
            .recompute_counts()
            .map_err(BasicScanError::InvalidFacts)?;
        facts.validate().map_err(BasicScanError::InvalidFacts)?;

        Ok(facts)
    }
}

fn parse_basic_language_id(language_id: &str) -> Result<LanguageId, BasicScanError> {
    LanguageId::try_from(language_id)
        .map_err(|_| BasicScanError::InvalidLanguageId(language_id.to_owned()))
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

fn facts_from_scan(
    request: &ScanRequest,
    scan: LineScanResult,
    language_id: &LanguageId,
) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: language_id.clone(),
            version: build_version().to_owned(),
            ecosystem: "basic".to_owned(),
            parser_name: "text-line-scanner".to_owned(),
            parser_version: build_version().to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: scan.status,
        design_system_components: scan.design_system_components,
        local_components: scan.local_components,
        usage_sites: scan.usage_sites,
        diagnostics: scan.diagnostics,
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            token_reference_ratio: None,
            parse_extract_ms: 0,
            files_scanned: scan.files_scanned,
        },
        counts: CountSummary::default(),
        symbol_usage_summary: vec![],
        design_system_tokens: scan.design_system_tokens,
        token_sites: scan.token_sites,
        hardcoded_style_sites: vec![],
        token_usage_summary: vec![],
    }
}

fn scaffold_facts(request: &ScanRequest, language_id: &LanguageId) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: language_id.clone(),
            version: build_version().to_owned(),
            ecosystem: "basic".to_owned(),
            parser_name: "text-line-scanner".to_owned(),
            parser_version: build_version().to_owned(),
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
            message: "Basic text scanner is scaffolded; configure registry and roots to scan."
                .to_owned(),
            location: None,
        }],
        metrics: Metrics {
            invocation_adoption_ratio: None,
            registry_resolution_ratio: None,
            token_reference_ratio: None,
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

#[cfg(test)]
mod tests {
    use super::{BasicScanError, parse_basic_language_id};

    #[test]
    fn invalid_hardcoded_language_id_maps_to_typed_scan_error() {
        let invalid_id = "Basic!";

        let error = parse_basic_language_id(invalid_id).expect_err("language id should be invalid");

        assert!(
            matches!(error, BasicScanError::InvalidLanguageId(ref id) if id == invalid_id),
            "unexpected error: {error}"
        );
    }
}
