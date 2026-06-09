//! React language pack implementation.

#![deny(missing_docs)]

mod config;
mod extract;
mod files;
mod module_graph;
mod registry;
mod swc_parse;

use std::path::Path;

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::{ScanRequest, build_version};

pub use config::{PackageConfig, ReactConfigMode, ReactScanConfig, parse_react_scan_config};
pub use extract::{ReactUsageExtraction, collect_usage_sites, discover_local_components};
pub use files::{ReactFileCollectionError, ReactSourceFileCollection, collect_react_source_files};
pub use module_graph::{
    ExportBinding, ImportBinding, ImportedSymbol, ReactModuleGraph, ReactModuleGraphBuild,
    ReactModuleRecord, ResolvedSymbol, build_react_module_graph,
};
pub use registry::{ReactRegistryIndex, RegistryError, load_react_registry};
pub use swc_parse::{
    ParsedReactModule, ReactParseError, ReactParseOutcome, parse_react_source_file,
};

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
    /// A filesystem operation failed during source collection.
    Io {
        /// Human-readable context for the failed operation.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for ReactScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid react language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "react facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid react scan config: {reason}"),
            Self::RegistryInvalid(reason) => write!(f, "invalid react registry: {reason}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReactScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::InvalidConfig(_) | Self::RegistryInvalid(_) => None,
            Self::InvalidFacts(err) => Some(err),
            Self::Io { source, .. } => Some(source),
        }
    }
}

impl From<ReactFileCollectionError> for ReactScanError {
    fn from(err: ReactFileCollectionError) -> Self {
        match err {
            ReactFileCollectionError::Io { context, source } => Self::Io { context, source },
        }
    }
}

impl From<ReactParseError> for ReactScanError {
    fn from(err: ReactParseError) -> Self {
        match err {
            ReactParseError::Io { context, source } => Self::Io { context, source },
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
                let repo_root = Path::new(&request.repo_root);
                let registry_path = repo_root.join(&config.design_system_registry);
                let registry = load_react_registry(&registry_path)
                    .map_err(|err| ReactScanError::RegistryInvalid(err.reason().to_owned()))?;
                let collection =
                    collect_react_source_files(repo_root, &config.roots, &config.ignore)?;
                configured_scan_facts(
                    request,
                    react_language_id,
                    registry,
                    collection,
                    repo_root,
                    &config,
                )?
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

fn configured_scan_facts(
    request: &ScanRequest,
    react_language_id: LanguageId,
    registry: ReactRegistryIndex,
    collection: ReactSourceFileCollection,
    repo_root: &Path,
    config: &ReactScanConfig,
) -> Result<ScanFacts, ReactScanError> {
    let ReactSourceFileCollection {
        files,
        root_diagnostics,
    } = collection;
    let mut diagnostics = root_diagnostics;
    let mut files_scanned = 0_u32;
    let mut parsed_modules = Vec::new();

    for relative_path in &files {
        files_scanned = files_scanned.saturating_add(1);
        match parse_react_source_file(repo_root, relative_path)? {
            ReactParseOutcome::Parsed(parsed) => parsed_modules.push(parsed),
            ReactParseOutcome::Failed(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    let file_collection = ReactSourceFileCollection {
        files,
        root_diagnostics: Vec::new(),
    };
    let graph_build = build_react_module_graph(
        repo_root,
        &parsed_modules,
        &file_collection,
        config,
        &registry,
    );
    diagnostics.extend(graph_build.diagnostics);

    diagnostics.push(Diagnostic {
        severity: DiagnosticSeverity::Info,
        code: "react_scaffold".to_owned(),
        message: "React extraction is scaffolded but not implemented.".to_owned(),
        location: None,
    });

    Ok(ScanFacts {
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
        diagnostics,
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned,
        },
        counts: CountSummary {
            design_system_component_count: 0,
            local_component_count: 0,
            usage_site_count: 0,
            resolved_count: 0,
            candidate_count: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::ReactLanguage;
    use std::fs;
    use wax_contract::{DiagnosticSeverity, ScanStatus};
    use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType};

    #[test]
    fn configured_scan_emits_module_graph_diagnostics() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let repo_root = tempdir.path();
        fs::create_dir_all(repo_root.join("src")).unwrap();
        fs::create_dir_all(repo_root.join(".wax")).unwrap();
        fs::write(
            repo_root.join("src/App.tsx"),
            r#"import { Button } from "@acme/design-system"; export const App = () => <Button />;"#,
        )
        .unwrap();
        fs::write(
            repo_root.join(".wax/wax.registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
        )
        .unwrap();

        let mut config = ScanConfig::new();
        config.insert(
            "registry".to_owned(),
            serde_json::Value::String(".wax/wax.registry.json".to_owned()),
        );
        config.insert(
            "roots".to_owned(),
            serde_json::Value::Array(vec![serde_json::Value::String("src".to_owned())]),
        );
        config.insert(
            "packages".to_owned(),
            serde_json::json!({
                "@acme/design-system": {
                    "exports": {
                        ".": "src/ds/index.ts"
                    }
                }
            }),
        );

        let request = ScanRequest {
            request_type: ScanRequestType::Scan,
            api_version: 1,
            language_id: "react".try_into().unwrap(),
            repo_root: repo_root.to_string_lossy().to_string(),
            snapshot_id: "snap-react-configured".to_owned(),
            config,
        };

        let facts = ReactLanguage::new().scan(&request).unwrap();

        assert_eq!(facts.status, ScanStatus::Partial);
        assert!(
            facts
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ds_import_unresolved")
        );
    }

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
