//! Tree-sitter-swift backed SwiftUI scanner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, IdentityStability, LocalComponent,
    MatchStatus, ParentScope, ScanStatus, SourceLocation, UsageSite,
};

use crate::component_detect::{
    collect_component_declarations, is_pascal_case_symbol, nearest_enclosing_view,
};
use crate::swift_ast::{
    ImportBindings, ParseSwiftFileError, collect_import_bindings, collect_swift_files, new_parser,
    parse_swift_file_permissive, partial_tree_parse_diagnostic, tree_has_syntax_errors,
    unparseable_file_diagnostic,
};
use wax_lang_api::{
    RootPatternKind, RootResolutionError, ScanConfig, resolve_import_aware_match,
    resolve_source_roots, swift_module_from_source_path,
};

/// Parsed Swift scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwiftScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Swift source roots to scan.
    pub roots: Vec<PathBuf>,
    /// Repo-relative file paths or glob patterns to exclude from scanning.
    pub excludes: Vec<String>,
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
    /// Configured registry file does not exist.
    RegistryNotFound {
        /// Registry path that was missing.
        path: PathBuf,
        /// Human-readable reason.
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
            Self::RegistryNotFound { path, reason } => {
                write!(
                    f,
                    "design-system registry not found at {}: {reason}",
                    path.display()
                )
            }
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
            | Self::RegistryNotFound { .. }
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
    let has_excludes = config.contains_key("excludes");
    if !has_registry && !has_roots && !has_excludes {
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

    let excludes = parse_excludes(config)?;

    Ok(SwiftConfigMode::Configured(SwiftScanConfig {
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
    resolve_targets: BTreeMap<String, String>,
    component_packages: BTreeMap<String, Option<String>>,
}

fn load_registry(path: &Path) -> Result<RegistryIndex, TreeSitterScanError> {
    let raw = fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            TreeSitterScanError::RegistryNotFound {
                path: path.to_path_buf(),
                reason: source.to_string(),
            }
        } else {
            TreeSitterScanError::Io {
                context: format!("read design-system registry {}", path.display()),
                source,
            }
        }
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
            reason: "registry must declare at least one Swift component symbol".to_owned(),
        });
    }

    canonical_symbols.sort();
    Ok(RegistryIndex {
        canonical_symbols,
        resolve_targets,
        component_packages,
    })
}

fn resolve_registry_match(
    call_symbol: &str,
    call_qualifier: Option<&str>,
    registry_symbol: &str,
    registry: &RegistryIndex,
    imports: &ImportBindings,
) -> Option<MatchStatus> {
    resolve_import_aware_match(
        registry
            .component_packages
            .get(registry_symbol)
            .and_then(|package| package.as_deref()),
        imports
            .package_for_call(call_symbol, call_qualifier)
            .as_deref(),
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

fn module_identity_for_file(file: &str) -> (String, bool) {
    if let Some(module) = swift_module_from_source_path(Path::new(file)) {
        (module, true)
    } else {
        let stem = Path::new(file)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(file)
            .to_owned();
        (stem, false)
    }
}

fn qualified_view_symbol(module_identity: &str, symbol: &str) -> String {
    format!("{module_identity}.{symbol}")
}

fn local_definition_id(module_identity: &str, symbol: &str) -> String {
    format!("local.swift:{module_identity}#{symbol}")
}

fn local_component_for_declaration(
    file: &str,
    module_identity: &str,
    semantic_module: bool,
    component: &crate::component_detect::DetectedComponent,
) -> LocalComponent {
    let qualified_symbol = qualified_view_symbol(module_identity, &component.symbol);
    LocalComponent {
        id: local_definition_id(module_identity, &component.symbol),
        symbol: component.symbol.clone(),
        qualified_symbol: Some(qualified_symbol),
        identity_basis: Some(if semantic_module {
            "module_qualified_symbol".to_owned()
        } else {
            "module_path_and_symbol".to_owned()
        }),
        identity_stability: Some(if semantic_module {
            IdentityStability::Semantic
        } else {
            IdentityStability::PathSensitive
        }),
        location: SourceLocation {
            file: file.to_owned(),
            line: component.line,
            column: Some(component.column),
        },
    }
}

fn parent_scope_for_view(
    file: &str,
    module_identity: &str,
    semantic_module: bool,
    view_name: &str,
    pos: tree_sitter::Point,
) -> ParentScope {
    let qualified_symbol = qualified_view_symbol(module_identity, view_name);
    ParentScope {
        parent_id: format!("swiftui:view:{module_identity}#{view_name}"),
        symbol: view_name.to_owned(),
        qualified_symbol: Some(qualified_symbol),
        scope_kind: "view".to_owned(),
        identity_basis: if semantic_module {
            "module_qualified_symbol".to_owned()
        } else {
            "module_path_and_symbol".to_owned()
        },
        identity_stability: if semantic_module {
            IdentityStability::Semantic
        } else {
            IdentityStability::PathSensitive
        },
        location: Some(SourceLocation {
            file: file.to_owned(),
            line: pos.row as u32 + 1,
            column: Some(pos.column as u32 + 1),
        }),
    }
}

#[derive(Debug, Default)]
struct LocalViewIndex {
    by_file_symbol: BTreeMap<(String, String), LocalComponent>,
    by_qualified: BTreeMap<String, LocalComponent>,
}

impl LocalViewIndex {
    fn insert(&mut self, file: &str, component: LocalComponent) {
        if let Some(qualified) = &component.qualified_symbol {
            self.by_qualified
                .insert(qualified.clone(), component.clone());
        }
        self.by_file_symbol
            .insert((file.to_owned(), component.symbol.clone()), component);
    }

    fn resolve(&self, file: &str, module_identity: &str, symbol: &str) -> Option<&LocalComponent> {
        if let Some(component) = self
            .by_file_symbol
            .get(&(file.to_owned(), symbol.to_owned()))
        {
            return Some(component);
        }
        let qualified = qualified_view_symbol(module_identity, symbol);
        self.by_qualified.get(&qualified)
    }
}

fn unresolved_symbol_is_swiftui_shaped(
    call_site: &ResolvedCallSite,
    imports: &ImportBindings,
) -> bool {
    if call_site.qualifier.as_deref() == Some("SwiftUI") {
        return false;
    }
    if is_framework_swiftui_symbol(&call_site.symbol) {
        return false;
    }
    if !imports
        .module_imports
        .iter()
        .any(|module| module == "SwiftUI")
    {
        return false;
    }
    if imports
        .package_for_call(&call_site.symbol, call_site.qualifier.as_deref())
        .as_deref()
        .is_some_and(|package| package != "SwiftUI")
    {
        return false;
    }
    true
}

fn is_framework_swiftui_symbol(symbol: &str) -> bool {
    matches!(
        symbol,
        "AnyView"
            | "Button"
            | "Color"
            | "Divider"
            | "EmptyView"
            | "ForEach"
            | "Form"
            | "Group"
            | "HStack"
            | "Image"
            | "Label"
            | "LazyHGrid"
            | "LazyHStack"
            | "LazyVGrid"
            | "LazyVStack"
            | "List"
            | "NavigationLink"
            | "NavigationStack"
            | "Picker"
            | "ProgressView"
            | "ScrollView"
            | "Section"
            | "Slider"
            | "Spacer"
            | "Text"
            | "TextField"
            | "Toggle"
            | "VStack"
            | "ZStack"
    )
}

fn index_local_components_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
) -> Vec<LocalComponent> {
    let (module_identity, semantic_module) = module_identity_for_file(file);
    collect_component_declarations(root, source, false)
        .into_iter()
        .map(|component| {
            local_component_for_declaration(file, &module_identity, semantic_module, &component)
        })
        .collect()
}

fn extract_usage_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    registry: &RegistryIndex,
    local_index: &LocalViewIndex,
    usage_sites: &mut Vec<UsageSite>,
) {
    let (module_identity, semantic_module) = module_identity_for_file(file);
    let imports = collect_import_bindings(root, source);

    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_call_expression_node(node)
            && let Some(call_site) = resolve_call_site(node, source)
            && is_pascal_case_symbol(&call_site.symbol)
        {
            let line = call_site.position.row as u32 + 1;
            let column = call_site.position.column as u32 + 1;
            let location = SourceLocation {
                file: file.to_owned(),
                line,
                column: Some(column),
            };
            let parent = nearest_enclosing_view(node, source).map(|(name, parent_pos)| {
                parent_scope_for_view(file, &module_identity, semantic_module, &name, parent_pos)
            });

            if let Some(registry_symbol) = registry.resolve_targets.get(&call_site.symbol) {
                if let Some(match_status) = resolve_registry_match(
                    &call_site.symbol,
                    call_site.qualifier.as_deref(),
                    registry_symbol,
                    registry,
                    &imports,
                ) {
                    usage_sites.push(UsageSite {
                        id: format!("usage.swift:{file}:{line}:{column}:{}", call_site.symbol),
                        location: location.clone(),
                        symbol: call_site.symbol.clone(),
                        qualified_symbol: None,
                        match_status,
                        registry_symbol: Some(registry_symbol.clone()),
                        local_definition_id: None,
                        parent,
                    });
                }
            } else if let Some(local) =
                local_index.resolve(file, &module_identity, &call_site.symbol)
            {
                usage_sites.push(UsageSite {
                    id: format!("usage.swift:{file}:{line}:{column}:{}", call_site.symbol),
                    location: location.clone(),
                    symbol: call_site.symbol.clone(),
                    qualified_symbol: local.qualified_symbol.clone(),
                    match_status: MatchStatus::Local,
                    registry_symbol: None,
                    local_definition_id: Some(local.id.clone()),
                    parent,
                });
            } else if parent.is_some() && unresolved_symbol_is_swiftui_shaped(&call_site, &imports)
            {
                usage_sites.push(UsageSite {
                    id: format!("usage.swift:{file}:{line}:{column}:{}", call_site.symbol),
                    location,
                    symbol: call_site.symbol,
                    qualified_symbol: None,
                    match_status: MatchStatus::Unresolved,
                    registry_symbol: None,
                    local_definition_id: None,
                    parent,
                });
            }
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
}

#[allow(dead_code)]
fn extract_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    registry: &RegistryIndex,
    local_index: &LocalViewIndex,
    local_components: &mut Vec<LocalComponent>,
    usage_sites: &mut Vec<UsageSite>,
) {
    for local in index_local_components_from_source(root, source, file) {
        local_components.push(local);
    }
    extract_usage_from_source(root, source, file, registry, local_index, usage_sites);
}

fn is_call_expression_node(node: tree_sitter::Node<'_>) -> bool {
    node.kind() == "call_expression"
}

struct ResolvedCallSite {
    symbol: String,
    qualifier: Option<String>,
    position: tree_sitter::Point,
}

fn resolve_call_site(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<ResolvedCallSite> {
    let mut cursor = node.walk();
    let callee = node.named_children(&mut cursor).next()?;
    resolve_call_site_from_callee(callee, source)
}

fn resolve_call_site_from_callee(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<ResolvedCallSite> {
    match node.kind() {
        "simple_identifier" => {
            let name = node.utf8_text(source).ok()?.to_owned();
            Some(ResolvedCallSite {
                symbol: name,
                qualifier: None,
                position: node.start_position(),
            })
        }
        "navigation_expression" => {
            let suffix = node.child_by_field_name("suffix")?;
            let member = suffix.child_by_field_name("suffix")?;
            if member.kind() != "simple_identifier" {
                return None;
            }
            let name = member.utf8_text(source).ok()?.to_owned();
            let qualifier = navigation_expression_qualifier(node, source);
            Some(ResolvedCallSite {
                symbol: name,
                qualifier,
                position: member.start_position(),
            })
        }
        _ => None,
    }
}

fn navigation_expression_qualifier(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let target = node.child_by_field_name("target")?;
    identifier_from_expression(target, source)
}

fn identifier_from_expression(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "simple_identifier" | "type_identifier" => node.utf8_text(source).ok().map(str::to_owned),
        "navigation_expression" => {
            let target = node.child_by_field_name("target")?;
            identifier_from_expression(target, source)
        }
        _ => None,
    }
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
    swift_files.retain(|file_path| {
        let relative_file = file_path.strip_prefix(repo_root).unwrap_or(file_path);
        let relative_text = normalize_repo_relative_path(relative_file);
        !path_matches_any(&relative_text, &config.excludes)
    });

    let mut parser =
        new_parser().map_err(|reason| TreeSitterScanError::ParserInitFailed { reason })?;

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
    let mut parsed_files = Vec::new();
    for file_path in &swift_files {
        files_scanned += 1;
        let relative_file = file_path
            .strip_prefix(repo_root)
            .unwrap_or(file_path)
            .display()
            .to_string();

        match parse_swift_file_permissive(&mut parser, file_path) {
            Ok(parsed) => {
                if tree_has_syntax_errors(&parsed.tree) {
                    parse_failures += 1;
                    diagnostics.push(partial_tree_parse_diagnostic(
                        parsed.tree.root_node(),
                        &relative_file,
                    ));
                }
                parsed_files.push((relative_file, parsed));
            }
            Err(ParseSwiftFileError::ParseFailed(_)) => {
                parse_failures += 1;
                diagnostics.push(unparseable_file_diagnostic(&relative_file));
            }
            Err(ParseSwiftFileError::Io { context, source }) => {
                return Err(TreeSitterScanError::Io { context, source });
            }
        }
    }

    let mut local_index = LocalViewIndex::default();
    for (relative_file, parsed) in &parsed_files {
        for local in index_local_components_from_source(
            parsed.tree.root_node(),
            parsed.source.as_bytes(),
            relative_file,
        ) {
            local_index.insert(relative_file, local.clone());
            local_components.push(local);
        }
    }

    for (relative_file, parsed) in &parsed_files {
        extract_usage_from_source(
            parsed.tree.root_node(),
            parsed.source.as_bytes(),
            relative_file,
            &registry,
            &local_index,
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

    let has_gaps = parse_failures > 0
        || diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "root_not_found" || diagnostic.code == "root_glob_not_found"
        });

    Ok(TreeSitterScanResult {
        design_system_components,
        local_components,
        usage_sites,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parser() -> tree_sitter::Parser {
        new_parser().expect("parser")
    }

    fn resolve_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
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
        let tree = parser.parse(source.as_bytes(), None).expect("parse");
        let mut local_index = LocalViewIndex::default();
        let mut locals = Vec::new();
        for local in
            index_local_components_from_source(tree.root_node(), source.as_bytes(), "Test.swift")
        {
            local_index.insert("Test.swift", local.clone());
            locals.push(local);
        }
        let mut usages = Vec::new();
        extract_usage_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.swift",
            registry,
            &local_index,
            &mut usages,
        );
        (locals, usages)
    }

    #[test]
    fn parse_config_rejects_parent_dir_roots() {
        let mut config = ScanConfig::new();
        config.insert("registry".to_owned(), serde_json::json!("registry.json"));
        config.insert("roots".to_owned(), serde_json::json!(["../Sources/App"]));

        let err = parse_swift_scan_config(&config).expect_err("parent-dir roots must fail");
        assert!(matches!(err, TreeSitterScanError::ConfigInvalid { .. }));
    }

    #[test]
    fn direct_member_and_alias_calls_resolve_to_registry_symbols() {
        let registry = registry_without_packages(&[
            ("PrimaryButton", "PrimaryButton"),
            ("PrimaryCTA", "PrimaryButton"),
            ("Card", "Card"),
        ]);
        let (_, usages) = parse_and_extract(
            r#"
        struct Screen: View {
            var body: some View {
                VStack {
                    PrimaryButton(title: "Save")
                    DesignSystem.PrimaryCTA(title: "Go")
                    DS.Card { Text("Body") }
                }
            }
        }
        "#,
            &registry,
        );

        assert_eq!(usages.len(), 3);
        assert_eq!(usages[0].registry_symbol.as_deref(), Some("PrimaryButton"));
        assert_eq!(usages[1].registry_symbol.as_deref(), Some("PrimaryButton"));
        assert_eq!(usages[2].registry_symbol.as_deref(), Some("Card"));
    }

    #[test]
    fn comments_strings_and_non_registry_calls_are_ignored() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            r#"
        let label = "PrimaryButton(title:)"
        // PrimaryButton(title: "No")
        func Screen() -> some View {
            LocalCard()
        }
        "#,
            &registry,
        );

        assert!(usages.is_empty());
    }

    #[test]
    fn unknown_pascal_case_view_call_becomes_unresolved() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            r#"
        import SwiftUI
        struct Screen: View {
            var body: some View {
                VStack {
                    Text("Title")
                    UnknownCard()
                }
            }
        }
        "#,
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].symbol, "UnknownCard");
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
    }

    #[test]
    fn framework_swiftui_calls_are_not_unresolved() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            r#"
        import SwiftUI
        struct Screen: View {
            var body: some View {
                VStack {
                    Text("Title")
                    SwiftUI.Button("Save") {}
                }
            }
        }
        "#,
            &registry,
        );

        assert!(usages.is_empty());
    }

    #[test]
    fn multiline_call_is_detected_at_first_line() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let source = r#"
        struct Screen: View {
            var body: some View {
                PrimaryButton(
                    title: "Save"
                )
            }
        }
        "#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].location.line, 4);
        assert!(usages[0].location.column.unwrap() >= 16);
    }

    #[test]
    fn non_ds_module_import_is_not_counted_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert("Button".to_owned(), Some("AcmeDesignSystem".to_owned()));
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import SwiftUI

struct Screen: View {
    var body: some View {
        Button("Save") {}
    }
}
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn qualified_non_ds_call_is_not_counted_when_package_is_configured() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert("Button".to_owned(), Some("AcmeDesignSystem".to_owned()));
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import SwiftUI
import AcmeDesignSystem

struct Screen: View {
    var body: some View {
        SwiftUI.Button("Save") {}
    }
}
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 0);
    }

    #[test]
    fn unqualified_call_with_multiple_module_imports_becomes_candidate() {
        let mut component_packages = BTreeMap::new();
        component_packages.insert("Button".to_owned(), Some("AcmeDesignSystem".to_owned()));
        let registry = registry_index(resolve_map(&[("Button", "Button")]), component_packages);
        let source = r#"
import SwiftUI
import AcmeDesignSystem

struct Screen: View {
    var body: some View {
        Button("Save") {}
    }
}
"#;
        let (_, usages) = parse_and_extract(source, &registry);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Candidate);
    }

    #[test]
    fn missing_root_emits_warning_diagnostic_and_partial_status() {
        let config = SwiftScanConfig {
            design_system_registry: std::path::PathBuf::from("does-not-exist/registry.json"),
            roots: vec![std::path::PathBuf::from("no-such-root")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("does-not-exist");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Btn","targets":["swift"]}]}"#,
        )
        .unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should succeed even with missing root");

        let has_root_warning = result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "root_not_found");
        assert!(has_root_warning, "expected root_not_found diagnostic");
        assert_eq!(
            result.status,
            ScanStatus::Partial,
            "missing root must yield Partial, not Complete"
        );
        assert_eq!(result.files_scanned, 0);
    }

    #[test]
    fn partial_parse_still_extracts_symbols_during_scan() {
        let config = SwiftScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/Sources")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton","targets":["swift"]}]}"#,
        )
        .unwrap();

        let source_dir = tmp.path().join("app/Sources");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("Screen.swift"),
            "struct Screen: View {\n    var body: some View {\n        PrimaryButton(title: \"Save\")\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(source_dir.join("Broken.swift"), "struct Broken(\n").unwrap();

        let result = scan_repository(tmp.path(), &config)
            .expect("scan should keep extracting from valid files");

        assert_eq!(result.files_scanned, 2);
        assert_eq!(result.usage_sites.len(), 1);
        assert_eq!(result.local_components.len(), 1);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "parse_failed"),
            "partial trees with syntax errors must emit parse_failed"
        );
        assert_eq!(result.status, ScanStatus::Partial);
    }

    #[test]
    fn unmatched_wildcard_root_emits_glob_warning() {
        let config = SwiftScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("*/Sources")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"Btn","targets":["swift"]}]}"#,
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
        let config = SwiftScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("*/Sources")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton","targets":["swift"]}]}"#,
        )
        .unwrap();

        for module in ["app", "feature-profile"] {
            let source_dir = tmp.path().join(module).join("Sources");
            std::fs::create_dir_all(&source_dir).unwrap();
            std::fs::write(
                source_dir.join("Screen.swift"),
                "struct Screen: View {\n    var body: some View {\n        PrimaryButton(title: \"Save\")\n    }\n}\n",
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
        let config = SwiftScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("capsule/**/Sources")],
            excludes: vec![],
        };

        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton","targets":["swift"]}]}"#,
        )
        .unwrap();

        for module in ["shared/feature", "design-system"] {
            let source_dir = tmp.path().join("capsule").join(module).join("Sources");
            std::fs::create_dir_all(&source_dir).unwrap();
            std::fs::write(
                source_dir.join("Screen.swift"),
                "struct Screen: View {\n    var body: some View {\n        PrimaryButton(title: \"Save\")\n    }\n}\n",
            )
            .unwrap();
        }

        let excluded_dir = tmp.path().join("other/shared/feature/Sources");
        std::fs::create_dir_all(&excluded_dir).unwrap();
        std::fs::write(
            excluded_dir.join("Screen.swift"),
            "struct Screen: View {\n    var body: some View {\n        PrimaryButton(title: \"Save\")\n    }\n}\n",
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
}
