//! Reference Kotlin scanner used by the small-fixture correctness gate.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, LocalComponent, MatchStatus, ScanStatus,
    SourceLocation, UsageSite,
};
use wax_lang_api::ScanConfig;

/// Parsed compose scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Kotlin source roots to scan.
    pub roots: Vec<PathBuf>,
}

/// Whether the request should run the reference scanner or return scaffold facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeConfigMode {
    /// No compose scan keys were provided.
    Scaffold,
    /// Registry and roots were provided and validated.
    Configured(ComposeScanConfig),
}

/// Loads compose scan settings from the engine request payload.
pub fn parse_compose_scan_config(
    config: &ScanConfig,
) -> Result<ComposeConfigMode, ReferenceScanError> {
    let has_registry = config.contains_key("design_system_registry");
    let has_roots = config.contains_key("roots");
    if !has_registry && !has_roots {
        return Ok(ComposeConfigMode::Scaffold);
    }

    let registry =
        config
            .get("design_system_registry")
            .ok_or_else(|| ReferenceScanError::ConfigInvalid {
                reason: "design_system_registry is required when compose scan config is present"
                    .to_owned(),
            })?;
    let registry = registry
        .as_str()
        .ok_or_else(|| ReferenceScanError::ConfigInvalid {
            reason: "design_system_registry must be a non-empty string".to_owned(),
        })?;
    if registry.is_empty() {
        return Err(ReferenceScanError::ConfigInvalid {
            reason: "design_system_registry must be a non-empty string".to_owned(),
        });
    }

    let roots_value = config
        .get("roots")
        .ok_or_else(|| ReferenceScanError::ConfigInvalid {
            reason: "roots is required when compose scan config is present".to_owned(),
        })?;
    let roots_array = roots_value
        .as_array()
        .ok_or_else(|| ReferenceScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        })?;
    if roots_array.is_empty() {
        return Err(ReferenceScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        });
    }

    let mut roots = Vec::with_capacity(roots_array.len());
    for (index, entry) in roots_array.iter().enumerate() {
        let root = entry
            .as_str()
            .ok_or_else(|| ReferenceScanError::ConfigInvalid {
                reason: format!("roots[{index}] must be a non-empty string"),
            })?;
        if root.is_empty() {
            return Err(ReferenceScanError::ConfigInvalid {
                reason: format!("roots[{index}] must be a non-empty string"),
            });
        }
        roots.push(PathBuf::from(root));
    }

    Ok(ComposeConfigMode::Configured(ComposeScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots,
    }))
}

/// Runs the reference scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &ComposeScanConfig,
) -> Result<ReferenceScanResult, ReferenceScanError> {
    let registry_path = repo_root.join(&config.design_system_registry);
    let registry = load_registry(&registry_path)?;
    let mut kotlin_files = Vec::new();
    for root in &config.roots {
        collect_kotlin_files(&repo_root.join(root), &mut kotlin_files)?;
    }
    kotlin_files.sort();

    let mut design_system_components = registry
        .canonical_symbols
        .iter()
        .map(|symbol| DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.clone(),
            registry_symbol: symbol.clone(),
        })
        .collect::<Vec<_>>();

    let mut local_components = Vec::new();
    let mut usage_sites = Vec::new();
    let mut files_scanned = 0_u32;

    for file_path in kotlin_files {
        files_scanned += 1;
        let source = fs::read_to_string(&file_path).map_err(|source| ReferenceScanError::Io {
            context: format!("read Kotlin source {}", file_path.display()),
            source,
        })?;
        let relative_file = file_path
            .strip_prefix(repo_root)
            .unwrap_or(&file_path)
            .display()
            .to_string();

        extract_local_components(&source, &relative_file, &mut local_components);
        extract_usage_sites(
            &source,
            &relative_file,
            &registry.resolve_targets,
            &mut usage_sites,
        );
    }

    design_system_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    local_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    usage_sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.symbol.cmp(&right.symbol))
    });

    Ok(ReferenceScanResult {
        design_system_components,
        local_components,
        usage_sites,
        files_scanned,
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "compose_reference_scan".to_owned(),
            message: "Compose reference scanner is active for correctness-gate fixtures only; tree-sitter extraction is pending.".to_owned(),
            location: None,
        }],
        status: ScanStatus::Partial,
    })
}

/// Output of the reference scanner before contract validation.
#[derive(Debug)]
pub struct ReferenceScanResult {
    /// Known design-system components from the registry file.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Local `@Composable` declarations discovered in Kotlin sources.
    pub local_components: Vec<LocalComponent>,
    /// Usage sites matched against the registry.
    pub usage_sites: Vec<UsageSite>,
    /// Number of Kotlin files scanned.
    pub files_scanned: u32,
    /// Diagnostics emitted by the reference scan.
    pub diagnostics: Vec<Diagnostic>,
    /// Overall scan status for the reference path.
    pub status: ScanStatus,
}

/// Errors produced while running the reference scanner.
#[derive(Debug)]
pub enum ReferenceScanError {
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
    /// A filesystem operation failed.
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for ReferenceScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigInvalid { reason } => write!(f, "invalid compose scan config: {reason}"),
            Self::RegistryInvalid { path, reason } => {
                write!(
                    f,
                    "invalid design-system registry at {}: {reason}",
                    path.display()
                )
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReferenceScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConfigInvalid { .. } | Self::RegistryInvalid { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

struct RegistryIndex {
    canonical_symbols: Vec<String>,
    resolve_targets: BTreeMap<String, String>,
}

fn load_registry(path: &Path) -> Result<RegistryIndex, ReferenceScanError> {
    let raw = fs::read_to_string(path).map_err(|source| ReferenceScanError::Io {
        context: format!("read design-system registry {}", path.display()),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| ReferenceScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("registry JSON is invalid: {err}"),
        })?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ReferenceScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry JSON must contain a components array".to_owned(),
        })?;

    let mut canonical_symbols = Vec::new();
    let mut resolve_targets = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ReferenceScanError::RegistryInvalid {
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
                        .ok_or_else(|| ReferenceScanError::RegistryInvalid {
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
        return Err(ReferenceScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry must declare at least one component symbol".to_owned(),
        });
    }

    canonical_symbols.sort();
    Ok(RegistryIndex {
        canonical_symbols,
        resolve_targets,
    })
}

fn collect_kotlin_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ReferenceScanError> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|source| ReferenceScanError::Io {
        context: format!("read Kotlin root {}", dir.display()),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ReferenceScanError::Io {
            context: format!("read Kotlin root entry {}", dir.display()),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_kotlin_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "kt") {
            files.push(path);
        }
    }
    Ok(())
}

fn extract_local_components(source: &str, file: &str, out: &mut Vec<LocalComponent>) {
    let lines: Vec<&str> = source.lines().collect();
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let fun_body = if let Some(body) = trimmed.strip_prefix("fun ") {
            body
        } else if let Some(pos) = trimmed.find("fun ") {
            trimmed[pos + 4..].trim_start()
        } else {
            continue;
        };

        let has_composable = line.contains("@Composable")
            || line_index > 0 && lines[line_index - 1].contains("@Composable");
        if !has_composable {
            continue;
        }

        let symbol = fun_body
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .next()
            .unwrap_or_default();
        if symbol.is_empty() || !symbol.starts_with(|ch: char| ch.is_ascii_uppercase()) {
            continue;
        }
        let line = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
        out.push(LocalComponent {
            id: format!("local.{file}:{line}:{symbol}"),
            symbol: symbol.to_owned(),
            location: SourceLocation {
                file: file.to_owned(),
                line,
                column: None,
            },
        });
    }
}

fn extract_usage_sites(
    source: &str,
    file: &str,
    resolve_targets: &BTreeMap<String, String>,
    out: &mut Vec<UsageSite>,
) {
    for (line_index, line) in source.lines().enumerate() {
        let Some(scannable) = scannable_line(line) else {
            continue;
        };
        let scannable = strip_string_literals(scannable);
        for (call_symbol, registry_symbol) in resolve_targets {
            let pattern = format!("{call_symbol}(");
            let mut search_from = 0;
            while let Some(offset) = scannable[search_from..].find(&pattern) {
                let start = search_from + offset;
                if !is_symbol_boundary(&scannable, start) {
                    search_from = start + pattern.len();
                    continue;
                }
                let line = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
                let column = u32::try_from(start + 1).unwrap_or(u32::MAX);
                out.push(UsageSite {
                    id: format!("usage.{file}:{line}:{column}:{call_symbol}"),
                    location: SourceLocation {
                        file: file.to_owned(),
                        line,
                        column: Some(column),
                    },
                    symbol: call_symbol.clone(),
                    match_status: MatchStatus::Resolved,
                    registry_symbol: Some(registry_symbol.clone()),
                });
                search_from = start + pattern.len();
            }
        }
    }
}

fn scannable_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") {
        return None;
    }
    let code = trimmed.split("//").next().unwrap_or(trimmed).trim();
    if code.is_empty() {
        return None;
    }
    Some(code)
}

fn strip_string_literals(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut in_string = false;
    for ch in line.chars() {
        if ch == '"' {
            in_string = !in_string;
            result.push(' ');
            continue;
        }
        if in_string {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

fn is_symbol_boundary(line: &str, start: usize) -> bool {
    if start == 0 {
        return true;
    }
    let before = line.as_bytes()[start - 1];
    !before.is_ascii_alphanumeric() && before != b'_' && before != b'.'
}

#[cfg(test)]
mod tests {
    use super::*;
    use wax_lang_api::ScanConfig;

    #[test]
    fn alias_resolves_to_canonical_registry_symbol() {
        let mut resolve_targets = BTreeMap::new();
        resolve_targets.insert("PrimaryButton".to_owned(), "PrimaryButton".to_owned());
        resolve_targets.insert("PrimaryBtn".to_owned(), "PrimaryButton".to_owned());

        let mut usage_sites = Vec::new();
        extract_usage_sites(
            "fun Screen() { PrimaryBtn(onClick = {}) }",
            "Screen.kt",
            &resolve_targets,
            &mut usage_sites,
        );
        assert_eq!(usage_sites.len(), 1);
        assert_eq!(usage_sites[0].symbol, "PrimaryBtn");
        assert_eq!(
            usage_sites[0].registry_symbol.as_deref(),
            Some("PrimaryButton")
        );
    }

    #[test]
    fn scannable_line_ignores_line_comments() {
        assert_eq!(scannable_line("// PrimaryButton( must not count"), None);
        assert_eq!(
            scannable_line("val x = 1 // PrimaryButton( trailing"),
            Some("val x = 1")
        );
    }

    #[test]
    fn strip_string_literals_removes_quoted_call_patterns() {
        let stripped = strip_string_literals(r#"val ignored = "TextField(not a call)""#);
        assert!(!stripped.contains("TextField("));
    }

    #[test]
    fn parse_rejects_partial_compose_config() {
        let mut config = ScanConfig::new();
        config.insert("roots".to_owned(), serde_json::json!(["src"]));
        let err = parse_compose_scan_config(&config).expect_err("missing registry must fail");
        assert!(matches!(err, ReferenceScanError::ConfigInvalid { .. }));
    }
}
