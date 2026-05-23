//! Reference Kotlin scanner used by the small-fixture correctness gate.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, LocalComponent, MatchStatus, ScanStatus,
    SourceLocation, UsageSite,
};
use wax_lang_api::ScanConfig;

/// Parsed compose scan configuration from the engine request payload.
#[derive(Debug, Clone)]
pub struct ComposeScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Kotlin source roots to scan.
    pub roots: Vec<PathBuf>,
}

/// Loads compose scan settings when the engine forwarded registry and roots.
pub fn scan_config_from_request(config: &ScanConfig) -> Option<ComposeScanConfig> {
    let registry = config.get("design_system_registry")?.as_str()?;
    let roots_value = config.get("roots")?;
    let roots_array = roots_value.as_array()?;
    let mut roots = Vec::with_capacity(roots_array.len());
    for entry in roots_array {
        let root = entry.as_str()?;
        roots.push(PathBuf::from(root));
    }
    if roots.is_empty() {
        return None;
    }

    Some(ComposeScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots,
    })
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
        .symbols
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
        extract_usage_sites(&source, &relative_file, &registry.symbols, &mut usage_sites);
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
            message: "Compose extraction uses the reference Kotlin scanner for correctness gates; tree-sitter integration is pending.".to_owned(),
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
            Self::RegistryInvalid { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

struct RegistrySymbols {
    symbols: BTreeSet<String>,
}

fn load_registry(path: &Path) -> Result<RegistrySymbols, ReferenceScanError> {
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

    let mut symbols = BTreeSet::new();
    for (index, component) in components.iter().enumerate() {
        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ReferenceScanError::RegistryInvalid {
                path: path.to_path_buf(),
                reason: format!("components[{index}] is missing symbol"),
            })?;
        symbols.insert(symbol.to_owned());
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
                symbols.insert(alias_symbol.to_owned());
            }
        }
    }

    if symbols.is_empty() {
        return Err(ReferenceScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry must declare at least one component symbol".to_owned(),
        });
    }

    Ok(RegistrySymbols { symbols })
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
    let needle = "@Composable";
    for (line_index, line) in source.lines().enumerate() {
        let Some(pos) = line.find(needle) else {
            continue;
        };
        let after_composable = &line[pos + needle.len()..];
        let Some(fun_pos) = after_composable.find("fun ") else {
            continue;
        };
        let after_fun = after_composable[fun_pos + 4..].trim_start();
        let symbol = after_fun
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
    registry_symbols: &BTreeSet<String>,
    out: &mut Vec<UsageSite>,
) {
    for (line_index, line) in source.lines().enumerate() {
        for symbol in registry_symbols {
            let pattern = format!("{symbol}(");
            let mut search_from = 0;
            while let Some(offset) = line[search_from..].find(&pattern) {
                let start = search_from + offset;
                if !is_symbol_boundary(line, start) {
                    search_from = start + pattern.len();
                    continue;
                }
                let line = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
                let column = u32::try_from(start + 1).unwrap_or(u32::MAX);
                out.push(UsageSite {
                    id: format!("usage.{file}:{line}:{column}:{symbol}"),
                    location: SourceLocation {
                        file: file.to_owned(),
                        line,
                        column: Some(column),
                    },
                    symbol: symbol.clone(),
                    match_status: MatchStatus::Resolved,
                    registry_symbol: Some(symbol.clone()),
                });
                search_from = start + pattern.len();
            }
        }
    }
}

fn is_symbol_boundary(line: &str, start: usize) -> bool {
    if start == 0 {
        return true;
    }
    let before = line.as_bytes()[start - 1];
    !before.is_ascii_alphanumeric() && before != b'_' && before != b'.'
}
