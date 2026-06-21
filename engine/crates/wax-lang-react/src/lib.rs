//! React language pack implementation.

#![deny(missing_docs)]

mod component_detect;
mod config;
mod diagnostics;
mod discover;
mod extract;
mod facts;
mod files;
mod module_graph;
mod registry;
mod swc_parse;

use std::path::Path;

use wax_contract::{LanguageId, ScanFacts, ScanFactsError};
use wax_lang_api::{DiscoverRequest, DiscoveredRegistrySymbol, ScanRequest};

pub use config::{PackageConfig, ReactConfigMode, ReactScanConfig, parse_react_scan_config};
pub use discover::{DiscoverRegistryResult, ReactDiscoverError, discover_registry_symbols};
pub use extract::{ReactUsageExtraction, collect_usage_sites, discover_local_components};
pub use facts::{configured_scan_facts, scaffold_facts};
pub use files::{ReactFileCollectionError, ReactSourceFileCollection, collect_react_source_files};
pub use module_graph::{
    ExportBinding, ImportBinding, ImportedSymbol, ReactModuleGraph, ReactModuleGraphBuild,
    ReactModuleRecord, ResolvedSymbol, build_react_module_graph,
};
pub use registry::{ReactRegistryIndex, RegistryError, RegistryErrorKind, load_react_registry};
pub use swc_parse::{
    ParsedReactModule, ReactParseError, ReactParseOutcome, SWC_PARSER_VERSION,
    parse_react_source_file,
};

/// Result of a React registry symbol discovery request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverSymbolsResult {
    /// Discovered design-system symbols with optional package identity.
    pub components: Vec<DiscoveredRegistrySymbol>,
    /// Structured diagnostics emitted with the result.
    pub diagnostics: Vec<wax_contract::Diagnostic>,
}

impl DiscoverSymbolsResult {
    /// Returns discovered symbol names in stable order.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        DiscoveredRegistrySymbol::symbol_names(&self.components)
    }
}

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
    Registry(RegistryError),
    /// A filesystem operation failed during source collection.
    Io {
        /// Human-readable context for the failed operation.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// SWC parsing failed before facts could be assembled.
    Parse(ReactParseError),
}

impl std::fmt::Display for ReactScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid react language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "react facts validation failed: {err}"),
            Self::InvalidConfig(reason) => write!(f, "invalid react scan config: {reason}"),
            Self::Registry(err) => match err.kind() {
                RegistryErrorKind::NotFound => {
                    write!(f, "react registry not found: {}", err.reason())
                }
                RegistryErrorKind::Invalid => {
                    write!(f, "invalid react registry: {}", err.reason())
                }
            },
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Parse(err) => write!(f, "react parse failed: {err}"),
        }
    }
}

impl std::error::Error for ReactScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::InvalidConfig(_) => None,
            Self::Registry(err) => Some(err),
            Self::InvalidFacts(err) => Some(err),
            Self::Io { source, .. } => Some(source),
            Self::Parse(err) => Some(err),
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
        Self::Parse(err)
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
                let registry =
                    load_react_registry(&registry_path).map_err(ReactScanError::Registry)?;
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

    /// Discovers likely public React component symbols for the provided request.
    pub fn discover(
        &self,
        request: &DiscoverRequest,
    ) -> Result<DiscoverSymbolsResult, ReactDiscoverError> {
        let react_language_id =
            LanguageId::try_from("react").expect("hardcoded react id must be valid");

        if request.language_id != react_language_id {
            return Err(ReactDiscoverError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let repo_root = Path::new(&request.repo_root);
        let absolute_roots = request
            .roots
            .iter()
            .map(|root| repo_root.join(root))
            .collect::<Vec<_>>();

        let result = discover_registry_symbols(repo_root, &absolute_roots)?;

        Ok(DiscoverSymbolsResult {
            components: result.components,
            diagnostics: result.diagnostics,
        })
    }
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
            repo_root.join(".wax/react.registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
        )
        .unwrap();

        let mut config = ScanConfig::new();
        config.insert(
            "registry".to_owned(),
            serde_json::Value::String(".wax/react.registry.json".to_owned()),
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
        assert!(
            !facts
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "react_scaffold")
        );
    }

    #[test]
    fn configured_scan_emits_resolved_usage_sites() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let repo_root = tempdir.path();
        fs::create_dir_all(repo_root.join("src/ds")).unwrap();
        fs::create_dir_all(repo_root.join(".wax")).unwrap();
        fs::write(
            repo_root.join("src/App.tsx"),
            r#"import { Button } from "@acme/design-system"; export const App = () => <Button />;"#,
        )
        .unwrap();
        fs::write(
            repo_root.join("src/ds/Button.tsx"),
            "export const Button = () => null;",
        )
        .unwrap();
        fs::write(
            repo_root.join(".wax/react.registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Button","targets":["react"]}]}"#,
        )
        .unwrap();

        let mut config = ScanConfig::new();
        config.insert(
            "registry".to_owned(),
            serde_json::Value::String(".wax/react.registry.json".to_owned()),
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
                        "Button": "src/ds/Button.tsx"
                    }
                }
            }),
        );

        let request = ScanRequest {
            request_type: ScanRequestType::Scan,
            api_version: 1,
            language_id: "react".try_into().unwrap(),
            repo_root: repo_root.to_string_lossy().to_string(),
            snapshot_id: "snap-react-resolved".to_owned(),
            config,
        };

        let facts = ReactLanguage::new().scan(&request).unwrap();

        assert_eq!(facts.status, ScanStatus::Complete);
        assert_eq!(facts.counts.raw_invocations.total, 1);
        assert_eq!(facts.counts.raw_invocations.resolved, 1);
        assert_eq!(facts.language.parser_name, "swc");
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
        assert_eq!(facts.language.parser_name, "swc");
        assert!(facts.design_system_components.is_empty());
        assert!(facts.local_components.is_empty());
        assert!(facts.usage_sites.is_empty());
        assert_eq!(facts.diagnostics.len(), 1);
        assert_eq!(facts.diagnostics[0].severity, DiagnosticSeverity::Info);
        assert_eq!(facts.diagnostics[0].code, "react_scaffold");
        assert!(
            facts.diagnostics[0]
                .message
                .contains("configure registry and roots")
        );
    }
}
