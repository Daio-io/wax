//! Tree-sitter-kotlin backed Compose scanner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::kotlin_ast::{
    ImportBindings, ParseKotlinFileError, call_simple_callee, collect_import_bindings,
    collect_kotlin_files, function_name_from_decl, has_composable_annotation,
    has_preview_annotation, is_non_ui_scaffolding_composable_symbol,
    is_pascal_case_composable_symbol, is_within_preview_composable, nearest_enclosing_composable,
    new_parser, node_has_error_ancestor_within, package_name_from_source,
    parse_kotlin_file_permissive, partial_tree_parse_diagnostic, unparseable_file_diagnostic,
};
use crate::kotlin_recovery::{
    ByteRange, ComponentScopePolicy, SyntaxRegion, node_in_type_annotation_range,
};

/// Grammar version bundled via the `tree-sitter-kotlin-ng` crate dependency.
/// Update this constant when bumping the crate in `Cargo.toml`.
pub const TREE_SITTER_KOTLIN_GRAMMAR_VERSION: &str = "1.1.0";

use wax_contract::{
    CalleeOrigin, DesignSystemComponent, DesignSystemToken, Diagnostic, DiagnosticSeverity,
    HardcodedStyleSite, IdentityStability, LocalComponent, MatchStatus, ParentScope,
    ResolutionEvidence, ResolutionEvidenceKind, ScanStatus, SourceLocation, StyleContext,
    TokenCategory, TokenSite, UsageSite,
};
use wax_lang_api::{
    RegistryImportMatch, RegistryTokenIndex, RootResolutionError, ScanConfig,
    normalize_repo_relative_path, parse_registry_tokens, path_matches_any,
    resolve_import_aware_match, resolve_source_roots, root_not_found_code, root_not_found_message,
    token_index,
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
) -> RegistryImportMatch {
    resolve_import_aware_match(
        registry
            .component_packages
            .get(registry_symbol)
            .and_then(|package| package.as_deref()),
        imports.package_for_symbol(call_symbol),
    )
}

fn qualified_composable_symbol(package: Option<&str>, symbol: &str) -> String {
    package
        .map(|pkg| format!("{pkg}.{symbol}"))
        .unwrap_or_else(|| symbol.to_owned())
}

fn unresolved_origin(package: Option<&str>) -> CalleeOrigin {
    match package {
        Some(package) if package.starts_with("androidx.compose") => CalleeOrigin::Framework,
        Some(package) if package.starts_with("androidx.ui") => CalleeOrigin::Framework,
        Some(_) => CalleeOrigin::External,
        None => CalleeOrigin::Application,
    }
}

fn unresolved_origin_for_symbol(symbol: &str, package: Option<&str>) -> CalleeOrigin {
    if package.is_none()
        && matches!(
            symbol,
            "Box"
                | "Button"
                | "Color"
                | "Column"
                | "Image"
                | "Row"
                | "Text"
                | "TextField"
                | "TextStyle"
                | "RoundedCornerShape"
                | "Surface"
                | "Spacer"
        )
    {
        CalleeOrigin::Framework
    } else {
        unresolved_origin(package)
    }
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

    fn same_file(&self, file: &str, symbol: &str) -> Option<&LocalComponent> {
        self.by_file_symbol
            .get(&(file.to_owned(), symbol.to_owned()))
    }

    fn qualified_package(&self, package: Option<&str>, symbol: &str) -> Option<&LocalComponent> {
        let package = package?;
        let qualified = qualified_composable_symbol(Some(package), symbol);
        self.by_qualified.get(&qualified)
    }

    fn current_package(&self, package: Option<&str>, symbol: &str) -> Option<&LocalComponent> {
        self.qualified_package(package, symbol)
    }
}

fn index_local_components_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    clean: &[ByteRange],
) -> Vec<LocalComponent> {
    let package = package_name_from_source(root, source);
    let mut local_components = Vec::new();

    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_declaration"
            && node_is_extractable(node, clean)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiScope {
    NonUi,
    Composable,
    ComposableLambda,
}

impl UiScope {
    fn is_ui(self) -> bool {
        !matches!(self, Self::NonUi)
    }
}

fn node_is_extractable(node: tree_sitter::Node<'_>, clean: &[ByteRange]) -> bool {
    clean.iter().any(|range| range.contains_node(node))
        && !node_has_error_ancestor_within(node, clean)
}

fn component_scope_for_child(
    child: tree_sitter::Node<'_>,
    parent: tree_sitter::Node<'_>,
    inherited: UiScope,
    source: &[u8],
    syntax_regions: &[SyntaxRegion],
) -> UiScope {
    if syntax_regions.iter().any(|region| {
        region.component_scope == ComponentScopePolicy::Exclude
            && region.body.is_some_and(|body| body.contains_node(child))
    }) {
        return UiScope::NonUi;
    }
    if syntax_regions.iter().any(|region| {
        region.component_scope == ComponentScopePolicy::ComposableLambda
            && region.body.is_some_and(|body| body.contains_node(child))
    }) {
        return UiScope::ComposableLambda;
    }
    if parent.kind() == "function_declaration" {
        if has_composable_annotation(parent, source) && !has_preview_annotation(parent, source) {
            return UiScope::Composable;
        }
        if function_name_from_decl(parent, source).is_some() {
            return UiScope::NonUi;
        }
    }
    inherited
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors scan_repository extraction call sites; args are distinct inputs"
)]
fn extract_usage_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    registry: &RegistryIndex,
    local_index: &LocalComposableIndex,
    syntax_regions: &[SyntaxRegion],
    clean: &[ByteRange],
    usage_sites: &mut Vec<UsageSite>,
) {
    let package = package_name_from_source(root, source);
    let imports = collect_import_bindings(root, source);
    let mut ctx = ComponentUsageCtx {
        source,
        file,
        package: package.as_deref(),
        registry,
        local_index,
        imports: &imports,
        syntax_regions,
        clean,
        usage_sites,
    };
    visit_component_usage(root, UiScope::NonUi, &mut ctx);
}

struct ComponentUsageCtx<'a> {
    source: &'a [u8],
    file: &'a str,
    package: Option<&'a str>,
    registry: &'a RegistryIndex,
    local_index: &'a LocalComposableIndex,
    imports: &'a ImportBindings,
    syntax_regions: &'a [SyntaxRegion],
    clean: &'a [ByteRange],
    usage_sites: &'a mut Vec<UsageSite>,
}

fn visit_component_usage(
    node: tree_sitter::Node<'_>,
    scope: UiScope,
    ctx: &mut ComponentUsageCtx<'_>,
) {
    if node.kind() == "call_expression"
        && scope.is_ui()
        && node_is_extractable(node, ctx.clean)
        && let Some((call_symbol, pos)) = call_simple_callee(node, ctx.source)
        && is_pascal_case_composable_symbol(&call_symbol)
        && !is_within_preview_composable(node, ctx.source)
        && !is_non_ui_scaffolding_composable_symbol(&call_symbol)
    {
        let line = pos.row as u32 + 1;
        let column = pos.column as u32 + 1;
        let location = SourceLocation {
            file: ctx.file.to_owned(),
            line,
            column: Some(column),
        };
        let parent = match scope {
            UiScope::Composable => {
                nearest_enclosing_composable(node, ctx.source).map(|(name, parent_pos)| {
                    parent_scope_for_composable(ctx.file, ctx.package, &name, parent_pos)
                })
            }
            UiScope::ComposableLambda | UiScope::NonUi => None,
        };

        let import_package = ctx.imports.package_for_symbol(&call_symbol);
        let imported_symbol = ctx
            .imports
            .symbol_names
            .get(&call_symbol)
            .unwrap_or(&call_symbol);
        if let Some(local) = ctx
            .local_index
            .same_file(ctx.file, &call_symbol)
            .or_else(|| {
                ctx.local_index
                    .qualified_package(import_package, imported_symbol)
            })
            .or_else(|| ctx.local_index.current_package(ctx.package, &call_symbol))
        {
            ctx.usage_sites.push(UsageSite {
                id: format!("usage.compose:{}:{line}:{column}:{call_symbol}", ctx.file),
                location,
                symbol: call_symbol.clone(),
                qualified_symbol: local.qualified_symbol.clone(),
                callee_origin: CalleeOrigin::Local,
                resolution_evidence: ResolutionEvidence {
                    kind: if ctx.local_index.same_file(ctx.file, &call_symbol).is_some() {
                        ResolutionEvidenceKind::LocalSameFile
                    } else {
                        ResolutionEvidenceKind::LocalPackageMatch
                    },
                    package: import_package.map(str::to_owned),
                },
                match_status: MatchStatus::Local,
                registry_symbol: None,
                local_definition_id: Some(local.id.clone()),
                parent,
            });
        } else {
            let registry_target = ctx.registry.resolve_targets.get(&call_symbol);
            let registry_match = registry_target.map(|registry_symbol| {
                resolve_registry_match(&call_symbol, registry_symbol, ctx.registry, ctx.imports)
            });
            {
                let (match_status, registry_symbol, callee_origin, resolution_evidence) =
                    match registry_match {
                        Some(RegistryImportMatch::Resolved) => (
                            MatchStatus::Resolved,
                            registry_target.cloned(),
                            CalleeOrigin::Registry,
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::RegistryPackageMatch,
                                package: import_package.map(str::to_owned),
                            },
                        ),
                        Some(RegistryImportMatch::Candidate) => (
                            MatchStatus::Candidate,
                            registry_target.cloned(),
                            CalleeOrigin::Registry,
                            ResolutionEvidence {
                                kind: if ctx.imports.wildcard_packages.len() > 1
                                    && !ctx.imports.symbol_packages.contains_key(&call_symbol)
                                {
                                    ResolutionEvidenceKind::RegistryImportAmbiguous
                                } else {
                                    ResolutionEvidenceKind::RegistryImportMissing
                                },
                                package: import_package.map(str::to_owned),
                            },
                        ),
                        Some(RegistryImportMatch::Mismatch) => (
                            MatchStatus::Unresolved,
                            None,
                            unresolved_origin(import_package),
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::PackageMismatch,
                                package: import_package.map(str::to_owned),
                            },
                        ),
                        Some(RegistryImportMatch::LegacyNameOnly) => (
                            MatchStatus::Resolved,
                            registry_target.cloned(),
                            CalleeOrigin::Registry,
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::RegistryNameOnlyLegacy,
                                package: None,
                            },
                        ),
                        None => (
                            MatchStatus::Unresolved,
                            None,
                            unresolved_origin_for_symbol(&call_symbol, import_package),
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::NoMatchingDefinition,
                                package: import_package.map(str::to_owned),
                            },
                        ),
                    };
                let qualified_symbol = import_package
                    .map(|package| qualified_composable_symbol(Some(package), imported_symbol));
                ctx.usage_sites.push(UsageSite {
                    id: format!("usage.compose:{}:{line}:{column}:{call_symbol}", ctx.file),
                    location,
                    symbol: call_symbol,
                    qualified_symbol,
                    callee_origin,
                    resolution_evidence,
                    match_status,
                    registry_symbol,
                    local_definition_id: None,
                    parent,
                });
            }
        }
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            let child_scope =
                component_scope_for_child(child, node, scope, ctx.source, ctx.syntax_regions);
            visit_component_usage(child, child_scope, ctx);
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
    clean: &[ByteRange],
    syntax_regions: &[SyntaxRegion],
    out: &mut Vec<HardcodedStyleSite>,
) {
    let package = package_name_from_source(root, source);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node_in_type_annotation_range(node, syntax_regions) {
            continue;
        }
        if node.kind() == "call_expression"
            && node_is_extractable(node, clean)
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
    clean: &[ByteRange],
    syntax_regions: &[SyntaxRegion],
    out: &mut Vec<TokenSite>,
) {
    let package = package_name_from_source(root, source);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node_in_type_annotation_range(node, syntax_regions) {
            continue;
        }
        if is_token_reference_node(node)
            && node_is_extractable(node, clean)
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
    let clean = [ByteRange {
        start: 0,
        end: source.len(),
    }];
    let mut local_index = LocalComposableIndex::default();
    for local in index_local_components_from_source(root, source, file, &clean) {
        local_index.insert(file, local.clone());
        local_components.push(local);
    }
    extract_usage_from_source(
        root,
        source,
        file,
        registry,
        &local_index,
        &[],
        &clean,
        usage_sites,
    );
}

// ── Public scan entry point ───────────────────────────────────────────────────

/// Runs the tree-sitter Compose scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &ComposeScanConfig,
) -> Result<TreeSitterScanResult, TreeSitterScanError> {
    scan_repository_with_parser(repo_root, config, parse_kotlin_file_permissive)
}

type ParseFileFn = fn(
    &mut tree_sitter::Parser,
    &Path,
) -> Result<crate::kotlin_ast::ParsedKotlinFile, ParseKotlinFileError>;

fn scan_repository_with_parser(
    repo_root: &Path,
    config: &ComposeScanConfig,
    parse_file: ParseFileFn,
) -> Result<TreeSitterScanResult, TreeSitterScanError> {
    let mut parser = new_parser().map_err(|error| TreeSitterScanError::ParserInitFailed {
        reason: error.to_string(),
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
                code: root_not_found_code(resolved.kind).to_owned(),
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

        match parse_file(&mut parser, file_path) {
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
    let mut local_with_priority = Vec::new();
    for (relative_file, parsed) in &parsed_files {
        for pass in parsed.passes() {
            for local in index_local_components_from_source(
                pass.tree.root_node(),
                parsed.source.as_bytes(),
                relative_file,
                &pass.clean,
            ) {
                local_index.insert(relative_file, local.clone());
                local_with_priority.push((pass.priority, local));
            }
        }
    }

    let mut usage_with_priority = Vec::new();
    let mut token_with_priority = Vec::new();
    let mut style_with_priority = Vec::new();
    for (relative_file, parsed) in &parsed_files {
        for pass in parsed.passes() {
            let mut pass_usages = Vec::new();
            extract_usage_from_source(
                pass.tree.root_node(),
                parsed.source.as_bytes(),
                relative_file,
                &registry,
                &local_index,
                &parsed.syntax_regions,
                &pass.clean,
                &mut pass_usages,
            );
            usage_with_priority.extend(pass_usages.into_iter().map(|fact| (pass.priority, fact)));

            let mut pass_styles = Vec::new();
            extract_hardcoded_style_from_source(
                pass.tree.root_node(),
                parsed.source.as_bytes(),
                relative_file,
                &pass.clean,
                &parsed.syntax_regions,
                &mut pass_styles,
            );
            style_with_priority.extend(pass_styles.into_iter().map(|fact| (pass.priority, fact)));

            let mut pass_tokens = Vec::new();
            extract_token_sites_from_source(
                pass.tree.root_node(),
                parsed.source.as_bytes(),
                relative_file,
                &registry.token_index,
                &pass.clean,
                &parsed.syntax_regions,
                &mut pass_tokens,
            );
            token_with_priority.extend(pass_tokens.into_iter().map(|fact| (pass.priority, fact)));
        }
    }

    let mut local_components = retain_first_by_priority_id(local_with_priority, |fact| &fact.id);
    let mut usage_sites = retain_first_by_priority_id(usage_with_priority, |fact| &fact.id);
    let mut token_sites = retain_first_by_priority_id(token_with_priority, |fact| &fact.id);
    let mut hardcoded_style_sites =
        retain_first_by_priority_id(style_with_priority, |fact| &fact.id);

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

fn retain_first_by_priority_id<T>(mut facts: Vec<(u16, T)>, id: impl Fn(&T) -> &str) -> Vec<T> {
    facts.sort_by_key(|(priority, _)| *priority);
    let mut unique = BTreeMap::new();
    for (_, fact) in facts {
        unique.entry(id(&fact).to_owned()).or_insert(fact);
    }
    unique.into_values().collect()
}

fn map_root_resolution_error(err: RootResolutionError) -> TreeSitterScanError {
    match err {
        RootResolutionError::Io { context, source } => TreeSitterScanError::Io { context, source },
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kotlin_recovery::normalize_kotlin_for_parse;

    fn temp_scan_repo() -> (tempfile::TempDir, ComposeScanConfig) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let registry_dir = tmp.path().join("design-system");
        std::fs::create_dir_all(&registry_dir).expect("create registry dir");
        std::fs::write(
            registry_dir.join("registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.btn","symbol":"PrimaryButton"}]}"#,
        )
        .expect("write registry");
        std::fs::create_dir_all(tmp.path().join("app/src/main/kotlin")).expect("create source dir");
        let config = ComposeScanConfig {
            design_system_registry: std::path::PathBuf::from("design-system/registry.json"),
            roots: vec![std::path::PathBuf::from("app/src/main/kotlin")],
            excludes: vec![],
        };
        (tmp, config)
    }

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

    fn registry_with_package(symbol: &str, package: &str) -> RegistryIndex {
        RegistryIndex {
            canonical_symbols: vec![symbol.to_owned()],
            resolve_targets: BTreeMap::from([(symbol.to_owned(), symbol.to_owned())]),
            component_packages: BTreeMap::from([(symbol.to_owned(), Some(package.to_owned()))]),
            tokens: Vec::new(),
            token_index: RegistryTokenIndex::default(),
        }
    }

    fn parse_and_extract(
        source: &str,
        registry: &RegistryIndex,
    ) -> (Vec<LocalComponent>, Vec<UsageSite>) {
        let normalized = normalize_kotlin_for_parse(source);
        let mut parser = make_parser();
        let tree = parser.parse(normalized.bytes.as_slice(), None).unwrap();
        let clean = [ByteRange {
            start: 0,
            end: source.len(),
        }];
        let root = tree.root_node();
        let bytes = source.as_bytes();
        let mut locals = Vec::new();
        let mut usages = Vec::new();
        let mut local_index = LocalComposableIndex::default();
        for local in index_local_components_from_source(root, bytes, "Test.kt", &clean) {
            local_index.insert("Test.kt", local.clone());
            locals.push(local);
        }
        extract_usage_from_source(
            root,
            bytes,
            "Test.kt",
            registry,
            &local_index,
            &normalized.regions,
            &clean,
            &mut usages,
        );
        (locals, usages)
    }

    fn usage_symbols(usages: &[UsageSite]) -> Vec<&str> {
        usages.iter().map(|usage| usage.symbol.as_str()).collect()
    }

    #[test]
    fn component_calls_require_ui_scope() {
        let registry = registry_without_packages(&[
            ("PrimaryButton", "PrimaryButton"),
            ("UnknownCard", "UnknownCard"),
        ]);

        let (_, ordinary) =
            parse_and_extract("fun helper() { PrimaryButton(); UnknownCard() }", &registry);
        assert!(
            ordinary.is_empty(),
            "ordinary functions are NonUi: {ordinary:?}"
        );

        let (_, property) = parse_and_extract("val boot = PrimaryButton()", &registry);
        assert!(
            property.is_empty(),
            "top-level property initializers are NonUi: {property:?}"
        );

        let (_, composable) = parse_and_extract(
            "@Composable\nfun Screen() { PrimaryButton(); UnknownCard() }",
            &registry,
        );
        assert_eq!(
            usage_symbols(&composable),
            vec!["PrimaryButton", "UnknownCard"]
        );
        assert!(
            composable.iter().all(|site| {
                site.parent.as_ref().map(|parent| parent.symbol.as_str()) == Some("Screen")
            }),
            "declaration-backed Composable parents must stay Screen: {composable:?}"
        );

        let (_, nested_lambda) = parse_and_extract(
            "@Composable\nfun Screen() { list.forEach { PrimaryButton() } }",
            &registry,
        );
        assert_eq!(usage_symbols(&nested_lambda), vec!["PrimaryButton"]);
        assert_eq!(
            nested_lambda[0]
                .parent
                .as_ref()
                .map(|parent| parent.symbol.as_str()),
            Some("Screen"),
            "ordinary nested lambdas keep nearest enclosing composable parent"
        );

        let (_, nested_fun) = parse_and_extract(
            "@Composable\nfun Screen() { fun load() { UnknownCard() } }",
            &registry,
        );
        assert!(
            nested_fun.is_empty(),
            "nested named functions do not inherit UI scope: {nested_fun:?}"
        );

        let (_, slot_lambda) = parse_and_extract(
            "val content: @Composable (() -> Unit) = { PrimaryButton() }",
            &registry,
        );
        assert_eq!(usage_symbols(&slot_lambda), vec!["PrimaryButton"]);
        assert!(
            slot_lambda[0].parent.is_none(),
            "top-level annotated composable property lambda has no synthetic parent: {slot_lambda:?}"
        );

        let (_, suspend_body) = parse_and_extract(
            "val loader = suspend { FetchRepository(); PrimaryButton() }",
            &registry,
        );
        assert!(
            suspend_body.is_empty(),
            "suspend lambda bodies are Exclude for components: {suspend_body:?}"
        );

        let (_, field_init) = parse_and_extract(
            "class Holder {\n    val state: StateFlow<List<Item>>\n        field = MutableStateFlow(emptyList())\n}\n",
            &registry,
        );
        assert!(
            field_init
                .iter()
                .all(|site| site.symbol != "MutableStateFlow"),
            "explicit field initializers must not emit unresolved UI calls: {field_init:?}"
        );

        let (_, when_guard) = parse_and_extract(
            "@Composable\nfun Screen(item: Any) {\n    when (item) {\n        is Visible if enabled -> PrimaryButton()\n        else -> Unit\n    }\n}\n",
            &registry,
        );
        assert_eq!(usage_symbols(&when_guard), vec!["PrimaryButton"]);

        let (_, context_param) = parse_and_extract(
            "context(scope: Scope)\n@Composable\nfun Screen() { PrimaryButton() }\n",
            &registry,
        );
        assert_eq!(usage_symbols(&context_param), vec!["PrimaryButton"]);

        let token_index = token_match_index(&[
            (
                "token.color",
                "AppTokens.color.primary",
                TokenCategory::Color,
            ),
            ("token.spacing", "Spacing.small", TokenCategory::Spacing),
        ]);
        let token_source = r#"
val color = AppTokens.color.primary
class Holder {
    val spacing: Dp
        field = Spacing.small
    val modifier: Modifier
        field = Modifier.padding(7.dp)
}
"#;
        let tokens = extract_token_sites(token_source, &token_index);
        assert!(
            tokens
                .iter()
                .any(|site| site.key == "AppTokens.color.primary"),
            "tokens outside composables remain extractable: {tokens:?}"
        );
        assert!(
            tokens.iter().any(|site| site.key == "Spacing.small"),
            "tokens inside explicit-field initializers remain extractable: {tokens:?}"
        );
        let styles = extract_hardcoded_styles(token_source);
        assert!(
            styles
                .iter()
                .any(|site| site.value == "7.dp" && site.category == TokenCategory::Spacing),
            "hard-coded styles inside explicit-field initializers remain extractable: {styles:?}"
        );
        let (_, token_usages) = parse_and_extract(token_source, &registry);
        assert!(
            token_usages.is_empty(),
            "token/style independence cases must not create component usages: {token_usages:?}"
        );
    }

    #[test]
    fn malformed_non_ui_declaration_does_not_create_local_component() {
        let registry = registry_without_packages(&[]);
        let (locals, _) =
            parse_and_extract("@Composable\nfun Screen() {}\nfun Broken(\n", &registry);
        assert!(
            locals.iter().any(|local| local.symbol == "Screen"),
            "earlier UI declarations should still index: {locals:?}"
        );
        assert!(
            locals.iter().all(|local| local.symbol != "Broken"),
            "malformed non-UI declarations must not become local components: {locals:?}"
        );
    }

    #[test]
    fn composable_function_type_properties_are_not_indexed_as_locals() {
        let registry = registry_without_packages(&[]);
        let (locals, _) = parse_and_extract(
            "val content: @Composable (() -> Unit) = { }\n@Composable\nfun Screen() {}\n",
            &registry,
        );
        assert_eq!(
            locals
                .iter()
                .map(|local| local.symbol.as_str())
                .collect::<Vec<_>>(),
            vec!["Screen"]
        );
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
    fn same_file_local_component_wins_over_registry_name_collision() {
        let registry = registry_with_package("Button", "com.acme.designsystem");
        let (_, usages) = parse_and_extract(
            "@Composable\nfun Button() {}\n@Composable\nfun Screen() { Button() }",
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Local);
        assert_eq!(usages[0].registry_symbol, None);
        assert_eq!(
            usages[0].resolution_evidence.kind,
            ResolutionEvidenceKind::LocalSameFile
        );
    }

    #[test]
    fn package_mismatch_keeps_a_qualified_unresolved_usage() {
        let registry = registry_with_package("Button", "com.acme.designsystem");
        let (_, usages) = parse_and_extract(
            "import com.other.widgets.Button\n@Composable\nfun Screen() { Button() }",
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
        assert_eq!(usages[0].registry_symbol, None);
        assert_eq!(
            usages[0].qualified_symbol.as_deref(),
            Some("com.other.widgets.Button")
        );
        assert_eq!(
            usages[0].resolution_evidence.kind,
            ResolutionEvidenceKind::PackageMismatch
        );
        assert_eq!(
            usages[0].resolution_evidence.package.as_deref(),
            Some("com.other.widgets")
        );
    }

    #[test]
    fn package_mismatch_keeps_the_imported_symbol_for_an_alias() {
        let mut registry = registry_with_package("Button", "com.acme.designsystem");
        registry
            .resolve_targets
            .insert("DsButton".to_owned(), "Button".to_owned());
        let (_, usages) = parse_and_extract(
            "import com.other.widgets.RealButton as DsButton\n@Composable\nfun Screen() { DsButton() }",
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
        assert_eq!(
            usages[0].qualified_symbol.as_deref(),
            Some("com.other.widgets.RealButton")
        );
        assert_eq!(
            usages[0].resolution_evidence.kind,
            ResolutionEvidenceKind::PackageMismatch
        );
    }

    #[test]
    fn matching_named_alias_and_wildcard_imports_resolve_with_qualified_symbols() {
        let registry = RegistryIndex {
            canonical_symbols: vec!["Button".to_owned()],
            resolve_targets: BTreeMap::from([
                ("Button".to_owned(), "Button".to_owned()),
                ("DsButton".to_owned(), "Button".to_owned()),
            ]),
            component_packages: BTreeMap::from([(
                "Button".to_owned(),
                Some("com.acme.designsystem".to_owned()),
            )]),
            tokens: Vec::new(),
            token_index: RegistryTokenIndex::default(),
        };
        let (_, usages) = parse_and_extract(
            "import com.acme.designsystem.Button as DsButton\nimport com.acme.designsystem.*\n@Composable\nfun Screen() { DsButton(); Button() }",
            &registry,
        );

        assert_eq!(usages.len(), 2);
        assert_eq!(usages[0].match_status, MatchStatus::Resolved);
        assert_eq!(
            usages[0].qualified_symbol.as_deref(),
            Some("com.acme.designsystem.Button")
        );
        assert_eq!(usages[1].match_status, MatchStatus::Resolved);
        assert_eq!(
            usages[1].qualified_symbol.as_deref(),
            Some("com.acme.designsystem.Button")
        );
        assert!(usages.iter().all(|usage| {
            usage.resolution_evidence.kind == ResolutionEvidenceKind::RegistryPackageMatch
        }));
    }

    #[test]
    fn ambiguous_wildcard_import_is_a_candidate_with_no_qualified_symbol() {
        let registry = registry_with_package("Button", "com.acme.designsystem");
        let (_, usages) = parse_and_extract(
            "import com.acme.designsystem.*\nimport com.other.widgets.*\n@Composable\nfun Screen() { Button() }",
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Candidate);
        assert_eq!(usages[0].qualified_symbol, None);
        assert_eq!(
            usages[0].resolution_evidence.kind,
            ResolutionEvidenceKind::RegistryImportAmbiguous
        );
    }

    #[test]
    fn imported_local_component_resolves_by_imported_package() {
        let registry = registry_without_packages(&[]);
        let mut parser = make_parser();
        let local_source = "package feature\n@Composable\nfun LocalCard() {}";
        let local_tree = parser
            .parse(local_source.as_bytes(), None)
            .expect("parse local");
        let clean = [ByteRange {
            start: 0,
            end: local_source.len(),
        }];
        let local = index_local_components_from_source(
            local_tree.root_node(),
            local_source.as_bytes(),
            "feature/Local.kt",
            &clean,
        )[0]
        .clone();
        let mut local_index = LocalComposableIndex::default();
        local_index.insert("feature/Local.kt", local.clone());

        let source =
            "package app\nimport feature.LocalCard\n@Composable\nfun Screen() { LocalCard() }";
        let tree = parser.parse(source.as_bytes(), None).expect("parse caller");
        let mut usages = Vec::new();
        extract_usage_from_source(
            tree.root_node(),
            source.as_bytes(),
            "app/Screen.kt",
            &registry,
            &local_index,
            &[],
            &[ByteRange {
                start: 0,
                end: source.len(),
            }],
            &mut usages,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Local);
        assert_eq!(
            usages[0].local_definition_id.as_deref(),
            Some(local.id.as_str())
        );
    }

    #[test]
    fn imported_local_component_resolves_with_import_alias() {
        let registry = registry_without_packages(&[]);
        let mut parser = make_parser();
        let local_source = "package feature\n@Composable\nfun LocalCard() {}";
        let local_tree = parser
            .parse(local_source.as_bytes(), None)
            .expect("parse local");
        let clean = [ByteRange {
            start: 0,
            end: local_source.len(),
        }];
        let local = index_local_components_from_source(
            local_tree.root_node(),
            local_source.as_bytes(),
            "feature/Local.kt",
            &clean,
        )[0]
        .clone();
        let mut local_index = LocalComposableIndex::default();
        local_index.insert("feature/Local.kt", local.clone());

        let source =
            "package app\nimport feature.LocalCard as Card\n@Composable\nfun Screen() { Card() }";
        let tree = parser.parse(source.as_bytes(), None).expect("parse caller");
        let caller_clean = [ByteRange {
            start: 0,
            end: source.len(),
        }];
        let mut usages = Vec::new();
        extract_usage_from_source(
            tree.root_node(),
            source.as_bytes(),
            "app/Screen.kt",
            &registry,
            &local_index,
            &[],
            &caller_clean,
            &mut usages,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Local);
        assert_eq!(
            usages[0].local_definition_id.as_deref(),
            Some(local.id.as_str())
        );
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
    fn non_ds_import_is_unresolved_when_package_is_configured() {
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
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
    }

    #[test]
    fn framework_subpackage_import_is_unresolved_when_package_is_configured() {
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
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
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
    fn third_party_import_is_unresolved_when_package_is_configured() {
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
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
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
    fn primary_local_wins_over_earlier_recovered_local_with_same_id() {
        let (tmp, config) = temp_scan_repo();
        let source_dir = tmp.path().join("app/src/main/kotlin");
        std::fs::write(
            source_dir.join("RecoveredFirst.kt"),
            "@Composable\nfun BeforeGap() {\n    PrimaryButton(onClick = {})\n}\nfun Broken() = ()\n@Composable\nfun SharedScreen() {\n    PrimaryButton(onClick = {})\n}\n",
        )
        .expect("write recovered-first source");
        std::fs::write(
            source_dir.join("PrimaryLater.kt"),
            "@Composable\nfun SharedScreen() {\n    PrimaryButton(onClick = {})\n}\n",
        )
        .expect("write primary-later source");

        let result = scan_repository(tmp.path(), &config).expect("scan should succeed");
        let shared = result
            .local_components
            .iter()
            .filter(|component| component.symbol == "SharedScreen")
            .collect::<Vec<_>>();
        assert_eq!(
            shared.len(),
            1,
            "duplicate local ids must collapse to one fact: {shared:?}"
        );
        assert!(
            shared[0].location.file.ends_with("PrimaryLater.kt"),
            "lower-priority recovered local must not beat a later primary: {:?}",
            shared[0]
        );
    }

    #[test]
    fn no_tree_file_is_skipped_while_neighbors_still_scan() {
        let (tmp, config) = temp_scan_repo();
        let source_dir = tmp.path().join("app/src/main/kotlin");
        std::fs::write(source_dir.join("NoTree.kt"), "fun skipped() {}\n").expect("write no-tree");
        std::fs::write(
            source_dir.join("Partial.kt"),
            "@Composable\nfun BeforeGap() {\n    PrimaryButton(onClick = {})\n}\nfun Broken() = ()\n@Composable\nfun AfterGap() {\n    PrimaryButton(onClick = {})\n}\n",
        )
        .expect("write partial");
        std::fs::write(
            source_dir.join("Valid.kt"),
            "@Composable\nfun Screen() {\n    PrimaryButton(onClick = {})\n}\n",
        )
        .expect("write valid");

        fn parse_file_skipping_no_tree(
            parser: &mut tree_sitter::Parser,
            path: &Path,
        ) -> Result<crate::kotlin_ast::ParsedKotlinFile, ParseKotlinFileError> {
            if path.file_name().and_then(|name| name.to_str()) == Some("NoTree.kt") {
                return Err(ParseKotlinFileError::ParseFailed(path.to_path_buf()));
            }
            parse_kotlin_file_permissive(parser, path)
        }

        let result = scan_repository_with_parser(tmp.path(), &config, parse_file_skipping_no_tree)
            .expect("repository scan isolates no-tree failures");

        assert_eq!(result.files_scanned, 3);
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "parse_failed"
                    && diagnostic
                        .location
                        .as_ref()
                        .is_some_and(|location| location.file.ends_with("NoTree.kt"))
                    && diagnostic.message.contains("file skipped")
            }),
            "no-tree file must be skipped with a diagnostic: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .local_components
                .iter()
                .any(|component| component.symbol == "AfterGap"),
            "malformed neighbor must still recover later facts: {:?}",
            result.local_components
        );
        assert!(
            result
                .local_components
                .iter()
                .any(|component| component.symbol == "Screen"),
            "valid neighbor must still scan: {:?}",
            result.local_components
        );
        assert_eq!(result.status, ScanStatus::Partial);
    }

    #[test]
    fn filesystem_read_errors_abort_the_scan() {
        let (tmp, config) = temp_scan_repo();
        std::fs::write(tmp.path().join("app/src/main/kotlin/Unreadable.kt"), "")
            .expect("write source");

        fn parse_file_io_error(
            _parser: &mut tree_sitter::Parser,
            path: &Path,
        ) -> Result<crate::kotlin_ast::ParsedKotlinFile, ParseKotlinFileError> {
            Err(ParseKotlinFileError::Io {
                context: format!("read Kotlin source {}", path.display()),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
            })
        }

        let err = scan_repository_with_parser(tmp.path(), &config, parse_file_io_error)
            .expect_err("real filesystem errors must abort the scan");
        assert!(
            matches!(err, TreeSitterScanError::Io { .. }),
            "expected Io abort, got {err:?}"
        );
    }

    #[test]
    fn annotated_function_type_positions_do_not_emit_parse_failed() {
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

val handler: @Composable ((NavArgument) -> Unit) = {}
fun handlerFactory(): @Composable ((NavArgument) -> Unit) = {}

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
        let normalized = normalize_kotlin_for_parse(source);
        let mut parser = make_parser();
        let tree = parser.parse(normalized.bytes.as_slice(), None).unwrap();
        let clean = [ByteRange {
            start: 0,
            end: source.len(),
        }];
        let mut sites = Vec::new();
        extract_hardcoded_style_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.kt",
            &clean,
            &normalized.regions,
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
        let normalized = normalize_kotlin_for_parse(source);
        let mut parser = make_parser();
        let tree = parser.parse(normalized.bytes.as_slice(), None).unwrap();
        let clean = [ByteRange {
            start: 0,
            end: source.len(),
        }];
        let mut sites = Vec::new();
        extract_token_sites_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Test.kt",
            index,
            &clean,
            &normalized.regions,
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
