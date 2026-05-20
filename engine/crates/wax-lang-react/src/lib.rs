//! React language pack implementation.

#![deny(missing_docs)]

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::ScanRequest;

/// Errors returned by [`ReactLanguage::scan`].
#[derive(Debug)]
pub enum ReactScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
}

impl std::fmt::Display for ReactScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid react language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "react facts validation failed: {err}"),
        }
    }
}

impl std::error::Error for ReactScanError {}

/// React language extractor.
#[derive(Debug, Default)]
pub struct ReactLanguage;

impl ReactLanguage {
    /// Creates a react language extractor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Executes a react scan for the provided request.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, ReactScanError> {
        let react_language_id =
            LanguageId::try_from("react").expect("hardcoded react id must be valid");

        if request.language_id != react_language_id {
            return Err(ReactScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = ScanFacts {
            schema_version: SCHEMA_VERSION,
            language: LanguageMetadata {
                id: react_language_id,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                ecosystem: "react".to_owned(),
                parser_name: "react-parser".to_owned(),
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
                code: "react_scaffold".to_owned(),
                message: "React extraction is scaffolded but not implemented.".to_owned(),
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
            .map_err(ReactScanError::InvalidFacts)?;
        facts.validate().map_err(ReactScanError::InvalidFacts)?;

        Ok(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::ReactLanguage;
    use wax_contract::{DiagnosticSeverity, ScanStatus};
    use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType};

    #[test]
    fn scan_returns_stub_partial_facts_for_react() {
        let request = ScanRequest {
            request_type: ScanRequestType::Scan,
            api_version: 1,
            language_id: "react".try_into().unwrap(),
            repo_root: "/tmp/repo".to_owned(),
            snapshot_id: "snap-react".to_owned(),
            config: ScanConfig::new(),
        };

        let facts = ReactLanguage::new().scan(&request).unwrap();

        assert_eq!(facts.language.id.as_str(), "react");
        assert_eq!(facts.snapshot_id, "snap-react");
        assert_eq!(facts.status, ScanStatus::Partial);
        assert!(facts.design_system_components.is_empty());
        assert!(facts.local_components.is_empty());
        assert!(facts.usage_sites.is_empty());
        assert_eq!(facts.diagnostics.len(), 1);
        assert_eq!(facts.diagnostics[0].severity, DiagnosticSeverity::Info);
        assert_eq!(facts.diagnostics[0].code, "react_scaffold");
        assert!(
            facts.diagnostics[0]
                .message
                .contains("scaffolded but not implemented")
        );
    }
}
