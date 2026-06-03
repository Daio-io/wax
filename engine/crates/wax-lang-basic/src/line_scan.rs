//! Language-agnostic text line scanner for registry symbol matching.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;
use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, MatchStatus, ScanStatus, SourceLocation,
    UsageSite,
};
use wax_lang_api::{RootPatternKind, RootResolutionError, ScanConfig, resolve_source_roots};

const BASIC_TEXT_SCAN_DIAGNOSTIC: &str = "Basic text line scanner produced heuristic usage facts; parser-backed extraction is recommended for production. Heuristics strip // comments before matching (code after // inside strings or URLs may be missed).";

/// Parsed basic scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative source roots to scan.
    pub roots: Vec<PathBuf>,
    /// Optional file extensions to include (with or without a leading dot).
    pub file_extensions: Vec<String>,
    /// Optional filename patterns to include.
    ///
    /// Only `*suffix` wildcard patterns are supported (for example `*.src`).
    /// Full glob syntax such as `src/**/*.kt` or `*.{kt,kts}` is not supported.
    pub include_globs: Vec<String>,
}

/// Whether the request should run the line scanner or return scaffold facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BasicConfigMode {
    /// No basic scan keys were provided.
    Scaffold,
    /// Registry and roots were provided and validated.
    Configured(BasicScanConfig),
}

/// Loads basic scan settings from the engine request payload.
pub fn parse_basic_scan_config(config: &ScanConfig) -> Result<BasicConfigMode, LineScanError> {
    let has_registry =
        config.contains_key("registry") || config.contains_key("design_system_registry");
    let has_roots = config.contains_key("roots");
    let has_extensions = config.contains_key("file_extensions");
    let has_globs = config.contains_key("include_globs");
    if !has_registry && !has_roots && !has_extensions && !has_globs {
        return Ok(BasicConfigMode::Scaffold);
    }

    let registry = config
        .get("registry")
        .or_else(|| config.get("design_system_registry"))
        .ok_or_else(|| LineScanError::ConfigInvalid {
            reason: "registry is required when basic scan config is present".to_owned(),
        })?;
    let registry = registry
        .as_str()
        .ok_or_else(|| LineScanError::ConfigInvalid {
            reason: "registry must be a non-empty string".to_owned(),
        })?;
    if registry.is_empty() {
        return Err(LineScanError::ConfigInvalid {
            reason: "registry must be a non-empty string".to_owned(),
        });
    }
    validate_repo_relative_path(registry, "registry")?;

    let roots_value = config
        .get("roots")
        .ok_or_else(|| LineScanError::ConfigInvalid {
            reason: "roots is required when basic scan config is present".to_owned(),
        })?;
    let roots_array = roots_value
        .as_array()
        .ok_or_else(|| LineScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        })?;
    if roots_array.is_empty() {
        return Err(LineScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        });
    }

    let mut roots = Vec::with_capacity(roots_array.len());
    for (index, entry) in roots_array.iter().enumerate() {
        let root = entry.as_str().ok_or_else(|| LineScanError::ConfigInvalid {
            reason: format!("roots[{index}] must be a non-empty string"),
        })?;
        if root.is_empty() {
            return Err(LineScanError::ConfigInvalid {
                reason: format!("roots[{index}] must be a non-empty string"),
            });
        }
        validate_repo_relative_path(root, &format!("roots[{index}]"))?;
        roots.push(PathBuf::from(root));
    }

    let file_extensions = parse_string_array(config, "file_extensions")?;
    let include_globs = parse_string_array(config, "include_globs")?;

    Ok(BasicConfigMode::Configured(BasicScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots,
        file_extensions,
        include_globs,
    }))
}

fn parse_string_array(config: &ScanConfig, key: &str) -> Result<Vec<String>, LineScanError> {
    let Some(value) = config.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| LineScanError::ConfigInvalid {
            reason: format!("{key} must be an array of strings"),
        })?;
    let mut entries = Vec::with_capacity(array.len());
    for (index, entry) in array.iter().enumerate() {
        let text = entry.as_str().ok_or_else(|| LineScanError::ConfigInvalid {
            reason: format!("{key}[{index}] must be a non-empty string"),
        })?;
        if text.is_empty() {
            return Err(LineScanError::ConfigInvalid {
                reason: format!("{key}[{index}] must be a non-empty string"),
            });
        }
        entries.push(text.to_owned());
    }
    Ok(entries)
}

fn validate_repo_relative_path(path: &str, field: &str) -> Result<(), LineScanError> {
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(LineScanError::ConfigInvalid {
            reason: format!("{field} must be a repo-relative path"),
        });
    }
    if parsed
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(LineScanError::ConfigInvalid {
            reason: format!("{field} must not contain parent directory segments"),
        });
    }
    Ok(())
}

/// Runs the text line scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &BasicScanConfig,
) -> Result<LineScanResult, LineScanError> {
    let registry_path = repo_root.join(&config.design_system_registry);
    let registry = load_registry(&registry_path)?;
    let mut source_files = Vec::new();
    let mut diagnostics = vec![Diagnostic {
        severity: DiagnosticSeverity::Info,
        code: "basic_text_scan".to_owned(),
        message: BASIC_TEXT_SCAN_DIAGNOSTIC.to_owned(),
        location: None,
    }];
    for root in &config.roots {
        let resolved = resolve_source_roots(repo_root, root).map_err(map_root_resolution_error)?;
        if resolved.roots.is_empty() {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: root_not_found_code(resolved.kind),
                message: root_not_found_message(root, resolved.kind),
                location: None,
            });
        }
        for source_root in resolved.roots {
            collect_source_files(
                &source_root,
                &config.file_extensions,
                &config.include_globs,
                &mut source_files,
            )?;
        }
    }
    source_files.sort();

    let mut design_system_components = registry
        .canonical_symbols
        .iter()
        .map(|symbol| DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.clone(),
            registry_symbol: symbol.clone(),
        })
        .collect::<Vec<_>>();

    let mut usage_sites = Vec::new();
    let mut files_scanned = 0_u32;

    for file_path in source_files {
        files_scanned += 1;
        let source = fs::read_to_string(&file_path).map_err(|source| LineScanError::Io {
            context: format!("read source file {}", file_path.display()),
            source,
        })?;
        let relative_file = file_path
            .strip_prefix(repo_root)
            .unwrap_or(&file_path)
            .display()
            .to_string();

        extract_usage_sites(
            &source,
            &relative_file,
            &registry.resolve_targets,
            &mut usage_sites,
        );
    }

    design_system_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    usage_sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.symbol.cmp(&right.symbol))
    });

    Ok(LineScanResult {
        design_system_components,
        local_components: Vec::new(),
        usage_sites,
        files_scanned,
        diagnostics,
        status: ScanStatus::Partial,
    })
}

fn map_root_resolution_error(err: RootResolutionError) -> LineScanError {
    match err {
        RootResolutionError::Io { context, source } => LineScanError::Io { context, source },
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
            "configured root '{}' does not exist under repo root; no files scanned from it",
            root.display()
        ),
        RootPatternKind::Wildcard => format!(
            "configured root pattern '{}' matched no directories under repo root; no files scanned from it",
            root.display()
        ),
    }
}

/// Output of the text line scanner before contract validation.
#[derive(Debug)]
pub struct LineScanResult {
    /// Known design-system components from the registry file.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Local components are not extracted by the generic text scanner.
    pub local_components: Vec<wax_contract::LocalComponent>,
    /// Usage sites matched against the registry.
    pub usage_sites: Vec<UsageSite>,
    /// Number of source files scanned.
    pub files_scanned: u32,
    /// Diagnostics emitted by the line scan.
    pub diagnostics: Vec<Diagnostic>,
    /// Overall scan status for the text scanner path.
    pub status: ScanStatus,
}

/// Errors produced while running the text line scanner.
#[derive(Debug, Error)]
pub enum LineScanError {
    /// Scan config payload was present but invalid.
    #[error("invalid basic scan config: {reason}")]
    ConfigInvalid {
        /// Human-readable validation failure.
        reason: String,
    },
    /// Registry JSON could not be read or parsed.
    #[error("invalid design-system registry at {path}: {reason}")]
    RegistryInvalid {
        /// Registry path that failed.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },
    /// A filesystem operation failed.
    #[error("{context}: {source}")]
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

struct RegistryIndex {
    canonical_symbols: Vec<String>,
    resolve_targets: BTreeMap<String, String>,
}

fn load_registry(path: &Path) -> Result<RegistryIndex, LineScanError> {
    let raw = fs::read_to_string(path).map_err(|source| LineScanError::Io {
        context: format!("read design-system registry {}", path.display()),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| LineScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("registry JSON is invalid: {err}"),
        })?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| LineScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry JSON must contain a components array".to_owned(),
        })?;

    let mut canonical_symbols = Vec::new();
    let mut resolve_targets = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| LineScanError::RegistryInvalid {
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
                        .ok_or_else(|| LineScanError::RegistryInvalid {
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
        return Err(LineScanError::RegistryInvalid {
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

fn collect_source_files(
    dir: &Path,
    file_extensions: &[String],
    include_globs: &[String],
    files: &mut Vec<PathBuf>,
) -> Result<(), LineScanError> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|source| LineScanError::Io {
        context: format!("read source root {}", dir.display()),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LineScanError::Io {
            context: format!("read source root entry {}", dir.display()),
            source,
        })?;
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)
            .map_err(|source| LineScanError::Io {
                context: format!("read metadata for {}", path.display()),
                source,
            })?
            .file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_source_files(&path, file_extensions, include_globs, files)?;
        } else if should_include_file(&path, file_extensions, include_globs) {
            files.push(path);
        }
    }
    Ok(())
}

fn should_include_file(path: &Path, file_extensions: &[String], include_globs: &[String]) -> bool {
    if file_extensions.is_empty() && include_globs.is_empty() {
        return path.is_file();
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if !file_extensions.is_empty()
        && file_extensions
            .iter()
            .any(|extension| extension_matches(path, extension))
    {
        return true;
    }

    if !include_globs.is_empty()
        && include_globs
            .iter()
            .any(|pattern| glob_matches(file_name, pattern))
    {
        return true;
    }

    false
}

fn extension_matches(path: &Path, extension: &str) -> bool {
    let normalized = extension.strip_prefix('.').unwrap_or(extension);
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(normalized))
}

fn glob_matches(file_name: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*') {
        return file_name.ends_with(suffix);
    }
    file_name == pattern
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
        let stripped = strip_string_literals(scannable.code);
        for (call_symbol, registry_symbol) in resolve_targets {
            let pattern = format!("{call_symbol}(");
            let mut search_from = 0;
            while let Some(offset) = stripped[search_from..].find(&pattern) {
                let start = search_from + offset;
                if !is_symbol_boundary(&stripped, start) {
                    search_from = start + pattern.len();
                    continue;
                }
                let line = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
                let column = u32::try_from(scannable.start_column + start + 1).unwrap_or(u32::MAX);
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

struct ScannableLine<'a> {
    code: &'a str,
    start_column: usize,
}

fn scannable_line(line: &str) -> Option<ScannableLine<'_>> {
    let start_column = line
        .char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))?;
    let trimmed = &line[start_column..];
    if trimmed.starts_with("//") {
        return None;
    }
    let code = trimmed.split("//").next().unwrap_or(trimmed).trim();
    if code.is_empty() {
        return None;
    }
    Some(ScannableLine { code, start_column })
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
            "function Screen() { PrimaryBtn() }",
            "Screen.src",
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
        assert!(scannable_line("// PrimaryButton( must not count").is_none());
        let scannable = scannable_line("    var x = 1 // PrimaryButton( trailing")
            .expect("code before trailing comment should be scannable");
        assert_eq!(scannable.code, "var x = 1");
        assert_eq!(scannable.start_column, 4);
    }

    #[test]
    fn strip_string_literals_removes_quoted_call_patterns() {
        let stripped = strip_string_literals(r#"var ignored = "TextField(not a call)""#);
        assert!(!stripped.contains("TextField("));
    }

    #[test]
    fn parse_rejects_partial_basic_config() {
        let mut config = ScanConfig::new();
        config.insert("roots".to_owned(), serde_json::json!(["src"]));
        let err = parse_basic_scan_config(&config).expect_err("missing registry must fail");
        assert!(matches!(err, LineScanError::ConfigInvalid { .. }));
    }

    #[test]
    fn extension_filter_matches_with_or_without_dot() {
        let path = Path::new("Sample.src");
        assert!(extension_matches(path, "src"));
        assert!(extension_matches(path, ".src"));
    }

    #[test]
    fn glob_filter_matches_star_suffix_pattern() {
        assert!(glob_matches("Sample.src", "*.src"));
        assert!(!glob_matches("Sample.txt", "*.src"));
        assert!(
            !glob_matches("src/main/Sample.kt", "src/**/*.kt"),
            "only *suffix patterns are supported"
        );
    }

    #[test]
    fn validate_repo_relative_path_rejects_absolute_paths() {
        let err = validate_repo_relative_path("/etc/passwd", "registry")
            .expect_err("absolute path must fail");
        assert!(matches!(err, LineScanError::ConfigInvalid { .. }));
    }

    #[test]
    fn validate_repo_relative_path_rejects_parent_dir_segments() {
        let err = validate_repo_relative_path("../outside", "roots[0]")
            .expect_err("parent dir must fail");
        assert!(matches!(err, LineScanError::ConfigInvalid { .. }));
    }

    #[test]
    fn wildcard_root_scans_each_matching_module() {
        let repo_root = temp_repo("basic-wildcard-root");
        let registry_dir = repo_root.join("design-system");
        fs::create_dir_all(&registry_dir).unwrap();
        fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        for module in ["app", "feature-profile"] {
            let source_dir = repo_root.join(module).join("src/main/kotlin");
            fs::create_dir_all(&source_dir).unwrap();
            fs::write(
                source_dir.join("Screen.kt"),
                "fun Screen() {\n    PrimaryButton()\n}\n",
            )
            .unwrap();
        }

        let config = BasicScanConfig {
            design_system_registry: PathBuf::from("design-system/registry.json"),
            roots: vec![PathBuf::from("*/src/main/kotlin")],
            file_extensions: vec!["kt".to_owned()],
            include_globs: Vec::new(),
        };

        let result =
            scan_repository(&repo_root, &config).expect("wildcard roots should scan modules");

        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.usage_sites.len(), 2);

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn recursive_wildcard_root_scans_nested_modules() {
        let repo_root = temp_repo("basic-recursive-wildcard-root");
        let registry_dir = repo_root.join("design-system");
        fs::create_dir_all(&registry_dir).unwrap();
        fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        for module in ["shared/feature", "design-system"] {
            let source_dir = repo_root
                .join("capsule")
                .join(module)
                .join("src/main/kotlin");
            fs::create_dir_all(&source_dir).unwrap();
            fs::write(
                source_dir.join("Screen.kt"),
                "fun Screen() {\n    PrimaryButton()\n}\n",
            )
            .unwrap();
        }

        let excluded_dir = repo_root.join("other/shared/feature/src/main/kotlin");
        fs::create_dir_all(&excluded_dir).unwrap();
        fs::write(
            excluded_dir.join("Screen.kt"),
            "fun Screen() {\n    PrimaryButton()\n}\n",
        )
        .unwrap();

        let config = BasicScanConfig {
            design_system_registry: PathBuf::from("design-system/registry.json"),
            roots: vec![PathBuf::from("capsule/**/src/main/kotlin")],
            file_extensions: vec!["kt".to_owned()],
            include_globs: Vec::new(),
        };

        let result =
            scan_repository(&repo_root, &config).expect("recursive wildcard should scan modules");

        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.usage_sites.len(), 2);

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn missing_literal_root_emits_warning_diagnostic() {
        let repo_root = temp_repo("basic-missing-literal-root");
        let registry_dir = repo_root.join("design-system");
        fs::create_dir_all(&registry_dir).unwrap();
        fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        let config = BasicScanConfig {
            design_system_registry: PathBuf::from("design-system/registry.json"),
            roots: vec![PathBuf::from("app/src/main/kotlin")],
            file_extensions: vec!["kt".to_owned()],
            include_globs: Vec::new(),
        };

        let result = scan_repository(&repo_root, &config)
            .expect("missing literal root should warn without failing");

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "root_not_found"),
            "expected root_not_found diagnostic"
        );
        assert_eq!(result.files_scanned, 0);

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn unmatched_wildcard_root_emits_glob_warning() {
        let repo_root = temp_repo("basic-unmatched-wildcard-root");
        let registry_dir = repo_root.join("design-system");
        fs::create_dir_all(&registry_dir).unwrap();
        fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        let config = BasicScanConfig {
            design_system_registry: PathBuf::from("design-system/registry.json"),
            roots: vec![PathBuf::from("*/src/main/kotlin")],
            file_extensions: vec!["kt".to_owned()],
            include_globs: Vec::new(),
        };

        let result =
            scan_repository(&repo_root, &config).expect("unmatched wildcard should not fail");

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "root_glob_not_found"),
            "expected root_glob_not_found diagnostic"
        );
        assert_eq!(result.files_scanned, 0);

        fs::remove_dir_all(repo_root).unwrap();
    }

    fn temp_repo(name: &str) -> PathBuf {
        let unique = format!(
            "wax-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
