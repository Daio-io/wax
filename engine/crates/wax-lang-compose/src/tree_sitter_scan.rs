//! Tree-sitter-kotlin backed Compose scanner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::kotlin_ast::{
    ImportBindings, ParseKotlinFileError, call_simple_callee, collect_import_bindings,
    collect_kotlin_files, function_name_from_decl, has_composable_annotation,
    has_preview_annotation, is_non_ui_scaffolding_composable_symbol,
    is_pascal_case_composable_symbol, is_within_preview_composable, nearest_enclosing_composable,
    new_parser, package_name_from_source, parse_kotlin_file_permissive,
    partial_tree_parse_diagnostic, unparseable_file_diagnostic,
};

/// Grammar version bundled via the `tree-sitter-kotlin-ng` crate dependency.
/// Update this constant when bumping the crate in `Cargo.toml`.
pub const TREE_SITTER_KOTLIN_GRAMMAR_VERSION: &str = "1.1.0";

use wax_contract::{
    DesignSystemComponent, DesignSystemToken, Diagnostic, DiagnosticSeverity, HardcodedStyleSite,
    IdentityStability, LocalComponent, MatchStatus, ParentScope, ScanStatus, SourceLocation,
    StyleContext, TokenCategory, TokenSite, UsageSite,
};
use wax_lang_api::{
    RegistryTokenIndex, RootPatternKind, RootResolutionError, ScanConfig, parse_registry_tokens,
    resolve_import_aware_match, resolve_source_roots, token_index,
};

// ── Config ────────────────────────────────────────────────────────────────────

/// Parsed compose scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Kotlin source roots to scan.
    pub roots: Vec<PathBuf>,
    /// Repo-relative file paths or glob patterns to exclude from scanning.
    pub excludes: Vec<String>,
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
    let has_registry = config.contains_key("registry");
    let has_roots = config.contains_key("roots");
    let has_excludes = config.contains_key("excludes");
    if !has_registry && !has_roots && !has_excludes {
        return Ok(ComposeConfigMode::Scaffold);
    }

    let registry = config
        .get("registry")
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "registry is required when compose scan config is present".to_owned(),
        })?;
    let registry = registry
        .as_str()
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "registry must be a non-empty string".to_owned(),
        })?;
    if registry.is_empty() {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: "registry must be a non-empty string".to_owned(),
        });
    }
    validate_repo_relative_path(registry, "registry")?;

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
        validate_repo_relative_path(root, &format!("roots[{index}]"))?;
        roots.push(PathBuf::from(root));
    }

    let excludes = parse_excludes(config)?;

    Ok(ComposeConfigMode::Configured(ComposeScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots,
        excludes,
    }))
}

fn parse_excludes(config: &ScanConfig) -> Result<Vec<String>, TreeSitterScanError> {
    let Some(excludes_value) = config.get("excludes") else {
        return Ok(Vec::new());
    };
    let excludes_array =
        excludes_value
            .as_array()
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: "excludes must be an array of non-empty strings".to_owned(),
            })?;

    let mut excludes = Vec::with_capacity(excludes_array.len());
    for (index, entry) in excludes_array.iter().enumerate() {
        let exclude = entry
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: format!("excludes[{index}] must be a non-empty string"),
            })?;
        validate_repo_relative_path(exclude, &format!("excludes[{index}]"))?;
        excludes.push(exclude.to_owned());
    }

    Ok(excludes)
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
    /// Known design-system tokens from the registry file.
    pub design_system_tokens: Vec<DesignSystemToken>,
    /// Known token references matched in source.
    pub token_sites: Vec<TokenSite>,
    /// Hard-coded styling candidates discovered in source.
    pub hardcoded_style_sites: Vec<HardcodedStyleSite>,
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
    component_packages: BTreeMap<String, Option<String>>,
    tokens: Vec<DesignSystemToken>,
    token_index: RegistryTokenIndex,
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
    let mut component_packages = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                path: path.to_path_buf(),
                reason: format!("components[{index}] is missing symbol"),
            })?;
        let package = component
            .get("package")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                        path: path.to_path_buf(),
                        reason: format!("components[{index}].package must be a string"),
                    })
            })
            .transpose()?
            .map(str::to_owned);
        if let Some(package) = &package
            && package.is_empty()
        {
            return Err(TreeSitterScanError::RegistryInvalid {
                path: path.to_path_buf(),
                reason: format!("components[{index}].package must not be empty"),
            });
        }

        canonical_symbols.push(symbol.to_owned());
        resolve_targets.insert(symbol.to_owned(), symbol.to_owned());
        component_packages.insert(symbol.to_owned(), package);
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

    let tokens =
        parse_registry_tokens(&value).map_err(|err| TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
    let token_index = token_index(&tokens).map_err(|err| TreeSitterScanError::RegistryInvalid {
        path: path.to_path_buf(),
        reason: err.to_string(),
    })?;

    Ok(RegistryIndex {
        canonical_symbols,
        resolve_targets,
        component_packages,
        tokens,
        token_index,
    })
}

// ── Extraction ────────────────────────────────────────────────────────────────

fn resolve_registry_match(
    call_symbol: &str,
    registry_symbol: &str,
    registry: &RegistryIndex,
    imports: &ImportBindings,
) -> Option<MatchStatus> {
    resolve_import_aware_match(
        registry
            .component_packages
            .get(registry_symbol)
            .and_then(|package| package.as_deref()),
        imports.package_for_symbol(call_symbol).as_deref(),
    )
    .or_else(|| {
        if registry
            .component_packages
            .get(registry_symbol)
            .and_then(|package| package.as_deref())
            .is_none()
        {
            Some(MatchStatus::Resolved)
        } else {
            None
        }
    })
}

fn qualified_composable_symbol(package: Option<&str>, symbol: &str) -> String {
    package
        .map(|pkg| format!("{pkg}.{symbol}"))
        .unwrap_or_else(|| symbol.to_owned())
}

fn local_definition_id(qualified_symbol: &str) -> String {
    format!("local.compose:{qualified_symbol}")
}

fn parent_scope_for_composable(
    file: &str,
    package: Option<&str>,
    composable_name: &str,
    pos: tree_sitter::Point,
) -> ParentScope {
    let qualified_symbol = qualified_composable_symbol(package, composable_name);
    ParentScope {
        parent_id: format!("compose:composable:{qualified_symbol}"),
        symbol: composable_name.to_owned(),
        qualified_symbol: package.map(|_| qualified_symbol),
        scope_kind: "composable".to_owned(),
        identity_basis: "package_qualified_symbol".to_owned(),
        identity_stability: IdentityStability::Semantic,
        location: Some(SourceLocation {
            file: file.to_owned(),
            line: pos.row as u32 + 1,
            column: Some(pos.column as u32 + 1),
        }),
    }
}

#[derive(Debug, Default)]
struct LocalComposableIndex {
    by_file_symbol: BTreeMap<(String, String), LocalComponent>,
    by_qualified: BTreeMap<String, LocalComponent>,
}

impl LocalComposableIndex {
    fn insert(&mut self, file: &str, component: LocalComponent) {
        if let Some(qualified) = &component.qualified_symbol {
            self.by_qualified
                .insert(qualified.clone(), component.clone());
        }
        self.by_file_symbol
            .insert((file.to_owned(), component.symbol.clone()), component);
    }

    fn resolve(&self, file: &str, package: Option<&str>, symbol: &str) -> Option<&LocalComponent> {
        if let Some(component) = self
            .by_file_symbol
            .get(&(file.to_owned(), symbol.to_owned()))
        {
            return Some(component);
        }
        let qualified = qualified_composable_symbol(package, symbol);
        self.by_qualified.get(&qualified)
    }
}

fn index_local_components_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
) -> Vec<LocalComponent> {
    let package = package_name_from_source(root, source);
    let mut local_components = Vec::new();

    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_declaration"
            && has_composable_annotation(node, source)
            && !has_preview_annotation(node, source)
            && !is_within_preview_composable(node, source)
            && let Some((name, pos)) = function_name_from_decl(node, source)
            && is_pascal_case_composable_symbol(&name)
            && !is_non_ui_scaffolding_composable_symbol(&name)
        {
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let qualified_symbol = qualified_composable_symbol(package.as_deref(), &name);
            let component = LocalComponent {
                id: local_definition_id(&qualified_symbol),
                symbol: name,
                qualified_symbol: Some(qualified_symbol),
                identity_basis: Some("package_qualified_symbol".to_owned()),
                identity_stability: Some(IdentityStability::Semantic),
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
            };
            local_components.push(component);
        }

        let child_count = node.child_count();
        for i in (0..child_count).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
    local_components
}

fn extract_usage_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    registry: &RegistryIndex,
    local_index: &LocalComposableIndex,
    usage_sites: &mut Vec<UsageSite>,
) {
    let package = package_name_from_source(root, source);
    let imports = collect_import_bindings(root, source);

    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression"
            && let Some((call_symbol, pos)) = call_simple_callee(node, source)
            && is_pascal_case_composable_symbol(&call_symbol)
        {
            let skip_current = is_within_preview_composable(node, source)
                || is_non_ui_scaffolding_composable_symbol(&call_symbol);
            if !skip_current {
                let line = pos.row as u32 + 1;
                let column = pos.column as u32 + 1;
                let location = SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                };
                let parent =
                    nearest_enclosing_composable(node, source).map(|(name, parent_pos)| {
                        parent_scope_for_composable(file, package.as_deref(), &name, parent_pos)
                    });

                if let Some(registry_symbol) = registry.resolve_targets.get(&call_symbol) {
                    if let Some(match_status) =
                        resolve_registry_match(&call_symbol, registry_symbol, registry, &imports)
                    {
                        usage_sites.push(UsageSite {
                            id: format!("usage.compose:{file}:{line}:{column}:{call_symbol}"),
                            location: location.clone(),
                            symbol: call_symbol.clone(),
                            qualified_symbol: None,
                            match_status,
                            registry_symbol: Some(registry_symbol.clone()),
                            local_definition_id: None,
                            parent,
                        });
                    }
                } else if let Some(local) =
                    local_index.resolve(file, package.as_deref(), &call_symbol)
                {
                    usage_sites.push(UsageSite {
                        id: format!("usage.compose:{file}:{line}:{column}:{call_symbol}"),
                        location: location.clone(),
                        symbol: call_symbol.clone(),
                        qualified_symbol: local.qualified_symbol.clone(),
                        match_status: MatchStatus::Local,
                        registry_symbol: None,
                        local_definition_id: Some(local.id.clone()),
                        parent,
                    });
                } else {
                    usage_sites.push(UsageSite {
                        id: format!("usage.compose:{file}:{line}:{column}:{call_symbol}"),
                        location,
                        symbol: call_symbol,
                        qualified_symbol: None,
                        match_status: MatchStatus::Unresolved,
                        registry_symbol: None,
                        local_definition_id: None,
                        parent,
                    });
                }
            }
        }

        let child_count = node.child_count();
        for i in (0..child_count).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}

fn compose_style_metadata(call: &str) -> Option<(TokenCategory, StyleContext)> {
    match call {
        "Color" | "background" => Some((TokenCategory::Color, StyleContext::Color)),
        "padding" => Some((TokenCategory::Spacing, StyleContext::Padding)),
        "size" => Some((TokenCategory::Spacing, StyleContext::Size)),
        "width" => Some((TokenCategory::Spacing, StyleContext::Width)),
        "height" => Some((TokenCategory::Spacing, StyleContext::Height)),
        "spacedBy" => Some((TokenCategory::Spacing, StyleContext::Gap)),
        "fontSize" | "TextStyle" => Some((TokenCategory::Typography, StyleContext::Typography)),
        "clip" | "cornerRadius" | "RoundedCornerShape" => {
            Some((TokenCategory::Radius, StyleContext::Radius))
        }
        "shadow" | "elevation" => Some((TokenCategory::Elevation, StyleContext::Elevation)),
        _ => None,
    }
}

/// Resolves the callee name for styling-candidate detection.
///
/// Unlike [`call_simple_callee`] (used for composable usage-site attribution, which
/// intentionally ignores qualified calls), styling calls are almost always qualified
/// member calls such as `Modifier.padding(...)` or `.background(...)`. This helper
/// additionally resolves the trailing member name of a navigation-qualified callee.
fn style_call_callee(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    if let Some(found) = call_simple_callee(node, source) {
        return Some(found);
    }
    let mut cursor = node.walk();
    let navigation = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "navigation_expression")?;
    let mut inner_cursor = navigation.walk();
    let member = navigation
        .named_children(&mut inner_cursor)
        .filter(|child| matches!(child.kind(), "simple_identifier" | "identifier"))
        .last()?;
    let name = member.utf8_text(source).ok()?.to_owned();
    Some((name, member.start_position()))
}

/// Picks the first hard-coded style literal among a styling call's *direct* value
/// arguments by inspecting Kotlin AST literal / unit-navigation nodes.
///
/// Scoping to direct value arguments keeps nested style calls — e.g.
/// `background(Color(0xFF336699))` — from being double-counted: an argument that is
/// itself a nested `call_expression` is skipped here so the nested call can be visited
/// (and counted) on its own.
///
/// `.dp` / `.sp` are accepted only when applied to a numeric literal receiver
/// (`8.dp`), not when chained off an identifier (`Spacing.medium.dp`).
fn first_style_literal(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    category: TokenCategory,
) -> Option<String> {
    let arguments = call_value_arguments(node)?;
    let mut cursor = arguments.walk();
    for value_argument in arguments.named_children(&mut cursor) {
        if value_argument_contains_call_expression(value_argument) {
            continue;
        }
        if let Some(found) = find_style_literal_in_argument(value_argument, source, category) {
            return Some(found);
        }
    }
    None
}

fn call_value_arguments(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "value_arguments")
}

fn value_argument_contains_call_expression(value_argument: tree_sitter::Node<'_>) -> bool {
    let mut stack = vec![value_argument];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression" {
            return true;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    false
}

fn find_style_literal_in_argument(
    value_argument: tree_sitter::Node<'_>,
    source: &[u8],
    category: TokenCategory,
) -> Option<String> {
    let mut stack = vec![value_argument];
    while let Some(node) = stack.pop() {
        if let Some(value) = style_literal_from_node(node, source, category) {
            return Some(value);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "call_expression" {
                continue;
            }
            stack.push(child);
        }
    }
    None
}

fn style_literal_from_node(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    category: TokenCategory,
) -> Option<String> {
    match category {
        TokenCategory::Color => {
            if is_numeric_literal_node(node) {
                return Some(node.utf8_text(source).ok()?.to_owned());
            }
            None
        }
        TokenCategory::Typography => {
            if is_numeric_unit_navigation(node, source, "sp") {
                return Some(node.utf8_text(source).ok()?.to_owned());
            }
            if is_bare_numeric_literal(node) {
                return Some(node.utf8_text(source).ok()?.to_owned());
            }
            None
        }
        TokenCategory::Spacing | TokenCategory::Radius | TokenCategory::Elevation => {
            if is_numeric_unit_navigation(node, source, "dp") {
                return Some(node.utf8_text(source).ok()?.to_owned());
            }
            if is_bare_numeric_literal(node) {
                return Some(node.utf8_text(source).ok()?.to_owned());
            }
            None
        }
        TokenCategory::Unknown => None,
    }
}

fn is_numeric_literal_node(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "number_literal" | "float_literal")
}

fn is_bare_numeric_literal(node: tree_sitter::Node<'_>) -> bool {
    if !is_numeric_literal_node(node) {
        return false;
    }
    // Numeric receivers of `8.dp` / `14.sp` are handled by the navigation node itself.
    node.parent()
        .is_none_or(|parent| parent.kind() != "navigation_expression")
}

fn is_numeric_unit_navigation(node: tree_sitter::Node<'_>, source: &[u8], unit: &str) -> bool {
    if node.kind() != "navigation_expression" {
        return false;
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    let [receiver, member] = children.as_slice() else {
        return false;
    };
    is_numeric_literal_node(*receiver)
        && matches!(member.kind(), "simple_identifier" | "identifier")
        && member.utf8_text(source).ok() == Some(unit)
}

fn extract_hardcoded_style_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    out: &mut Vec<HardcodedStyleSite>,
) {
    let package = package_name_from_source(root, source);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression"
            && !is_within_preview_composable(node, source)
            && let Some((call_symbol, pos)) = style_call_callee(node, source)
            && let Some((category, context)) = compose_style_metadata(&call_symbol)
            && let Some(value) = first_style_literal(node, source, category)
        {
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let parent = nearest_enclosing_composable(node, source).map(|(name, parent_pos)| {
                parent_scope_for_composable(file, package.as_deref(), &name, parent_pos)
            });
            out.push(HardcodedStyleSite {
                id: format!("hardcoded.compose:{file}:{line}:{column}:{category:?}"),
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
                value,
                category,
                context,
                parent,
            });
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}

fn extract_token_sites_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    token_index: &RegistryTokenIndex,
    out: &mut Vec<TokenSite>,
) {
    let package = package_name_from_source(root, source);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_token_reference_node(node)
            && !is_within_preview_composable(node, source)
            && let Ok(text) = node.utf8_text(source)
            && let Some(token_match) = token_index.matches.get(text)
        {
            let pos = node.start_position();
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let parent = nearest_enclosing_composable(node, source).map(|(name, parent_pos)| {
                parent_scope_for_composable(file, package.as_deref(), &name, parent_pos)
            });
            out.push(TokenSite {
                id: format!(
                    "token.compose:{file}:{line}:{column}:{}",
                    token_match.token_id
                ),
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
                token_id: token_match.token_id.clone(),
                key: text.to_owned(),
                category: token_match.category,
                parent,
            });
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}

/// Token keys must match expression/reference nodes, not declaration bindings or types.
fn is_token_reference_node(node: tree_sitter::Node<'_>) -> bool {
    match node.kind() {
        "navigation_expression" | "call_expression" => true,
        "identifier" | "simple_identifier" => !is_declaration_or_type_identifier(node),
        _ => false,
    }
}

fn is_declaration_or_type_identifier(node: tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(
        parent.kind(),
        "parameter"
            | "variable_declaration"
            | "function_declaration"
            | "class_declaration"
            | "object_declaration"
            | "type_alias"
            | "user_type"
            | "enum_entry"
    )
}

#[allow(dead_code)]
fn extract_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    registry: &RegistryIndex,
    local_components: &mut Vec<LocalComponent>,
    usage_sites: &mut Vec<UsageSite>,
) {
    let mut local_index = LocalComposableIndex::default();
    for local in index_local_components_from_source(root, source, file) {
        local_index.insert(file, local.clone());
        local_components.push(local);
    }
    extract_usage_from_source(root, source, file, registry, &local_index, usage_sites);
}

// ── Public scan entry point ───────────────────────────────────────────────────

/// Runs the tree-sitter Compose scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &ComposeScanConfig,
) -> Result<TreeSitterScanResult, TreeSitterScanError> {
    let mut parser =
        new_parser().map_err(|reason| TreeSitterScanError::ParserInitFailed { reason })?;

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
                collect_kotlin_files(&abs_root, &mut kotlin_files).map_err(|source| {
                    TreeSitterScanError::Io {
                        context: format!("read Kotlin root {}", abs_root.display()),
                        source,
                    }
                })?;
            }
        }
    }
    kotlin_files.sort();
    kotlin_files.retain(|file_path| {
        let relative_file = file_path.strip_prefix(repo_root).unwrap_or(file_path);
        let relative_text = normalize_repo_relative_path(relative_file);
        !path_matches_any(&relative_text, &config.excludes)
    });

    let mut design_system_components = registry
        .canonical_symbols
        .iter()
        .map(|symbol| DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.clone(),
            registry_symbol: symbol.clone(),
        })
        .collect::<Vec<_>>();

    let design_system_tokens = registry.tokens.clone();
    let mut local_components = Vec::new();
    let mut usage_sites = Vec::new();
    let mut token_sites = Vec::new();
    let mut hardcoded_style_sites = Vec::new();
    let mut files_scanned = 0_u32;
    let mut parse_failures = 0_u32;
    let mut parsed_files = Vec::new();

    for file_path in &kotlin_files {
        files_scanned += 1;
        let relative_file = file_path
            .strip_prefix(repo_root)
            .unwrap_or(file_path)
            .display()
            .to_string();

        match parse_kotlin_file_permissive(&mut parser, file_path) {
            Ok(parsed) => {
                if parsed.is_partial() {
                    parse_failures += 1;
                    diagnostics.extend(
                        parsed
                            .unresolved_problems
                            .iter()
                            .map(|problem| partial_tree_parse_diagnostic(problem, &relative_file)),
                    );
                }
                parsed_files.push((relative_file, parsed));
            }
            Err(ParseKotlinFileError::ParseFailed(_)) => {
                parse_failures += 1;
                diagnostics.push(unparseable_file_diagnostic(&relative_file));
            }
            Err(ParseKotlinFileError::Io { context, source }) => {
                return Err(TreeSitterScanError::Io { context, source });
            }
        }
    }

    let mut local_index = LocalComposableIndex::default();
    for (relative_file, parsed) in &parsed_files {
        for local in index_local_components_from_source(
            parsed.primary_tree().root_node(),
            parsed.source.as_bytes(),
            relative_file,
        ) {
            local_index.insert(relative_file, local.clone());
            local_components.push(local);
        }
    }

    for (relative_file, parsed) in &parsed_files {
        extract_usage_from_source(
            parsed.primary_tree().root_node(),
            parsed.source.as_bytes(),
            relative_file,
            &registry,
            &local_index,
            &mut usage_sites,
        );
        extract_hardcoded_style_from_source(
            parsed.primary_tree().root_node(),
            parsed.source.as_bytes(),
            relative_file,
            &mut hardcoded_style_sites,
        );
        extract_token_sites_from_source(
            parsed.primary_tree().root_node(),
            parsed.source.as_bytes(),
            relative_file,
            &registry.token_index,
            &mut token_sites,
        );
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
    token_sites.sort_by(|l, r| {
        l.location
            .file
            .cmp(&r.location.file)
            .then(l.location.line.cmp(&r.location.line))
            .then(l.location.column.cmp(&r.location.column))
            .then(l.token_id.cmp(&r.token_id))
    });
    hardcoded_style_sites.sort_by(|l, r| {
        l.location
            .file
            .cmp(&r.location.file)
            .then(l.location.line.cmp(&r.location.line))
            .then(l.location.column.cmp(&r.location.column))
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
        design_system_tokens,
        token_sites,
        hardcoded_style_sites,
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

fn normalize_repo_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_matches_any(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| path_matches_glob(path, pattern))
}

fn path_matches_glob(path: &str, pattern: &str) -> bool {
    expand_brace_groups(pattern)
        .iter()
        .any(|expanded| path_matches_glob_no_brace(path, expanded))
}

fn expand_brace_groups(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(end_offset) = pattern[start..].find('}') else {
        return vec![pattern.to_owned()];
    };
    let end = start + end_offset;
    let prefix = &pattern[..start];
    let suffix = &pattern[end + 1..];
    let alternatives = pattern[start + 1..end].split(',');
    let mut expanded = Vec::new();
    for alternative in alternatives {
        expanded.extend(expand_brace_groups(&format!(
            "{prefix}{alternative}{suffix}"
        )));
    }
    expanded
}

fn path_matches_glob_no_brace(path: &str, pattern: &str) -> bool {
    let path_segments = split_path_segments(path);
    let pattern_segments = split_path_segments(pattern);
    segments_match(&path_segments, &pattern_segments)
}

fn split_path_segments(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn segments_match(path_segments: &[&str], pattern_segments: &[&str]) -> bool {
    let mut path_idx = 0;
    let mut pattern_idx = 0;

    while pattern_idx < pattern_segments.len() {
        if pattern_segments[pattern_idx] == "**" {
            if pattern_idx == pattern_segments.len() - 1 {
                return true;
            }
            for skip in 0..=(path_segments.len().saturating_sub(path_idx)) {
                if segments_match(
                    &path_segments[path_idx + skip..],
                    &pattern_segments[pattern_idx + 1..],
                ) {
                    return true;
                }
            }
            return false;
        }

        if path_idx >= path_segments.len()
            || !segment_matches(path_segments[path_idx], pattern_segments[pattern_idx])
        {
            return false;
        }

        path_idx += 1;
        pattern_idx += 1;
    }

    path_idx == path_segments.len()
}

fn segment_matches(segment: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    glob_segment_match(segment.as_bytes(), pattern.as_bytes())
}

fn glob_segment_match(segment: &[u8], pattern: &[u8]) -> bool {
    match (segment.first(), pattern.first()) {
        (None, None) => true,
        (Some(_), None) => false,
        (None, Some(b'*')) => glob_segment_match(segment, &pattern[1..]),
        (None, Some(_)) => false,
        (Some(_), Some(b'*')) => {
            glob_segment_match(&segment[1..], pattern) || glob_segment_match(segment, &pattern[1..])
        }
        (Some(segment_byte), Some(pattern_byte)) if segment_byte == pattern_byte => {
            glob_segment_match(&segment[1..], &pattern[1..])
        }
        _ => false,
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parser() -> tree_sitter::Parser {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
            .unwrap();
        p
    }

    fn resolve_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn registry_index(
        resolve_targets: BTreeMap<String, String>,
        component_packages: BTreeMap<String, Option<String>>,
    ) -> RegistryIndex {
        let canonical_symbols = resolve_targets
            .values()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        RegistryIndex {
            canonical_symbols,
            resolve_targets,
            component_packages,
            tokens: Vec::new(),
            token_index: RegistryTokenIndex::default(),
        }
    }

    fn registry_without_packages(pairs: &[(&str, &str)]) -> RegistryIndex {
        let resolve_targets = resolve_map(pairs);
        let component_packages = resolve_targets
            .values()
            .map(|symbol| (symbol.clone(), None))
            .collect();
        registry_index(resolve_targets, component_packages)
    }

    fn parse_and_extract(
        source: &str,
        registry: &RegistryIndex,
    ) -> (Vec<LocalComponent>, Vec<UsageSite>) {
        let mut parser = make_parser();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        let mut locals = Vec::new();
        let mut usages = Vec::new();
        extract_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.kt",
            registry,
            &mut locals,
            &mut usages,
        );
        (locals, usages)
    }

    #[test]
    fn direct_call_to_registry_symbol_is_resolved() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Screen() { PrimaryButton(onClick = {}) }",
            &registry,
        );
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].symbol, "PrimaryButton");
        assert_eq!(usages[0].registry_symbol.as_deref(), Some("PrimaryButton"));
        assert_eq!(usages[0].match_status, MatchStatus::Resolved);
    }

    #[test]
    fn alias_resolves_to_canonical_registry_symbol() {
        let registry = registry_without_packages(&[
            ("PrimaryButton", "PrimaryButton"),
            ("PrimaryBtn", "PrimaryButton"),
        ]);
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Screen() { PrimaryBtn(onClick = {}) }",
            &registry,
        );
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].symbol, "PrimaryBtn");
        assert_eq!(usages[0].registry_symbol.as_deref(), Some("PrimaryButton"));
    }

    #[test]
    fn comment_lines_are_not_extracted() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) =
            parse_and_extract("// PrimaryButton( not a call\nfun Screen() {}", &registry);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn string_literal_content_is_not_extracted() {
        let registry = registry_without_packages(&[("TextField", "TextField")]);
        let (_, usages) = parse_and_extract(
            "val label = \"TextField(not a call)\"\nfun Screen() {}",
            &registry,
        );
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn qualified_call_is_not_extracted() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Screen() { com.example.PrimaryButton(onClick = {}) }",
            &registry,
        );
        // navigation_expression as first child → not counted
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn composable_function_is_detected_as_local() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract("@Composable\nfun MyScreen() {}", &registry);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].symbol, "MyScreen");
    }

    #[test]
    fn non_composable_function_is_not_a_local_component() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract("fun helper() {}", &registry);
        assert_eq!(locals.len(), 0);
    }

    #[test]
    fn lowercase_composable_function_is_not_a_local_component() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract("@Composable\nfun myHelper() {}", &registry);
        assert_eq!(locals.len(), 0);
    }

    #[test]
    fn multiline_call_is_detected_at_first_line() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let source =
            "@Composable\nfun Screen() {\n    PrimaryButton(\n        onClick = {},\n    )\n}";
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 1);
        // Row 2 (0-based) = line 3 (1-based); col 4 (0-based) = col 5 (1-based)
        assert_eq!(usages[0].location.line, 3);
        assert_eq!(usages[0].location.column, Some(5));
    }

    #[test]
    fn annotation_on_previous_line_is_recognised() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract("@Composable\nfun CardComponent() {}", &registry);
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].symbol, "CardComponent");
    }

    #[test]
    fn qualified_annotation_is_recognised() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract(
            "@androidx.compose.runtime.Composable\nfun QualifiedCard() {}",
            &registry,
        );
        assert_eq!(locals.len(), 1);
        assert_eq!(locals[0].symbol, "QualifiedCard");
    }

    #[test]
    fn preview_composable_is_not_indexed_as_local_component() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract(
            r#"
@androidx.compose.ui.tooling.preview.Preview
@Composable
fun SamplePreview() {}

@Composable
@Preview
fun AlternatePreview() {}
"#,
            &registry,
        );
        assert!(locals.is_empty());
    }

    #[test]
    fn calls_inside_preview_composable_are_not_counted() {
        let registry = registry_without_packages(&[
            ("PrimaryButton", "PrimaryButton"),
            ("ProvideTheme", "ProvideTheme"),
        ]);
        let source = r#"
@Composable
fun LocalCard() {}

@Preview
@Composable
fun ExamplePreview() {
    PrimaryButton(onClick = {})
    LocalCard()
    ProvideTheme()
    UnknownCard()
}
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert!(usages.is_empty());
    }

    #[test]
    fn calls_inside_nested_composable_in_preview_are_not_counted() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let source = r#"
@Preview
@Composable
fun ExamplePreview() {
    @Composable
    fun InnerCard() {
        PrimaryButton(onClick = {})
        UnknownCard()
    }

    InnerCard()
}
"#;
        let (locals, usages) = parse_and_extract(source, &registry);
        assert!(locals.is_empty());
        assert!(usages.is_empty());
    }

    #[test]
    fn production_composable_in_preview_file_is_still_scanned() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let source = r#"
@Composable
fun LocalCard() {}

@Preview
@Composable
fun ExamplePreview() {
    PrimaryButton(onClick = {})
    LocalCard()
}

@Composable
fun Screen() {
    PrimaryButton(onClick = {})
    LocalCard()
}
"#;
        let (locals, usages) = parse_and_extract(source, &registry);
        let local_symbols = locals
            .iter()
            .map(|local| local.symbol.as_str())
            .collect::<Vec<_>>();
        assert_eq!(local_symbols, vec!["LocalCard", "Screen"]);

        let usage_symbols = usages
            .iter()
            .map(|usage| usage.symbol.as_str())
            .collect::<Vec<_>>();
        assert_eq!(usage_symbols, vec!["PrimaryButton", "LocalCard"]);
        assert!(usages.iter().all(|usage| {
            usage.parent.as_ref().map(|parent| parent.symbol.as_str()) == Some("Screen")
        }));
    }

    #[test]
    fn provider_and_effect_composables_are_not_indexed_as_local_components() {
        let registry = registry_without_packages(&[]);
        let source = r#"
@Composable
fun ProvideTheme() {}

@Composable
fun SideEffect() {}

@Composable
fun Screen() {}
"#;
        let (locals, _) = parse_and_extract(source, &registry);
        let local_symbols = locals
            .iter()
            .map(|local| local.symbol.as_str())
            .collect::<Vec<_>>();
        assert_eq!(local_symbols, vec!["Screen"]);
    }

    #[test]
    fn provider_and_effect_calls_are_not_counted_as_usage_sites() {
        let registry = registry_without_packages(&[
            ("PrimaryButton", "PrimaryButton"),
            ("ProvideTheme", "ProvideTheme"),
            ("LaunchEffect", "LaunchEffect"),
        ]);
        let source = r#"
@Composable
fun LocalCard() {}

@Composable
fun Screen() {
    ProvideTheme()
    LaunchEffect()
    SideEffect()
    PrimaryButton(onClick = {})
    LocalCard()
}
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        let usage_symbols = usages
            .iter()
            .map(|usage| usage.symbol.as_str())
            .collect::<Vec<_>>();
        assert_eq!(usage_symbols, vec!["PrimaryButton", "LocalCard"]);
    }

    #[test]
    fn non_ds_composable_call_becomes_local_usage() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (locals, usages) = parse_and_extract(
            "@Composable\nfun LocalCard() {}\n@Composable\nfun Screen() { LocalCard() }",
            &registry,
        );
        assert_eq!(locals.len(), 2);
        let local_usage = usages
            .iter()
            .find(|site| site.symbol == "LocalCard")
            .expect("LocalCard invocation must be present");
        assert_eq!(local_usage.match_status, MatchStatus::Local);
        assert!(local_usage.local_definition_id.is_some());
    }

    #[test]
    fn same_package_local_composable_resolves_across_files() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/src/main/kotlin")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton","targets":["compose"]}]}"#,
        )
        .unwrap();

        let source_dir = tmp.path().join("app/src/main/kotlin/com/example");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("LocalCard.kt"),
            "package com.example\n@Composable\nfun LocalCard() {}\n",
        )
        .unwrap();
        std::fs::write(
            source_dir.join("Screen.kt"),
            "package com.example\n@Composable\nfun Screen() { LocalCard() }\n",
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config).unwrap();
        let local_usage = result
            .usage_sites
            .iter()
            .find(|site| site.symbol == "LocalCard")
            .expect("LocalCard invocation must be emitted");
        assert_eq!(local_usage.match_status, MatchStatus::Local);
        assert_eq!(
            local_usage.local_definition_id.as_deref(),
            Some("local.compose:com.example.LocalCard")
        );
    }

    #[test]
    fn design_system_import_resolves_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert(
            "Button".to_owned(),
            Some("com.acme.designsystem".to_owned()),
        );
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import com.acme.designsystem.Button

@Composable
fun Screen() { Button(onClick = {}) }
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Resolved);
    }

    #[test]
    fn non_ds_import_is_not_counted_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert(
            "Button".to_owned(),
            Some("com.acme.designsystem".to_owned()),
        );
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import com.foundation.ui.Button

@Composable
fun Screen() { Button(onClick = {}) }
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn framework_subpackage_import_is_not_counted_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert(
            "Button".to_owned(),
            Some("com.acme.designsystem".to_owned()),
        );
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import androidx.compose.material3.Button

@Composable
fun Screen() { Button(onClick = {}) }
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn unclear_import_becomes_candidate_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert(
            "Button".to_owned(),
            Some("com.acme.designsystem".to_owned()),
        );
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = "@Composable\nfun Screen() { Button(onClick = {}) }";
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Candidate);
    }

    #[test]
    fn third_party_import_is_not_counted_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert(
            "Button".to_owned(),
            Some("com.acme.designsystem".to_owned()),
        );
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import com.other.vendor.Button

@Composable
fun Screen() { Button(onClick = {}) }
"#;
        let (_, usages) = parse_and_extract(source, &registry);
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
            excludes: vec![],
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
    fn partial_parse_reports_the_smallest_problem_and_keeps_prior_facts() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/src/main/kotlin")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        let source_dir = tmp.path().join("app/src/main/kotlin");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("Screen.kt"),
            "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\nfun Broken(\n",
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should keep extracting from partial trees");

        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.usage_sites.len(), 1);
        assert_eq!(result.local_components.len(), 1);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "parse_failed"),
            "partial trees with syntax errors must emit parse_failed"
        );
        let parse_failed = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "parse_failed")
            .expect("parse_failed diagnostic");
        assert_eq!(
            parse_failed.location.as_ref().map(|location| location.line),
            Some(5)
        );
        assert!(
            parse_failed.message.contains("file scanned with gaps"),
            "partial parse message should explain retained scan coverage"
        );
        assert_eq!(result.status, ScanStatus::Partial);
    }

    #[test]
    fn partial_and_valid_files_both_count_and_keep_valid_ui_facts() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/src/main/kotlin")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).expect("create registry dir");
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .expect("write registry");

        let source_dir = tmp.path().join("app/src/main/kotlin");
        std::fs::create_dir_all(&source_dir).expect("create source dir");
        std::fs::write(
            source_dir.join("Valid.kt"),
            "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\n",
        )
        .expect("write valid source");
        std::fs::write(source_dir.join("Broken.kt"), "@Composable\nfun Broken(\n")
            .expect("write broken source");

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should complete across malformed and valid files");

        assert_eq!(result.files_scanned, 2);
        assert!(
            result
                .local_components
                .iter()
                .any(|component| component.symbol == "Screen"),
            "valid composable should still be indexed"
        );
        assert!(
            result
                .usage_sites
                .iter()
                .any(|usage| usage.symbol == "PrimaryButton"),
            "valid file usage facts should survive unrelated parse failures"
        );
        assert_eq!(result.status, ScanStatus::Partial);
    }

    #[test]
    fn annotated_parenthesized_generic_function_type_does_not_emit_parse_failed() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/src/main/kotlin")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        let source_dir = tmp.path().join("app/src/main/kotlin");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("MainApp.kt"),
            r#"
import androidx.compose.runtime.Composable

interface NavArgument
interface NavDecoration

private object CapsuleDecor : NavDecoration {
    @Composable
    override fun <T : NavArgument> DecoratedContent(
        args: List<T>,
        modifier: Modifier,
        content: @Composable ((T) -> Unit),
    ) {
        PrimaryButton(onClick = {})
        content.invoke(args.first())
    }
}
"#,
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config).expect("scan should succeed");

        assert_eq!(result.status, ScanStatus::Complete);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "parse_failed"),
            "valid annotated parenthesized function types must not emit parse_failed: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .usage_sites
                .iter()
                .any(|usage| usage.symbol == "PrimaryButton"),
            "scanner should still extract calls from the file"
        );
    }

    #[test]
    fn malformed_annotated_parenthesized_function_type_still_emits_parse_failed() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/src/main/kotlin")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        let source_dir = tmp.path().join("app/src/main/kotlin");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("BrokenMainApp.kt"),
            r#"
import androidx.compose.runtime.Composable

interface NavArgument

@Composable
fun BrokenScreen(
    content: @Composable ((NavArgument) -> Unit,
) {
    PrimaryButton(onClick = {})
}
"#,
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should keep extracting from malformed trees");

        assert_eq!(result.files_scanned, 1);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "parse_failed"),
            "malformed annotated parenthesized function types must still emit parse_failed"
        );
        assert_eq!(result.status, ScanStatus::Partial);
    }

    #[test]
    fn unmatched_wildcard_root_emits_glob_warning() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("*/src/main/kotlin")],
            excludes: vec![],
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
            excludes: vec![],
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

    #[test]
    fn recursive_wildcard_root_scans_nested_modules() {
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("capsule/**/src/main/kotlin")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .unwrap();

        for module in ["shared/feature", "design-system"] {
            let source_dir = tmp
                .path()
                .join("capsule")
                .join(module)
                .join("src/main/kotlin");
            std::fs::create_dir_all(&source_dir).unwrap();
            std::fs::write(
                source_dir.join("Screen.kt"),
                "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\n",
            )
            .unwrap();
        }

        let excluded_dir = tmp.path().join("other/shared/feature/src/main/kotlin");
        std::fs::create_dir_all(&excluded_dir).unwrap();
        std::fs::write(
            excluded_dir.join("Screen.kt"),
            "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\n",
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("recursive wildcard roots should scan matching modules");

        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.usage_sites.len(), 2);
        assert_eq!(result.status, ScanStatus::Complete);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "root_not_found"),
            "matching recursive wildcard roots must not emit root_not_found diagnostics"
        );
    }

    fn extract_hardcoded_styles(source: &str) -> Vec<HardcodedStyleSite> {
        let mut parser = make_parser();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        let mut sites = Vec::new();
        extract_hardcoded_style_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.kt",
            &mut sites,
        );
        sites
    }

    fn token_match_index(pairs: &[(&str, &str, TokenCategory)]) -> RegistryTokenIndex {
        let mut tokens = Vec::new();
        for (id, key, category) in pairs {
            tokens.push(DesignSystemToken {
                id: (*id).to_owned(),
                key: (*key).to_owned(),
                category: *category,
                aliases: Vec::new(),
                value: None,
            });
        }
        token_index(&tokens).expect("token index should build")
    }

    fn extract_token_sites(source: &str, index: &RegistryTokenIndex) -> Vec<TokenSite> {
        let mut parser = make_parser();
        let tree = parser.parse(source.as_bytes(), None).unwrap();
        let mut sites = Vec::new();
        extract_token_sites_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.kt",
            index,
            &mut sites,
        );
        sites
    }

    #[test]
    fn qualified_padding_call_is_a_spacing_hardcoded_candidate() {
        let source = "@Composable\nfun Screen() {\n    Box(Modifier.padding(8.dp))\n}\n";
        let sites = extract_hardcoded_styles(source);
        assert!(
            sites
                .iter()
                .any(|site| site.category == TokenCategory::Spacing && site.value == "8.dp"),
            "expected a spacing candidate with value 8.dp, got: {sites:?}"
        );
    }

    #[test]
    fn direct_color_call_is_a_color_hardcoded_candidate() {
        let source =
            "@Composable\nfun Screen() {\n    Box(Modifier.background(Color(0xFF336699)))\n}\n";
        let sites = extract_hardcoded_styles(source);
        assert!(
            sites
                .iter()
                .any(|site| site.category == TokenCategory::Color && site.value.contains("0x")),
            "expected a color candidate containing 0x, got: {sites:?}"
        );
    }

    #[test]
    fn nested_background_color_emits_one_color_candidate() {
        let source =
            "@Composable\nfun Screen() {\n    Box(modifier.background(Color(0xFF336699)))\n}\n";
        let sites = extract_hardcoded_styles(source);
        assert_eq!(
            sites
                .iter()
                .filter(|site| {
                    site.category == TokenCategory::Color && site.value.contains("0x")
                })
                .count(),
            1,
            "nested background(Color(...)) must not double-count the same literal, got: {sites:?}"
        );
    }

    #[test]
    fn preview_composable_hardcoded_styles_are_skipped() {
        let source = "\
@Preview
@Composable
fun PreviewScreen() {
    Box(modifier.padding(8.dp).background(Color(0xFF336699)))
}
";
        let sites = extract_hardcoded_styles(source);
        assert!(
            sites.is_empty(),
            "hard-coded styles inside @Preview must be skipped, got: {sites:?}"
        );
    }

    #[test]
    fn preview_composable_token_references_are_skipped() {
        let index = token_match_index(&[(
            "color.primary",
            "Theme.colors.primary",
            TokenCategory::Color,
        )]);
        let source = "\
@Preview
@Composable
fun PreviewScreen() {
    val primary = Theme.colors.primary
}
";
        let sites = extract_token_sites(source, &index);
        assert!(
            sites.is_empty(),
            "token references inside @Preview must be skipped, got: {sites:?}"
        );
    }

    #[test]
    fn non_style_call_does_not_emit_hardcoded_candidate() {
        let source = "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\n";
        let sites = extract_hardcoded_styles(source);
        assert!(
            sites.is_empty(),
            "non-styling calls must not emit hard-coded candidates, got: {sites:?}"
        );
    }

    #[test]
    fn hardcoded_style_candidate_has_parent_attribution_inside_composable() {
        let source = "@Composable\nfun Screen() {\n    Box(Modifier.padding(8.dp))\n}\n";
        let sites = extract_hardcoded_styles(source);
        assert!(
            sites
                .iter()
                .all(|site| site.parent.as_ref().is_some_and(|p| p.symbol == "Screen")),
            "hard-coded candidates inside a composable must carry parent attribution"
        );
    }

    #[test]
    fn token_reference_resolves_qualified_navigation_expression() {
        let index = token_match_index(&[(
            "color.primary",
            "Theme.colors.primary",
            TokenCategory::Color,
        )]);
        let source = "@Composable\nfun Screen() {\n    val primary = Theme.colors.primary\n}\n";
        let sites = extract_token_sites(source, &index);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].token_id, "color.primary");
        assert_eq!(sites[0].key, "Theme.colors.primary");
        assert!(
            sites[0]
                .parent
                .as_ref()
                .is_some_and(|parent| parent.symbol == "Screen"),
            "token reference inside a composable must carry parent attribution"
        );
    }

    #[test]
    fn token_reference_outside_composable_has_no_parent() {
        let index = token_match_index(&[(
            "color.primary",
            "Theme.colors.primary",
            TokenCategory::Color,
        )]);
        let source = "val primary = Theme.colors.primary\n";
        let sites = extract_token_sites(source, &index);
        assert_eq!(sites.len(), 1);
        assert!(sites[0].parent.is_none());
    }

    #[test]
    fn token_key_matching_parameter_declaration_is_not_usage() {
        let index = token_match_index(&[("color.primary", "primary", TokenCategory::Color)]);
        let source = "@Composable\nfun Screen(primary: Color) {}\n";
        let sites = extract_token_sites(source, &index);
        assert!(
            sites.is_empty(),
            "parameter declarations must not count as token usage, got: {sites:?}"
        );
    }

    #[test]
    fn token_key_matching_variable_declaration_is_not_usage() {
        let index = token_match_index(&[("color.primary", "primary", TokenCategory::Color)]);
        let source = "@Composable\nfun Screen() {\n    val primary = Color.Red\n}\n";
        let sites = extract_token_sites(source, &index);
        assert!(
            sites.is_empty(),
            "variable declarations must not count as token usage, got: {sites:?}"
        );
    }

    #[test]
    fn token_key_matching_identifier_reference_is_usage() {
        let index = token_match_index(&[("color.primary", "primary", TokenCategory::Color)]);
        let source = "@Composable\nfun Screen(primary: Color) {\n    val x = primary\n}\n";
        let sites = extract_token_sites(source, &index);
        assert_eq!(sites.len(), 1, "expected one reference use, got: {sites:?}");
        assert_eq!(sites[0].key, "primary");
    }

    #[test]
    fn color_int_and_float_literals_are_hardcoded_candidates() {
        let int_sites =
            extract_hardcoded_styles("@Composable\nfun Screen() {\n    Color(255)\n}\n");
        assert!(
            int_sites
                .iter()
                .any(|site| site.category == TokenCategory::Color && site.value == "255"),
            "Color(255) should be a color candidate, got: {int_sites:?}"
        );

        let float_sites =
            extract_hardcoded_styles("@Composable\nfun Screen() {\n    Color(0.5f)\n}\n");
        assert!(
            float_sites
                .iter()
                .any(|site| site.category == TokenCategory::Color && site.value == "0.5f"),
            "Color(0.5f) should be a color candidate, got: {float_sites:?}"
        );
    }

    #[test]
    fn dp_chained_off_identifier_is_not_hardcoded_spacing() {
        let source =
            "@Composable\nfun Screen() {\n    Box(modifier.padding(Spacing.medium.dp))\n}\n";
        let sites = extract_hardcoded_styles(source);
        assert!(
            sites
                .iter()
                .all(|site| site.category != TokenCategory::Spacing),
            "Spacing.medium.dp must not be treated as a hard-coded spacing literal, got: {sites:?}"
        );
    }
}
