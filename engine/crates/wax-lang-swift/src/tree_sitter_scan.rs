//! Tree-sitter-swift backed SwiftUI scanner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, LocalComponent, ScanStatus, UsageSite,
};
use wax_lang_api::{RootPatternKind, RootResolutionError, ScanConfig, resolve_source_roots};

use crate::swift_ast::{
    ParseSwiftFileError, collect_swift_files, new_parser, parse_swift_file_permissive,
};

/// Parsed Swift scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwiftScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Swift source roots to scan.
    pub roots: Vec<PathBuf>,
}

/// Whether the request should run the tree-sitter scanner or return scaffold facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwiftConfigMode {
    /// No Swift scan keys were provided.
    Scaffold,
    /// Registry and roots were provided and validated.
    Configured(SwiftScanConfig),
}

/// Errors produced by the tree-sitter Swift scanner.
#[derive(Debug)]
pub enum TreeSitterScanError {
    /// Scan config payload was present but invalid.
    ConfigInvalid {
        /// Human-readable validation failure.
        reason: String,
    },
    /// Registry JSON could not be read or parsed.
    RegistryInvalid {
        /// Registry path that failed.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },
    /// Tree-sitter parser failed to initialize.
    ParserInitFailed {
        /// Human-readable reason.
        reason: String,
    },
    /// A filesystem operation failed.
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for TreeSitterScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigInvalid { reason } => write!(f, "invalid swift scan config: {reason}"),
            Self::RegistryInvalid { path, reason } => {
                write!(
                    f,
                    "invalid design-system registry at {}: {reason}",
                    path.display()
                )
            }
            Self::ParserInitFailed { reason } => {
                write!(f, "tree-sitter parser init failed: {reason}")
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for TreeSitterScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConfigInvalid { .. }
            | Self::RegistryInvalid { .. }
            | Self::ParserInitFailed { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Loads Swift scan settings from the engine request payload.
pub fn parse_swift_scan_config(
    config: &ScanConfig,
) -> Result<SwiftConfigMode, TreeSitterScanError> {
    let has_registry =
        config.contains_key("registry") || config.contains_key("design_system_registry");
    let has_roots = config.contains_key("roots");
    if !has_registry && !has_roots {
        return Ok(SwiftConfigMode::Scaffold);
    }

    let registry = config
        .get("registry")
        .or_else(|| config.get("design_system_registry"))
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "registry is required when swift scan config is present".to_owned(),
        })?;
    let registry = registry
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "registry must be a non-empty string".to_owned(),
        })?;
    validate_repo_relative_path(registry, "registry")?;

    let roots_value = config
        .get("roots")
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "roots is required when swift scan config is present".to_owned(),
        })?;
    let roots_array = roots_value
        .as_array()
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        })?;
    if roots_array.is_empty() {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        });
    }

    let mut roots = Vec::with_capacity(roots_array.len());
    for (index, entry) in roots_array.iter().enumerate() {
        let root = entry
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: format!("roots[{index}] must be a non-empty string"),
            })?;
        validate_repo_relative_path(root, &format!("roots[{index}]"))?;
        roots.push(PathBuf::from(root));
    }

    Ok(SwiftConfigMode::Configured(SwiftScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots,
    }))
}

fn validate_repo_relative_path(path: &str, field: &str) -> Result<(), TreeSitterScanError> {
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: format!("{field} must be a repo-relative path"),
        });
    }
    if parsed
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: format!("{field} must not contain parent directory segments"),
        });
    }
    Ok(())
}

/// Output of the tree-sitter scanner before contract validation.
#[derive(Debug)]
pub struct TreeSitterScanResult {
    /// Known design-system components from the registry file.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Local SwiftUI declarations discovered in Swift sources.
    pub local_components: Vec<LocalComponent>,
    /// Usage sites matched against the registry.
    pub usage_sites: Vec<UsageSite>,
    /// Number of Swift files scanned.
    pub files_scanned: u32,
    /// Diagnostics emitted during the scan.
    pub diagnostics: Vec<Diagnostic>,
    /// Overall scan status.
    pub status: ScanStatus,
}

struct RegistryIndex {
    canonical_symbols: Vec<String>,
    #[allow(dead_code)] // populated for upcoming usage extraction work
    resolve_targets: BTreeMap<String, String>,
}

fn load_registry(path: &Path) -> Result<RegistryIndex, TreeSitterScanError> {
    let raw = fs::read_to_string(path).map_err(|source| TreeSitterScanError::Io {
        context: format!("read design-system registry {}", path.display()),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("registry JSON is invalid: {err}"),
        })?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry JSON must contain a components array".to_owned(),
        })?;

    let mut canonical_symbols = Vec::new();
    let mut resolve_targets = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if !component_available_to_swift(component, index, path)? {
            continue;
        }

        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                path: path.to_path_buf(),
                reason: format!("components[{index}] is missing symbol"),
            })?;
        canonical_symbols.push(symbol.to_owned());
        resolve_targets.insert(symbol.to_owned(), symbol.to_owned());

        if let Some(aliases) = component
            .get("aliases")
            .and_then(serde_json::Value::as_array)
        {
            for (alias_index, alias) in aliases.iter().enumerate() {
                let alias_symbol =
                    alias
                        .as_str()
                        .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                            path: path.to_path_buf(),
                            reason: format!(
                                "components[{index}].aliases[{alias_index}] must be a string"
                            ),
                        })?;
                resolve_targets.insert(alias_symbol.to_owned(), symbol.to_owned());
            }
        }
    }

    if canonical_symbols.is_empty() {
        return Err(TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry must declare at least one Swift component symbol".to_owned(),
        });
    }

    canonical_symbols.sort();
    Ok(RegistryIndex {
        canonical_symbols,
        resolve_targets,
    })
}

fn component_available_to_swift(
    component: &serde_json::Value,
    index: usize,
    path: &Path,
) -> Result<bool, TreeSitterScanError> {
    let Some(targets_value) = component.get("targets") else {
        return Ok(true);
    };
    if targets_value.is_null() {
        return Ok(true);
    }
    let Some(targets) = targets_value.as_array() else {
        return Err(TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("components[{index}].targets must be an array of strings"),
        });
    };
    for (target_index, target) in targets.iter().enumerate() {
        let target = target
            .as_str()
            .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                path: path.to_path_buf(),
                reason: format!("components[{index}].targets[{target_index}] must be a string"),
            })?;
        if target == "swift" {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Runs the tree-sitter Swift scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &SwiftScanConfig,
) -> Result<TreeSitterScanResult, TreeSitterScanError> {
    let registry_path = repo_root.join(&config.design_system_registry);
    let registry = load_registry(&registry_path)?;

    let mut swift_files = Vec::new();
    let mut diagnostics = Vec::new();
    for root in &config.roots {
        let resolved = resolve_source_roots(repo_root, root).map_err(map_root_resolution_error)?;
        if resolved.roots.is_empty() {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: root_not_found_code(resolved.kind),
                message: root_not_found_message(root, resolved.kind),
                location: None,
            });
        } else {
            for abs_root in resolved.roots {
                collect_swift_files(&abs_root, &mut swift_files).map_err(|source| {
                    TreeSitterScanError::Io {
                        context: format!("read Swift root {}", abs_root.display()),
                        source,
                    }
                })?;
            }
        }
    }
    swift_files.sort();

    let mut parser =
        new_parser().map_err(|reason| TreeSitterScanError::ParserInitFailed { reason })?;

    let mut files_scanned = 0_u32;
    let mut parse_failures = 0_u32;
    for file_path in &swift_files {
        files_scanned += 1;
        let relative_file = file_path
            .strip_prefix(repo_root)
            .unwrap_or(file_path)
            .display()
            .to_string();

        match parse_swift_file_permissive(&mut parser, file_path) {
            Ok(_parsed) => {}
            Err(ParseSwiftFileError::ParseFailed(_)) => {
                parse_failures += 1;
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "parse_failed".to_owned(),
                    message: format!("tree-sitter failed to parse {relative_file}; file skipped"),
                    location: None,
                });
            }
            Err(ParseSwiftFileError::Io { context, source }) => {
                return Err(TreeSitterScanError::Io { context, source });
            }
        }
    }

    let mut design_system_components = registry
        .canonical_symbols
        .iter()
        .map(|symbol| DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.clone(),
            registry_symbol: symbol.clone(),
        })
        .collect::<Vec<_>>();
    design_system_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));

    let has_gaps = parse_failures > 0
        || diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "root_not_found" || diagnostic.code == "root_glob_not_found"
        });

    Ok(TreeSitterScanResult {
        design_system_components,
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        files_scanned,
        diagnostics,
        status: if has_gaps {
            ScanStatus::Partial
        } else {
            ScanStatus::Complete
        },
    })
}

fn map_root_resolution_error(err: RootResolutionError) -> TreeSitterScanError {
    match err {
        RootResolutionError::Io { context, source } => TreeSitterScanError::Io { context, source },
    }
}

fn root_not_found_code(kind: RootPatternKind) -> String {
    match kind {
        RootPatternKind::Literal => "root_not_found".to_owned(),
        RootPatternKind::Wildcard => "root_glob_not_found".to_owned(),
    }
}

fn root_not_found_message(root: &Path, kind: RootPatternKind) -> String {
    match kind {
        RootPatternKind::Literal => format!(
            "configured root '{}' does not exist under repo root; no Swift files scanned from it",
            root.display()
        ),
        RootPatternKind::Wildcard => format!(
            "configured root pattern '{}' matched no directories under repo root; no Swift files scanned from it",
            root.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_rejects_parent_dir_roots() {
        let mut config = ScanConfig::new();
        config.insert("registry".to_owned(), serde_json::json!("registry.json"));
        config.insert("roots".to_owned(), serde_json::json!(["../Sources/App"]));

        let err = parse_swift_scan_config(&config).expect_err("parent-dir roots must fail");
        assert!(matches!(err, TreeSitterScanError::ConfigInvalid { .. }));
    }
}
