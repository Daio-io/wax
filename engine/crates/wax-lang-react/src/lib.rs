//! React language pack implementation.

#![deny(missing_docs)]

mod config;
mod registry;

use std::path::Path;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::{ScanRequest, build_version};

pub use config::{PackageConfig, ReactConfigMode, ReactScanConfig, parse_react_scan_config};
pub use registry::{ReactRegistryIndex, RegistryError, load_react_registry};

/// Errors returned by [`ReactLanguage::scan`].
#[derive(Debug)]
pub enum ReactScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
    /// React scan config was present but invalid.
    InvalidConfig(String),
    /// Design-system registry could not be loaded.
    RegistryInvalid(String),
}

impl std::fmt::Display for ReactScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid react language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "react facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid react scan config: {reason}"),
            Self::RegistryInvalid(reason) => write!(f, "invalid react registry: {reason}"),
        }
    }
}

impl std::error::Error for ReactScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::InvalidConfig(_) | Self::RegistryInvalid(_) => None,
            Self::InvalidFacts(err) => Some(err),
        }
    }
}

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

        let config_mode = parse_react_scan_config(&request.config)
            .map_err(|err| ReactScanError::InvalidConfig(err.reason().to_owned()))?;

        let mut facts = match config_mode {
            ReactConfigMode::Scaffold => scaffold_facts(request, react_language_id),
            ReactConfigMode::Configured(config) => {
                let registry_path =
                    Path::new(&request.repo_root).join(&config.design_system_registry);
                let registry = load_react_registry(&registry_path)
                    .map_err(|err| ReactScanError::RegistryInvalid(err.reason().to_owned()))?;
                configured_scaffold_facts(request, react_language_id, registry)
            }
        };

        facts
            .recompute_counts()
            .map_err(ReactScanError::InvalidFacts)?;
        facts.validate().map_err(ReactScanError::InvalidFacts)?;

        Ok(facts)
    }
}

fn scaffold_facts(request: &ScanRequest, react_language_id: LanguageId) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: react_language_id,
            version: build_version().to_owned(),
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
    }
}

fn configured_scaffold_facts(
    request: &ScanRequest,
    react_language_id: LanguageId,
    registry: ReactRegistryIndex,
) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: react_language_id,
            version: build_version().to_owned(),
            ecosystem: "react".to_owned(),
            parser_name: "react-parser".to_owned(),
            parser_version: "0.1.0".to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: ScanStatus::Partial,
        design_system_components: registry.design_system_components,
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
