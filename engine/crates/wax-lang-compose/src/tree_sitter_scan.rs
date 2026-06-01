//! Tree-sitter-kotlin backed Compose scanner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Grammar version bundled via the `tree-sitter-kotlin` crate dependency.
/// Update this constant when bumping the crate in `Cargo.toml`.
pub const TREE_SITTER_KOTLIN_GRAMMAR_VERSION: &str = "0.3.8";

use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, LocalComponent, MatchStatus, ScanStatus,
    SourceLocation, UsageSite,
};
use wax_lang_api::{RootPatternKind, RootResolutionError, ScanConfig, resolve_source_roots};

// ── Config ────────────────────────────────────────────────────────────────────

/// Parsed compose scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Kotlin source roots to scan.
    pub roots: Vec<PathBuf>,
}

/// Whether the request should run the tree-sitter scanner or return scaffold facts.
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
) -> Result<ComposeConfigMode, TreeSitterScanError> {
    let has_registry = config.contains_key("design_system_registry");
    let has_roots = config.contains_key("roots");
    if !has_registry && !has_roots {
        return Ok(ComposeConfigMode::Scaffold);
    }

    let registry =
        config
            .get("design_system_registry")
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: "design_system_registry is required when compose scan config is present"
                    .to_owned(),
            })?;
    let registry = registry
        .as_str()
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "design_system_registry must be a non-empty string".to_owned(),
        })?;
    if registry.is_empty() {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: "design_system_registry must be a non-empty string".to_owned(),
        });
    }

    let roots_value = config
        .get("roots")
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "roots is required when compose scan config is present".to_owned(),
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
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: format!("roots[{index}] must be a non-empty string"),
            })?;
        if root.is_empty() {
            return Err(TreeSitterScanError::ConfigInvalid {
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

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors produced by the tree-sitter Compose scanner.
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
            Self::ConfigInvalid { reason } => write!(f, "invalid compose scan config: {reason}"),
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

// ── Result ────────────────────────────────────────────────────────────────────

/// Output of the tree-sitter scanner before contract validation.
#[derive(Debug)]
pub struct TreeSitterScanResult {
    /// Known design-system components from the registry file.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Local `@Composable` declarations discovered in Kotlin sources.
    pub local_components: Vec<LocalComponent>,
    /// Usage sites matched against the registry.
    pub usage_sites: Vec<UsageSite>,
    /// Number of Kotlin files scanned.
    pub files_scanned: u32,
    /// Diagnostics emitted during the scan.
    pub diagnostics: Vec<Diagnostic>,
    /// Overall scan status.
    pub status: ScanStatus,
}

// ── Registry ──────────────────────────────────────────────────────────────────

struct RegistryIndex {
    canonical_symbols: Vec<String>,
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
            reason: "registry must declare at least one component symbol".to_owned(),
        });
    }

    canonical_symbols.sort();
    Ok(RegistryIndex {
        canonical_symbols,
        resolve_targets,
    })
}

// ── File collection ───────────────────────────────────────────────────────────

fn collect_kotlin_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), TreeSitterScanError> {
    let entries = fs::read_dir(dir).map_err(|source| TreeSitterScanError::Io {
        context: format!("read Kotlin root {}", dir.display()),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| TreeSitterScanError::Io {
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

// ── AST helpers ───────────────────────────────────────────────────────────────

/// Returns the annotation type name (e.g. "Composable") from an `annotation` node.
fn annotation_type_name(annotation: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = annotation.walk();
    for child in annotation.named_children(&mut cursor) {
        if child.kind() == "user_type" {
            let mut ut_cursor = child.walk();
            for ut_child in child.named_children(&mut ut_cursor) {
                if ut_child.kind() == "type_identifier" {
                    return ut_child.utf8_text(source).ok().map(|s| s.to_owned());
                }
            }
        }
    }
    None
}

/// Returns true if `function_declaration` node has a `@Composable` annotation in its modifiers.
fn has_composable_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut mod_cursor = child.walk();
            for modifier in child.named_children(&mut mod_cursor) {
                if modifier.kind() == "annotation"
                    && annotation_type_name(modifier, source).as_deref() == Some("Composable")
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Returns the (name, start_position) of the function name from a `function_declaration` node.
fn function_name_from_decl(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "simple_identifier" {
            let name = child.utf8_text(source).ok()?.to_owned();
            return Some((name, child.start_position()));
        }
    }
    None
}

/// If the first named child of a `call_expression` is a `simple_identifier`, returns
/// (callee_name, start_position). Returns `None` for navigation/qualified calls.
fn call_simple_callee(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    let mut cursor = node.walk();
    let first = node.named_children(&mut cursor).next()?;
    if first.kind() == "simple_identifier" {
        let name = first.utf8_text(source).ok()?.to_owned();
        Some((name, first.start_position()))
    } else {
        None
    }
}

// ── Extraction ────────────────────────────────────────────────────────────────

fn extract_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    resolve_targets: &BTreeMap<String, String>,
    local_components: &mut Vec<LocalComponent>,
    usage_sites: &mut Vec<UsageSite>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();

        if kind == "function_declaration"
            && has_composable_annotation(node, source)
            && let Some((name, pos)) = function_name_from_decl(node, source)
            && name.starts_with(|c: char| c.is_ascii_uppercase())
        {
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            local_components.push(LocalComponent {
                id: format!("local.{file}:{line}:{name}"),
                symbol: name,
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
            });
        }

        if kind == "call_expression"
            && let Some((call_symbol, pos)) = call_simple_callee(node, source)
            && let Some(registry_symbol) = resolve_targets.get(&call_symbol)
        {
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            usage_sites.push(UsageSite {
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
        }

        let child_count = node.child_count();
        for i in (0..child_count).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}

// ── Public scan entry point ───────────────────────────────────────────────────

/// Runs the tree-sitter Compose scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &ComposeScanConfig,
) -> Result<TreeSitterScanResult, TreeSitterScanError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin::language())
        .map_err(|e| TreeSitterScanError::ParserInitFailed {
            reason: format!("{e}"),
        })?;

    let registry_path = repo_root.join(&config.design_system_registry);
    let registry = load_registry(&registry_path)?;

    let mut kotlin_files = Vec::new();
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
                collect_kotlin_files(&abs_root, &mut kotlin_files)?;
            }
        }
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
    let mut parse_failures = 0_u32;

    for file_path in &kotlin_files {
        files_scanned += 1;
        let source = fs::read_to_string(file_path).map_err(|source| TreeSitterScanError::Io {
            context: format!("read Kotlin source {}", file_path.display()),
            source,
        })?;
        let relative_file = file_path
            .strip_prefix(repo_root)
            .unwrap_or(file_path)
            .display()
            .to_string();

        match parser.parse(source.as_bytes(), None) {
            Some(tree) => {
                extract_from_source(
                    tree.root_node(),
                    source.as_bytes(),
                    &relative_file,
                    &registry.resolve_targets,
                    &mut local_components,
                    &mut usage_sites,
                );
            }
            None => {
                parse_failures += 1;
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "parse_failed".to_owned(),
                    message: format!("tree-sitter failed to parse {relative_file}; file skipped"),
                    location: None,
                });
            }
        }
    }

    design_system_components.sort_by(|l, r| l.symbol.cmp(&r.symbol));
    local_components.sort_by(|l, r| l.symbol.cmp(&r.symbol));
    usage_sites.sort_by(|l, r| {
        l.location
            .file
            .cmp(&r.location.file)
            .then(l.location.line.cmp(&r.location.line))
            .then(l.symbol.cmp(&r.symbol))
    });

    // Report Partial when any file was skipped (parse failure) or any root was missing,
    // so downstream adoption metrics are not treated as complete.
    let has_gaps = parse_failures > 0
        || diagnostics
            .iter()
            .any(|d| d.code == "root_not_found" || d.code == "root_glob_not_found");
    let status = if has_gaps {
        ScanStatus::Partial
    } else {
        ScanStatus::Complete
    };

    Ok(TreeSitterScanResult {
        design_system_components,
        local_components,
        usage_sites,
        files_scanned,
        diagnostics,
        status,
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
            "configured root '{}' does not exist under repo root; no files scanned from it",
            root.display()
        ),
        RootPatternKind::Wildcard => format!(
            "configured root pattern '{}' matched no directories under repo root; no files scanned from it",
            root.display()
        ),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parser() -> tree_sitter::Parser {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_kotlin::language()).unwrap();
        p
    }

    fn resolve_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn parse_and_extract(
        source: &str,
        resolve_targets: &BTreeMap<String, String>,
    ) -> (Vec<LocalComponent>, Vec<UsageSite>) {
        let mut parser = make_parser();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        let mut locals = Vec::new();
        let mut usages = Vec::new();
        extract_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.kt",
            resolve_targets,
            &mut locals,
            &mut usages,
        );
        (locals, usages)
    }

    #[test]
    fn direct_call_to_registry_symbol_is_resolved() {
        let resolve = resolve_map(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Screen() { PrimaryButton(onClick = {}) }",
            &resolve,
        );
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].symbol, "PrimaryButton");
        assert_eq!(usages[0].registry_symbol.as_deref(), Some("PrimaryButton"));
        assert_eq!(usages[0].match_status, MatchStatus::Resolved);
    }

    #[test]
    fn alias_resolves_to_canonical_registry_symbol() {
        let resolve = resolve_map(&[
            ("PrimaryButton", "PrimaryButton"),
            ("PrimaryBtn", "PrimaryButton"),
        ]);
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Screen() { PrimaryBtn(onClick = {}) }",
            &resolve,
        );
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].symbol, "PrimaryBtn");
        assert_eq!(usages[0].registry_symbol.as_deref(), Some("PrimaryButton"));
    }

    #[test]
    fn comment_lines_are_not_extracted() {
        let resolve = resolve_map(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) =
            parse_and_extract("// PrimaryButton( not a call\nfun Screen() {}", &resolve);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn string_literal_content_is_not_extracted() {
        let resolve = resolve_map(&[("TextField", "TextField")]);
        let (_, usages) = parse_and_extract(
            "val label = \"TextField(not a call)\"\nfun Screen() {}",
            &resolve,
        );
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn qualified_call_is_not_extracted() {
        let resolve = resolve_map(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Screen() { com.example.PrimaryButton(onClick = {}) }",
            &resolve,
        );
        // navigation_expression as first child → not counted
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn composable_function_is_detected_as_local() {
        let resolve = BTreeMap::new();
        let (locals, _) = parse_and_extract("@Composable\nfun MyScreen() {}", &resolve);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].symbol, "MyScreen");
    }

    #[test]
    fn non_composable_function_is_not_a_local_component() {
        let resolve = BTreeMap::new();
        let (locals, _) = parse_and_extract("fun helper() {}", &resolve);
        assert_eq!(locals.len(), 0);
    }

    #[test]
    fn lowercase_composable_function_is_not_a_local_component() {
        let resolve = BTreeMap::new();
        let (locals, _) = parse_and_extract("@Composable\nfun myHelper() {}", &resolve);
        assert_eq!(locals.len(), 0);
    }

    #[test]
    fn multiline_call_is_detected_at_first_line() {
        let resolve = resolve_map(&[("PrimaryButton", "PrimaryButton")]);
        let source =
            "@Composable\nfun Screen() {\n    PrimaryButton(\n        onClick = {},\n    )\n}";
        let (_, usages) = parse_and_extract(source, &resolve);
        assert_eq!(usages.len(), 1);
        // Row 2 (0-based) = line 3 (1-based); col 4 (0-based) = col 5 (1-based)
        assert_eq!(usages[0].location.line, 3);
        assert_eq!(usages[0].location.column, Some(5));
    }

    #[test]
    fn annotation_on_previous_line_is_recognised() {
        let resolve = BTreeMap::new();
        let (locals, _) = parse_and_extract("@Composable\nfun CardComponent() {}", &resolve);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].symbol, "CardComponent");
    }

    #[test]
    fn non_ds_composable_call_is_not_a_resolved_usage() {
        let resolve = resolve_map(&[("PrimaryButton", "PrimaryButton")]);
        // LocalCard is not in the registry
        let (_, usages) = parse_and_extract("@Composable\nfun Screen() { LocalCard {} }", &resolve);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn parse_rejects_partial_compose_config() {
        let mut config = ScanConfig::new();
        config.insert("roots".to_owned(), serde_json::json!(["src"]));
        let err = parse_compose_scan_config(&config).expect_err("missing registry must fail");
        assert!(matches!(err, TreeSitterScanError::ConfigInvalid { .. }));
    }

    #[test]
    fn missing_root_emits_warning_diagnostic_and_partial_status() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("does-not-exist/registry.json"),
            roots: vec![std::path::PathBuf::from("no-such-root")],
        };

        // Create a temp dir with just a minimal registry file.
        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("does-not-exist");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Btn"}]}"#,
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should succeed even with missing root");

        let has_root_warning = result
            .diagnostics
            .iter()
            .any(|d| d.code == "root_not_found");
        assert!(has_root_warning, "expected root_not_found diagnostic");
        assert_eq!(
            result.status,
            ScanStatus::Partial,
            "missing root must yield Partial, not Complete"
        );
        assert_eq!(result.files_scanned, 0);
    }

    #[test]
    fn unmatched_wildcard_root_emits_glob_warning() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("*/src/main/kotlin")],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Btn"}]}"#,
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should succeed even when wildcard roots match nothing");

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "root_glob_not_found"),
            "expected root_glob_not_found diagnostic"
        );
        assert_eq!(result.status, ScanStatus::Partial);
        assert_eq!(result.files_scanned, 0);
    }

    #[test]
    fn wildcard_root_scans_each_matching_module() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("*/src/main/kotlin")],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        for module in ["app", "feature-profile"] {
            let source_dir = tmp.path().join(module).join("src/main/kotlin");
            std::fs::create_dir_all(&source_dir).unwrap();
            std::fs::write(
                source_dir.join("Screen.kt"),
                "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\n",
            )
            .unwrap();
        }

        let result = scan_repository(tmp.path(), &config)
            .expect("wildcard roots should scan matching modules");

        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.usage_sites.len(), 2);
        assert_eq!(result.status, ScanStatus::Complete);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "root_not_found"),
            "matching wildcard roots must not emit root_not_found diagnostics"
        );
    }
}
