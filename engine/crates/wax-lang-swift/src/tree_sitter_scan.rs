//! Tree-sitter-swift backed SwiftUI scanner.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use wax_contract::{
    CalleeOrigin, DesignSystemComponent, DesignSystemToken, Diagnostic, DiagnosticSeverity,
    HardcodedStyleSite, IdentityStability, LocalComponent, MatchStatus, ParentScope,
    ResolutionEvidence, ResolutionEvidenceKind, ScanStatus, SourceLocation, StyleContext,
    TokenCategory, TokenSite, UsageSite,
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
    RegistryImportMatch, RegistryTokenIndex, RootResolutionError, ScanConfig,
    normalize_repo_relative_path, parse_registry_tokens, path_matches_any,
    resolve_import_aware_match, resolve_source_roots, root_not_found_code, root_not_found_message,
    swift_module_from_source_path, token_index,
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
    let has_registry = config.contains_key("registry");
    let has_roots = config.contains_key("roots");
    let has_excludes = config.contains_key("excludes");
    if !has_registry && !has_roots && !has_excludes {
        return Ok(SwiftConfigMode::Scaffold);
    }

    let registry = config
        .get("registry")
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
    /// Known design-system tokens from the registry file.
    pub design_system_tokens: Vec<DesignSystemToken>,
    /// Known token references matched in source.
    pub token_sites: Vec<TokenSite>,
    /// Hard-coded styling candidates discovered in source.
    pub hardcoded_style_sites: Vec<HardcodedStyleSite>,
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
    tokens: Vec<DesignSystemToken>,
    token_index: RegistryTokenIndex,
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

fn resolve_registry_match(
    call_symbol: &str,
    call_qualifier: Option<&str>,
    registry_symbol: &str,
    registry: &RegistryIndex,
    imports: &ImportBindings,
) -> RegistryImportMatch {
    resolve_import_aware_match(
        registry
            .component_packages
            .get(registry_symbol)
            .and_then(|package| package.as_deref()),
        imports
            .package_for_call(call_symbol, call_qualifier)
            .as_deref(),
    )
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
    scanned_modules: BTreeSet<String>,
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

    fn same_file(&self, file: &str, symbol: &str) -> Option<&LocalComponent> {
        self.by_file_symbol
            .get(&(file.to_owned(), symbol.to_owned()))
    }

    fn qualified_module(&self, module: Option<&str>, symbol: &str) -> Option<&LocalComponent> {
        let module = module?;
        let qualified = qualified_view_symbol(module, symbol);
        self.by_qualified.get(&qualified)
    }

    fn current_module(&self, module: &str, symbol: &str) -> Option<&LocalComponent> {
        self.qualified_module(Some(module), symbol)
    }
}

fn is_framework_swiftui_module(module: &str) -> bool {
    module == "SwiftUI"
}

fn is_framework_swiftui_symbol(symbol: &str) -> bool {
    matches!(
        symbol,
        "AnyView"
            | "Button"
            | "Capsule"
            | "Circle"
            | "Color"
            | "Divider"
            | "Ellipse"
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
            | "Rectangle"
            | "RoundedRectangle"
            | "ScrollView"
            | "Section"
            | "Slider"
            | "Spacer"
            | "Text"
            | "TextField"
            | "Toggle"
            | "UnevenRoundedRectangle"
            | "VStack"
            | "ZStack"
    )
}

fn is_scanned_swift_module(module: &str, scanned_modules: &BTreeSet<String>) -> bool {
    scanned_modules.contains(module)
}

fn has_explicit_swiftui_framework_evidence(
    package: Option<&str>,
    symbol: &str,
    qualifier: Option<&str>,
    selective_swiftui_import: bool,
) -> bool {
    package.is_some_and(is_framework_swiftui_module)
        && (qualifier == Some("SwiftUI")
            || selective_swiftui_import
            || is_framework_swiftui_symbol(symbol))
}

fn unresolved_origin(
    package: Option<&str>,
    symbol: &str,
    explicit_swiftui_framework: bool,
    swiftui_imported: bool,
    current_module_known: bool,
    scanned_modules: &BTreeSet<String>,
    identity_ambiguous: bool,
) -> CalleeOrigin {
    match package {
        Some(_) if explicit_swiftui_framework => CalleeOrigin::Framework,
        None if swiftui_imported && is_framework_swiftui_symbol(symbol) => CalleeOrigin::Framework,
        Some(package) if is_scanned_swift_module(package, scanned_modules) => {
            CalleeOrigin::Application
        }
        // Sole-module `import SwiftUI` can weakly attribute uncatalogued symbols to SwiftUI.
        // Prefer application/unknown over framework so those calls stay adoption-eligible.
        Some(package)
            if is_framework_swiftui_module(package)
                && current_module_known
                && !identity_ambiguous =>
        {
            CalleeOrigin::Application
        }
        Some(package) if is_framework_swiftui_module(package) => CalleeOrigin::Unknown,
        Some(_) => CalleeOrigin::External,
        None if current_module_known && !identity_ambiguous => CalleeOrigin::Application,
        None => CalleeOrigin::Unknown,
    }
}

fn import_identity_is_ambiguous(call_site: &ResolvedCallSite, imports: &ImportBindings) -> bool {
    call_site.qualifier.is_none()
        && !imports.symbol_packages.contains_key(&call_site.symbol)
        && imports.module_imports.len() > 1
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
            && !is_inside_preview_macro(node, source)
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

            let import_package =
                imports.package_for_call(&call_site.symbol, call_site.qualifier.as_deref());
            let unresolved_package = import_package.clone().or_else(|| {
                (!imports
                    .module_imports
                    .iter()
                    .any(|module| is_framework_swiftui_module(module))
                    || !is_framework_swiftui_symbol(&call_site.symbol))
                .then(|| imports.sole_non_swiftui_module())
                .flatten()
            });
            if let Some(local) = call_site
                .qualifier
                .is_none()
                .then(|| local_index.same_file(file, &call_site.symbol))
                .flatten()
                .or_else(|| {
                    local_index.qualified_module(import_package.as_deref(), &call_site.symbol)
                })
                .or_else(|| {
                    call_site
                        .qualifier
                        .is_none()
                        .then(|| local_index.current_module(&module_identity, &call_site.symbol))
                        .flatten()
                })
            {
                usage_sites.push(UsageSite {
                    id: format!("usage.swift:{file}:{line}:{column}:{}", call_site.symbol),
                    location: location.clone(),
                    symbol: call_site.symbol.clone(),
                    qualified_symbol: local.qualified_symbol.clone(),
                    callee_origin: CalleeOrigin::Local,
                    resolution_evidence: ResolutionEvidence {
                        kind: if call_site.qualifier.is_none()
                            && local_index.same_file(file, &call_site.symbol).is_some()
                        {
                            ResolutionEvidenceKind::LocalSameFile
                        } else {
                            ResolutionEvidenceKind::LocalPackageMatch
                        },
                        package: import_package.clone(),
                    },
                    match_status: MatchStatus::Local,
                    registry_symbol: None,
                    local_definition_id: Some(local.id.clone()),
                    parent,
                });
            } else if let Some(registry_symbol) = registry.resolve_targets.get(&call_site.symbol) {
                let registry_match = resolve_registry_match(
                    &call_site.symbol,
                    call_site.qualifier.as_deref(),
                    registry_symbol,
                    registry,
                    &imports,
                );
                let (match_status, registry_symbol, callee_origin, resolution_evidence) =
                    match registry_match {
                        RegistryImportMatch::Resolved => (
                            MatchStatus::Resolved,
                            Some(registry_symbol.clone()),
                            CalleeOrigin::Registry,
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::RegistryPackageMatch,
                                package: import_package.clone(),
                            },
                        ),
                        RegistryImportMatch::LegacyNameOnly => (
                            MatchStatus::Resolved,
                            Some(registry_symbol.clone()),
                            CalleeOrigin::Registry,
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::RegistryNameOnlyLegacy,
                                package: import_package.clone(),
                            },
                        ),
                        RegistryImportMatch::Candidate => (
                            MatchStatus::Candidate,
                            Some(registry_symbol.clone()),
                            CalleeOrigin::Registry,
                            ResolutionEvidence {
                                kind: if imports.module_imports.len() > 1
                                    && call_site.qualifier.is_none()
                                    && !imports.symbol_packages.contains_key(&call_site.symbol)
                                {
                                    ResolutionEvidenceKind::RegistryImportAmbiguous
                                } else {
                                    ResolutionEvidenceKind::RegistryImportMissing
                                },
                                package: import_package.clone(),
                            },
                        ),
                        RegistryImportMatch::Mismatch => (
                            MatchStatus::Unresolved,
                            None,
                            unresolved_origin(
                                unresolved_package.as_deref(),
                                &call_site.symbol,
                                has_explicit_swiftui_framework_evidence(
                                    import_package.as_deref(),
                                    &call_site.symbol,
                                    call_site.qualifier.as_deref(),
                                    imports.symbol_packages.get(&call_site.symbol).is_some_and(
                                        |package| is_framework_swiftui_module(package),
                                    ),
                                ),
                                imports
                                    .module_imports
                                    .iter()
                                    .any(|module| is_framework_swiftui_module(module)),
                                semantic_module,
                                &local_index.scanned_modules,
                                import_identity_is_ambiguous(&call_site, &imports),
                            ),
                            ResolutionEvidence {
                                kind: ResolutionEvidenceKind::PackageMismatch,
                                package: import_package.clone(),
                            },
                        ),
                    };
                usage_sites.push(UsageSite {
                    id: format!("usage.swift:{file}:{line}:{column}:{}", call_site.symbol),
                    location,
                    symbol: call_site.symbol.clone(),
                    qualified_symbol: import_package.as_deref().map(|package| {
                        qualified_view_symbol(
                            package,
                            registry_symbol.as_deref().unwrap_or(&call_site.symbol),
                        )
                    }),
                    callee_origin,
                    resolution_evidence,
                    match_status,
                    registry_symbol,
                    local_definition_id: None,
                    parent,
                });
            } else if parent.is_some() {
                let symbol = call_site.symbol.clone();
                usage_sites.push(UsageSite {
                    id: format!("usage.swift:{file}:{line}:{column}:{}", call_site.symbol),
                    location,
                    symbol: symbol.clone(),
                    qualified_symbol: import_package
                        .as_deref()
                        .map(|package| qualified_view_symbol(package, &symbol)),
                    callee_origin: unresolved_origin(
                        unresolved_package.as_deref(),
                        &symbol,
                        has_explicit_swiftui_framework_evidence(
                            import_package.as_deref(),
                            &symbol,
                            call_site.qualifier.as_deref(),
                            imports
                                .symbol_packages
                                .get(&symbol)
                                .is_some_and(|package| is_framework_swiftui_module(package)),
                        ),
                        imports
                            .module_imports
                            .iter()
                            .any(|module| is_framework_swiftui_module(module)),
                        semantic_module,
                        &local_index.scanned_modules,
                        import_identity_is_ambiguous(&call_site, &imports),
                    ),
                    resolution_evidence: ResolutionEvidence {
                        kind: ResolutionEvidenceKind::NoMatchingDefinition,
                        package: import_package.clone(),
                    },
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

fn is_inside_preview_macro(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if ancestor.kind() == "macro_invocation"
            && ancestor
                .utf8_text(source)
                .ok()
                .is_some_and(is_preview_macro_text)
        {
            return true;
        }
        current = ancestor.parent();
    }
    preview_macro_body_contains(node.start_byte(), source)
}

fn preview_macro_body_contains(offset: usize, source: &[u8]) -> bool {
    let marker = b"#Preview";
    let mut index = 0;
    let mut line_comment = false;
    let mut block_comment_depth = 0_u32;
    let mut string_literal = false;
    let mut escaped = false;
    while index < source.len() {
        let byte = source[index];
        if line_comment {
            line_comment = byte != b'\n';
            index += 1;
            continue;
        }
        if block_comment_depth > 0 {
            if source.get(index..index + 2) == Some(b"/*") {
                block_comment_depth += 1;
                index += 2;
            } else if source.get(index..index + 2) == Some(b"*/") {
                block_comment_depth -= 1;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if string_literal {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string_literal = false;
            }
            index += 1;
            continue;
        }
        if source.get(index..index + 2) == Some(b"//") {
            line_comment = true;
            index += 2;
            continue;
        }
        if source.get(index..index + 2) == Some(b"/*") {
            block_comment_depth = 1;
            index += 2;
            continue;
        }
        if byte == b'"' {
            string_literal = true;
            index += 1;
            continue;
        }
        if !source[index..].starts_with(marker) {
            index += 1;
            continue;
        }

        let start = index;
        let end = start + marker.len();
        let is_marker = source
            .get(end)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_');
        if !is_marker {
            index = end;
            continue;
        }

        let Some(relative_open_brace) = source[end..].iter().position(|byte| *byte == b'{') else {
            break;
        };
        let open_brace = end + relative_open_brace;
        let Some(close_brace) = matching_brace(source, open_brace) else {
            break;
        };
        if open_brace < offset && offset < close_brace {
            return true;
        }
        index = close_brace + 1;
    }
    false
}

fn matching_brace(source: &[u8], open_brace: usize) -> Option<usize> {
    let mut depth = 0_u32;
    let mut index = open_brace;
    let mut line_comment = false;
    let mut block_comment_depth = 0_u32;
    let mut string_literal = false;
    let mut escaped = false;

    while index < source.len() {
        let byte = source[index];
        if line_comment {
            line_comment = byte != b'\n';
            index += 1;
            continue;
        }
        if block_comment_depth > 0 {
            if source.get(index..index + 2) == Some(b"/*") {
                block_comment_depth += 1;
                index += 2;
            } else if source.get(index..index + 2) == Some(b"*/") {
                block_comment_depth -= 1;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if string_literal {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string_literal = false;
            }
            index += 1;
            continue;
        }

        if source.get(index..index + 2) == Some(b"//") {
            line_comment = true;
            index += 2;
        } else if source.get(index..index + 2) == Some(b"/*") {
            block_comment_depth = 1;
            index += 2;
        } else if byte == b'"' {
            string_literal = true;
            index += 1;
        } else if byte == b'{' {
            depth += 1;
            index += 1;
        } else if byte == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
            index += 1;
        } else {
            index += 1;
        }
    }
    None
}

fn is_preview_macro_text(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("#Preview")
        .is_some_and(|suffix| {
            suffix
                .as_bytes()
                .first()
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        })
}

fn swift_style_metadata(call_symbol: &str) -> Option<(TokenCategory, StyleContext)> {
    match call_symbol {
        "Color" | "foregroundStyle" | "foregroundColor" | "background" => {
            Some((TokenCategory::Color, StyleContext::Color))
        }
        "padding" => Some((TokenCategory::Spacing, StyleContext::Padding)),
        "frame" => Some((TokenCategory::Spacing, StyleContext::Size)),
        "spacing" => Some((TokenCategory::Spacing, StyleContext::Gap)),
        "font" | "fontWeight" => Some((TokenCategory::Typography, StyleContext::Typography)),
        "cornerRadius" | "clipShape" => Some((TokenCategory::Radius, StyleContext::Radius)),
        "shadow" => Some((TokenCategory::Elevation, StyleContext::Elevation)),
        _ => None,
    }
}

fn style_label_metadata(label: &str) -> Option<(TokenCategory, StyleContext)> {
    match label {
        "spacing" => Some((TokenCategory::Spacing, StyleContext::Gap)),
        "width" => Some((TokenCategory::Spacing, StyleContext::Width)),
        "height" => Some((TokenCategory::Spacing, StyleContext::Height)),
        "padding" | "leading" | "trailing" | "top" | "bottom" | "horizontal" | "vertical" => {
            Some((TokenCategory::Spacing, StyleContext::Padding))
        }
        "size" | "weight" | "pointSize" => {
            Some((TokenCategory::Typography, StyleContext::Typography))
        }
        "cornerRadius" | "radius" => Some((TokenCategory::Radius, StyleContext::Radius)),
        "blur" => Some((TokenCategory::Elevation, StyleContext::Elevation)),
        _ => None,
    }
}

struct HardcodedLiteral {
    value: String,
    category: TokenCategory,
    context: StyleContext,
    position: tree_sitter::Point,
    start_byte: usize,
    end_byte: usize,
}

fn extract_hardcoded_style_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    module_identity: &str,
    semantic_module: bool,
    out: &mut Vec<HardcodedStyleSite>,
) {
    let mut candidates = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_call_expression_node(node) && !is_inside_preview_macro(node, source) {
            collect_hardcoded_literals_from_call(node, source, &mut candidates);
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }

    dedupe_hardcoded_literals_by_longest_range(&mut candidates);

    for literal in candidates {
        let line = literal.position.row as u32 + 1;
        let column = literal.position.column as u32 + 1;
        let parent = nearest_enclosing_view_at_byte(root, source, literal.start_byte).map(
            |(name, parent_pos)| {
                parent_scope_for_view(file, module_identity, semantic_module, &name, parent_pos)
            },
        );
        out.push(HardcodedStyleSite {
            id: format!(
                "hardcoded.swift:{file}:{line}:{column}:{:?}",
                literal.category
            ),
            location: SourceLocation {
                file: file.to_owned(),
                line,
                column: Some(column),
            },
            value: literal.value,
            category: literal.category,
            context: literal.context,
            parent,
        });
    }
}

fn nearest_enclosing_view_at_byte(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    start_byte: usize,
) -> Option<(String, tree_sitter::Point)> {
    find_node_covering_byte(root, start_byte).and_then(|node| nearest_enclosing_view(node, source))
}

fn find_node_covering_byte(
    root: tree_sitter::Node<'_>,
    start_byte: usize,
) -> Option<tree_sitter::Node<'_>> {
    let mut best = None;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.start_byte() <= start_byte && start_byte < node.end_byte() {
            best = Some(node);
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    stack.push(child);
                }
            }
        }
    }
    best
}

fn collect_hardcoded_literals_from_call(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<HardcodedLiteral>,
) {
    let Some(call_site) = resolve_call_site(node, source) else {
        return;
    };

    // Color(...) is reported as the whole call expression, never individual components.
    if call_site.symbol == "Color" {
        if let Ok(text) = node.utf8_text(source) {
            out.push(HardcodedLiteral {
                value: text.to_owned(),
                category: TokenCategory::Color,
                context: StyleContext::Color,
                position: call_site.position,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
            });
        }
        return;
    }

    let callee_metadata = swift_style_metadata(&call_site.symbol);
    let allows_style_labels =
        callee_metadata.is_some() || is_swiftui_style_label_callee(&call_site.symbol);
    // Non-styling calls must not emit hard-coded style candidates from labeled args such as
    // `analytics.record(size: 14)`.
    if callee_metadata.is_none() && !allows_style_labels {
        return;
    }

    if let Some(arguments) = call_argument_node(node) {
        collect_style_literals_in_arguments(
            arguments,
            source,
            callee_metadata,
            allows_style_labels,
            out,
        );
    }
}

/// SwiftUI callees that accept layout/style labels even when they are not themselves style
/// modifiers (e.g. `VStack(spacing:)`, `RoundedRectangle(cornerRadius:)`, `.system(size:)`).
fn is_swiftui_style_label_callee(symbol: &str) -> bool {
    matches!(
        symbol,
        "VStack"
            | "HStack"
            | "ZStack"
            | "LazyVStack"
            | "LazyHStack"
            | "LazyVGrid"
            | "LazyHGrid"
            | "Grid"
            | "GridRow"
            | "RoundedRectangle"
            | "UnevenRoundedRectangle"
            | "Rectangle"
            | "Circle"
            | "Ellipse"
            | "Capsule"
            | "system"
            | "custom"
    )
}

fn collect_style_literals_in_arguments(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    inherited_metadata: Option<(TokenCategory, StyleContext)>,
    allows_style_labels: bool,
    out: &mut Vec<HardcodedLiteral>,
) {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        // Nested calls establish their own style context and are visited independently.
        if current.kind() == "call_expression" {
            continue;
        }

        if let Some((label, value_node)) = labeled_argument_parts(current, source) {
            // Nested call values belong to the nested call, not this enclosing traversal.
            if !value_contains_call_expression(value_node) {
                let metadata = if allows_style_labels {
                    match (style_label_metadata(&label), inherited_metadata) {
                        // SwiftUI `.shadow(radius:)` uses a radius label for elevation depth.
                        (
                            Some((TokenCategory::Radius, StyleContext::Radius)),
                            Some((TokenCategory::Elevation, StyleContext::Elevation)),
                        ) if label == "radius" => inherited_metadata,
                        (Some(label_meta), _) => Some(label_meta),
                        (None, inherited) => inherited,
                    }
                } else {
                    inherited_metadata
                };
                if let Some((category, context)) = metadata
                    && let Some(literal) =
                        literal_from_style_value(value_node, source, category, context)
                {
                    out.push(literal);
                }
            }
        } else if let Some((category, context)) = inherited_metadata
            && let Some(literal) = bare_literal_node(current, source, category, context)
        {
            out.push(literal);
        }

        for i in (0..current.child_count()).rev() {
            if let Some(child) = current.child(i) {
                if child.kind() == "call_expression" {
                    continue;
                }
                stack.push(child);
            }
        }
    }
}

fn value_contains_call_expression(node: tree_sitter::Node<'_>) -> bool {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "call_expression" {
            return true;
        }
        for i in 0..current.child_count() {
            if let Some(child) = current.child(i) {
                stack.push(child);
            }
        }
    }
    false
}

fn labeled_argument_parts<'a>(
    node: tree_sitter::Node<'a>,
    source: &[u8],
) -> Option<(String, tree_sitter::Node<'a>)> {
    // Swift value arguments look like `label: expr` under value_argument / call_suffix.
    if !matches!(node.kind(), "value_argument" | "labeled_expression") {
        // Some grammars flatten label + colon + expr as siblings under call_suffix.
        return labeled_argument_from_siblings(node, source);
    }

    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    let label = children.iter().find_map(|child| {
        if matches!(child.kind(), "simple_identifier" | "value_argument_label") {
            child.utf8_text(source).ok().map(str::to_owned)
        } else {
            None
        }
    })?;
    let value = *children.last()?;
    if value.kind() == "simple_identifier" && children.len() == 1 {
        return None;
    }
    Some((label, value))
}

fn labeled_argument_from_siblings<'a>(
    node: tree_sitter::Node<'a>,
    source: &[u8],
) -> Option<(String, tree_sitter::Node<'a>)> {
    // Detect `identifier ':' expression` patterns among immediate children.
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for window in children.windows(3) {
        let (label_node, colon, value) = (window[0], window[1], window[2]);
        if colon.kind() != ":" {
            continue;
        }
        if !matches!(
            label_node.kind(),
            "simple_identifier" | "value_argument_label"
        ) {
            continue;
        }
        let label = label_node.utf8_text(source).ok()?.to_owned();
        return Some((label, value));
    }
    None
}

fn literal_from_style_value(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    category: TokenCategory,
    context: StyleContext,
) -> Option<HardcodedLiteral> {
    if let Some(literal) = bare_literal_node(node, source, category, context) {
        return Some(literal);
    }
    // Peel non-call wrappers only; nested calls own their own literals.
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "call_expression" {
            continue;
        }
        if let Some(literal) = bare_literal_node(current, source, category, context) {
            return Some(literal);
        }
        for i in (0..current.child_count()).rev() {
            if let Some(child) = current.child(i) {
                if child.kind() == "call_expression" {
                    continue;
                }
                stack.push(child);
            }
        }
    }
    None
}

fn bare_literal_node(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    category: TokenCategory,
    context: StyleContext,
) -> Option<HardcodedLiteral> {
    let kind = node.kind();
    let is_number = matches!(
        kind,
        "integer_literal" | "float_literal" | "number" | "real_literal"
    );
    let is_string = matches!(
        kind,
        "line_string_literal" | "raw_string_literal" | "string_literal"
    );
    match category {
        TokenCategory::Color => {
            if !is_string && !is_number {
                return None;
            }
        }
        TokenCategory::Spacing
        | TokenCategory::Typography
        | TokenCategory::Radius
        | TokenCategory::Elevation => {
            if !is_number {
                return None;
            }
        }
        TokenCategory::Unknown => return None,
    }
    let text = node.utf8_text(source).ok()?.to_owned();
    Some(HardcodedLiteral {
        value: text,
        category,
        context,
        position: node.start_position(),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
    })
}

fn call_argument_node(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "call_suffix" | "value_arguments" | "lambda_literal"
        )
    })
}

fn dedupe_hardcoded_literals_by_longest_range(candidates: &mut Vec<HardcodedLiteral>) {
    candidates.sort_by(|left, right| {
        let left_len = left.end_byte.saturating_sub(left.start_byte);
        let right_len = right.end_byte.saturating_sub(right.start_byte);
        right_len
            .cmp(&left_len)
            .then(left.start_byte.cmp(&right.start_byte))
            .then(format!("{:?}", left.category).cmp(&format!("{:?}", right.category)))
    });
    let mut kept_ranges: Vec<(usize, usize)> = Vec::new();
    let mut kept = Vec::new();
    for candidate in candidates.drain(..) {
        let contained = kept_ranges
            .iter()
            .any(|&(start, end)| start <= candidate.start_byte && candidate.end_byte <= end);
        if contained {
            continue;
        }
        kept_ranges.push((candidate.start_byte, candidate.end_byte));
        kept.push(candidate);
    }
    *candidates = kept;
}

fn extract_token_sites_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    module_identity: &str,
    semantic_module: bool,
    token_index: &RegistryTokenIndex,
    out: &mut Vec<TokenSite>,
) {
    struct PendingToken {
        start_byte: usize,
        end_byte: usize,
        site: TokenSite,
    }

    let mut candidates = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !is_inside_preview_macro(node, source)
            && is_token_reference_node(node)
            && let Ok(text) = node.utf8_text(source)
            && let Some(token_match) = token_index.matches.get(text)
        {
            let pos = node.start_position();
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let parent = nearest_enclosing_view(node, source).map(|(name, parent_pos)| {
                parent_scope_for_view(file, module_identity, semantic_module, &name, parent_pos)
            });
            candidates.push(PendingToken {
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                site: TokenSite {
                    id: format!(
                        "token.swift:{file}:{line}:{column}:{}",
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
                },
            });
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }

    candidates.sort_by(|left, right| {
        let left_len = left.end_byte.saturating_sub(left.start_byte);
        let right_len = right.end_byte.saturating_sub(right.start_byte);
        right_len
            .cmp(&left_len)
            .then(left.start_byte.cmp(&right.start_byte))
            .then(left.site.key.cmp(&right.site.key))
    });

    let mut kept_ranges: Vec<(usize, usize)> = Vec::new();
    for candidate in candidates {
        let contained = kept_ranges
            .iter()
            .any(|&(start, end)| start <= candidate.start_byte && candidate.end_byte <= end);
        if contained {
            continue;
        }
        kept_ranges.push((candidate.start_byte, candidate.end_byte));
        out.push(candidate.site);
    }
}

/// Token keys must match expression/reference nodes, not declaration bindings or types.
fn is_token_reference_node(node: tree_sitter::Node<'_>) -> bool {
    match node.kind() {
        "navigation_expression" | "call_expression" => true,
        "simple_identifier" => !is_declaration_or_type_identifier(node),
        _ => false,
    }
}

fn is_declaration_or_type_identifier(node: tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() == "pattern" {
        return parent.parent().is_some_and(|grandparent| {
            matches!(
                grandparent.kind(),
                "property_declaration"
                    | "variable_declaration"
                    | "for_statement"
                    | "closure_parameter"
                    | "parameter"
            )
        });
    }
    matches!(
        parent.kind(),
        "property_declaration"
            | "function_declaration"
            | "class_declaration"
            | "struct_declaration"
            | "enum_declaration"
            | "protocol_declaration"
            | "typealias_declaration"
            | "type_identifier"
            | "parameter"
            | "value_parameter"
            | "enum_entry"
    )
}

#[allow(dead_code, clippy::too_many_arguments)]
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
        "prefix_expression" => {
            // Implicit member expressions such as `.system(size: 14)`.
            let mut cursor = node.walk();
            let member = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "simple_identifier")?;
            let name = member.utf8_text(source).ok()?.to_owned();
            Some(ResolvedCallSite {
                symbol: name,
                qualifier: None,
                position: member.start_position(),
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
                code: root_not_found_code(resolved.kind).to_owned(),
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

    let mut parser = new_parser().map_err(|error| TreeSitterScanError::ParserInitFailed {
        reason: error.to_string(),
    })?;

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
    let mut token_sites = Vec::new();
    let mut hardcoded_style_sites = Vec::new();
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
    let mut scanned_modules = BTreeSet::new();
    for (relative_file, parsed) in &parsed_files {
        let (module_identity, semantic_module) = module_identity_for_file(relative_file);
        if semantic_module {
            scanned_modules.insert(module_identity);
        }
        for local in index_local_components_from_source(
            parsed.tree.root_node(),
            parsed.source.as_bytes(),
            relative_file,
        ) {
            local_index.insert(relative_file, local.clone());
            local_components.push(local);
        }
    }
    local_index.scanned_modules = scanned_modules;

    for (relative_file, parsed) in &parsed_files {
        let root = parsed.tree.root_node();
        let source = parsed.source.as_bytes();
        extract_usage_from_source(
            root,
            source,
            relative_file,
            &registry,
            &local_index,
            &mut usage_sites,
        );
        let (module_identity, semantic_module) = module_identity_for_file(relative_file);
        extract_hardcoded_style_from_source(
            root,
            source,
            relative_file,
            &module_identity,
            semantic_module,
            &mut hardcoded_style_sites,
        );
        extract_token_sites_from_source(
            root,
            source,
            relative_file,
            &module_identity,
            semantic_module,
            &registry.token_index,
            &mut token_sites,
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
    token_sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.location.column.cmp(&right.location.column))
            .then(left.token_id.cmp(&right.token_id))
    });
    hardcoded_style_sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.location.column.cmp(&right.location.column))
    });

    let has_gaps = parse_failures > 0
        || diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "root_not_found" || diagnostic.code == "root_glob_not_found"
        });

    Ok(TreeSitterScanResult {
        design_system_components,
        local_components,
        usage_sites,
        design_system_tokens: registry.tokens,
        token_sites,
        hardcoded_style_sites,
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

    #[test]
    fn swift_framework_catalog_matches_only_swiftui_module() {
        assert!(is_framework_swiftui_module("SwiftUI"));
        assert!(!is_framework_swiftui_module("SwiftUIExtras"));
        assert!(!is_framework_swiftui_module("UIKit"));
    }

    #[test]
    fn qualified_call_from_swiftui_module_is_framework_origin() {
        let registry = registry_without_packages(&[]);
        let (_, usages) = parse_and_extract(
            "import SwiftUI\nstruct Screen: View { var body: some View { SwiftUI.PlatformOnlyView() } }",
            &registry,
        );

        let usage = usages
            .iter()
            .find(|usage| usage.symbol == "PlatformOnlyView")
            .expect("SwiftUI call should be retained");
        assert_eq!(usage.callee_origin, CalleeOrigin::Framework);
    }

    fn parse_and_extract(
        source: &str,
        registry: &RegistryIndex,
    ) -> (Vec<LocalComponent>, Vec<UsageSite>) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("Test.swift");
        std::fs::write(&path, source).expect("source");
        let mut parser = make_parser();
        let parsed = parse_swift_file_permissive(&mut parser, &path).expect("parse");
        let source = parsed.source.as_bytes();
        let tree = parsed.tree;
        let mut local_index = LocalViewIndex::default();
        let mut locals = Vec::new();
        for local in index_local_components_from_source(tree.root_node(), source, "Test.swift") {
            local_index.insert("Test.swift", local.clone());
            locals.push(local);
        }
        let mut usages = Vec::new();
        extract_usage_from_source(
            tree.root_node(),
            source,
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

        assert_eq!(usages.len(), 5);
        assert_eq!(
            usages
                .iter()
                .filter(|usage| usage.registry_symbol.is_some())
                .count(),
            3
        );
    }

    #[test]
    fn same_file_local_view_wins_over_registry_name_collision() {
        let registry = registry_with_package("Button", "AcmeDesignSystem");
        let (_, usages) = parse_and_extract(
            "import SwiftUI\nstruct Button: View { var body: some View { Text(\"local\") } }\nstruct Screen: View { var body: some View { Button() } }",
            &registry,
        );

        assert_eq!(usages.len(), 2);
        assert_eq!(
            usages
                .iter()
                .filter(|usage| usage.match_status == MatchStatus::Local)
                .count(),
            1
        );
    }

    #[test]
    fn matching_module_import_resolves_with_a_qualified_symbol() {
        let registry = registry_with_package("Button", "AcmeDesignSystem");
        let (_, usages) = parse_and_extract(
            "import AcmeDesignSystem\nstruct Screen: View { var body: some View { Button() } }",
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Resolved);
        assert_eq!(
            usages[0].qualified_symbol.as_deref(),
            Some("AcmeDesignSystem.Button")
        );
    }

    #[test]
    fn package_mismatch_keeps_a_qualified_unresolved_usage() {
        let registry = registry_with_package("Button", "AcmeDesignSystem");
        let (_, usages) = parse_and_extract(
            "import OtherWidgets\nstruct Screen: View { var body: some View { Button() } }",
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
        assert_eq!(
            usages[0].qualified_symbol.as_deref(),
            Some("OtherWidgets.Button")
        );
    }

    #[test]
    fn imported_local_view_resolves_by_explicit_module_import() {
        let registry = registry_without_packages(&[]);
        let mut parser = make_parser();
        let local_source =
            "import SwiftUI\nstruct LocalCard: View { var body: some View { Text(\"local\") } }";
        let local_tree = parser
            .parse(local_source.as_bytes(), None)
            .expect("parse local");
        let local = index_local_components_from_source(
            local_tree.root_node(),
            local_source.as_bytes(),
            "Sources/Feature/Local.swift",
        )[0]
        .clone();
        let mut local_index = LocalViewIndex::default();
        local_index.insert("Sources/Feature/Local.swift", local.clone());

        let source = "import Feature\nstruct Screen: View { var body: some View { LocalCard() } }";
        let tree = parser.parse(source.as_bytes(), None).expect("parse caller");
        let mut usages = Vec::new();
        extract_usage_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Sources/App/Screen.swift",
            &registry,
            &local_index,
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

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].callee_origin, wax_contract::CalleeOrigin::Unknown);
    }

    #[test]
    fn unrelated_macro_bodies_are_not_treated_as_preview_bodies() {
        let registry = registry_without_packages(&[("PreviewOnlyButton", "PreviewOnlyButton")]);
        let (_, usages) = parse_and_extract("#PreviewFoo { PreviewOnlyButton() }", &registry);

        assert_eq!(usages.len(), 1);
    }

    #[test]
    fn preview_macro_bodies_are_not_usage_sites() {
        let registry = registry_without_packages(&[("PreviewOnlyButton", "PreviewOnlyButton")]);
        let (_, usages) = parse_and_extract("#Preview { PreviewOnlyButton() }", &registry);
        assert!(usages.is_empty());
    }

    #[test]
    fn preview_markers_in_comments_and_strings_are_ignored() {
        let source = br###"// #Preview { CommentButton() }
let text = "#Preview { StringButton() }"
CommentButton()
"###;
        let call = source
            .windows(b"CommentButton()".len())
            .rposition(|window| window == b"CommentButton()")
            .expect("call after comment");

        assert!(!preview_macro_body_contains(call, source));
    }

    #[test]
    fn recovered_available_preview_bodies_are_not_usage_sites() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("Test.swift");
        std::fs::write(
            &path,
            "@available(iOS 18.0, *)\n#Preview { PreviewOnlyButton() }",
        )
        .expect("source");
        let mut parser = make_parser();
        let parsed = parse_swift_file_permissive(&mut parser, &path).expect("parse");
        let registry = registry_without_packages(&[("PreviewOnlyButton", "PreviewOnlyButton")]);
        let local_index = LocalViewIndex::default();
        let mut usages = Vec::new();

        extract_usage_from_source(
            parsed.tree.root_node(),
            parsed.source.as_bytes(),
            "Test.swift",
            &registry,
            &local_index,
            &mut usages,
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

        assert_eq!(usages.len(), 3);
        let unknown = usages
            .iter()
            .find(|usage| usage.symbol == "UnknownCard")
            .expect("unknown call should be retained");
        assert_eq!(unknown.match_status, MatchStatus::Unresolved);
        assert_eq!(unknown.callee_origin, wax_contract::CalleeOrigin::Unknown);
    }

    #[test]
    fn framework_swiftui_calls_are_reported_separately() {
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

        assert_eq!(usages.len(), 3);
        assert!(usages.iter().all(|usage| {
            usage.callee_origin == wax_contract::CalleeOrigin::Framework
                && usage.match_status == MatchStatus::Unresolved
        }));
    }

    #[test]
    fn any_swiftui_module_import_is_framework_even_for_uncatalogued_symbols() {
        let registry = registry_without_packages(&[("PrimaryButton", "PrimaryButton")]);
        let (_, usages) = parse_and_extract(
            r#"
        import SwiftUI
        struct Screen: View {
            var body: some View {
                SwiftUI.NonStandardChrome()
            }
        }
        "#,
            &registry,
        );

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].symbol, "NonStandardChrome");
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
        assert_eq!(
            usages[0].callee_origin,
            wax_contract::CalleeOrigin::Framework
        );
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
    fn non_ds_module_import_is_retained_as_unresolved_when_package_is_configured() {
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
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
    }

    #[test]
    fn qualified_non_ds_call_is_retained_as_unresolved_when_package_is_configured() {
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
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].match_status, MatchStatus::Unresolved);
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

    fn token_index_from_json(tokens_json: &str) -> RegistryTokenIndex {
        let value = serde_json::json!({ "tokens": serde_json::from_str::<serde_json::Value>(tokens_json).unwrap() });
        let tokens = parse_registry_tokens(&value).expect("tokens");
        token_index(&tokens).expect("index")
    }

    fn extract_tokens(source: &str, index: &RegistryTokenIndex) -> Vec<TokenSite> {
        let mut parser = make_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parse");
        let mut out = Vec::new();
        extract_token_sites_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Screen.swift",
            "App",
            true,
            index,
            &mut out,
        );
        out
    }

    fn extract_hardcoded(source: &str) -> Vec<HardcodedStyleSite> {
        let mut parser = make_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parse");
        let mut out = Vec::new();
        extract_hardcoded_style_from_source(
            tree.root_node(),
            source.as_bytes(),
            "Screen.swift",
            "App",
            true,
            &mut out,
        );
        out
    }

    #[test]
    fn overlapping_token_alias_keeps_longest_match_only() {
        let index = token_index_from_json(
            r#"[
              {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color",
                "aliases": ["primary"]
              }
            ]"#,
        );
        let source = r#"
import SwiftUI
struct Screen: View {
    var body: some View {
        let value = Theme.colors.primary
        Text("x")
    }
}
"#;
        let sites = extract_tokens(source, &index);
        assert_eq!(sites.len(), 1, "expected one longest match, got {sites:?}");
        assert_eq!(sites[0].key, "Theme.colors.primary");
        assert_eq!(sites[0].token_id, "color.primary");
    }

    #[test]
    fn token_alias_does_not_match_declaration_bindings() {
        let index = token_index_from_json(
            r#"[
              {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color",
                "aliases": ["primary"]
              }
            ]"#,
        );
        let source = r#"
import SwiftUI
struct Screen: View {
    var body: some View {
        let primary = Theme.colors.primary
        Text("x")
    }
}
"#;
        let sites = extract_tokens(source, &index);
        assert_eq!(
            sites.len(),
            1,
            "binding name must not become a token site: {sites:?}"
        );
        assert_eq!(sites[0].key, "Theme.colors.primary");
    }

    #[test]
    fn preview_macro_bodies_are_excluded_from_token_sites() {
        let index = token_index_from_json(
            r#"[{"id":"color.primary","key":"Theme.colors.primary","category":"color"}]"#,
        );
        let sites = extract_tokens("#Preview { Text(Theme.colors.primary) }", &index);
        assert!(sites.is_empty());
    }

    #[test]
    fn nested_and_labeled_swiftui_style_literals_are_detected() {
        let source = r#"
import SwiftUI
struct Screen: View {
    var body: some View {
        VStack(spacing: 12) {
            Text("Hi")
                .font(.system(size: 14))
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .cornerRadius(8)
        }
    }
}
"#;
        let sites = extract_hardcoded(source);
        assert!(
            sites
                .iter()
                .any(|s| s.category == TokenCategory::Spacing && s.value == "12"),
            "VStack(spacing: 12) missed: {sites:?}"
        );
        assert!(
            sites
                .iter()
                .any(|s| s.category == TokenCategory::Typography && s.value == "14"),
            ".font(.system(size: 14)) missed: {sites:?}"
        );
        assert!(
            sites
                .iter()
                .any(|s| s.category == TokenCategory::Radius && s.value == "8"),
            "radius literals missed: {sites:?}"
        );
        let eights = sites
            .iter()
            .filter(|s| s.category == TokenCategory::Radius && s.value == "8")
            .count();
        assert!(eights >= 1);
        // Dedup: both clipShape labeled cornerRadius and cornerRadius(8) may share value but
        // distinct ranges — both are legitimate. Ensure no identical range duplicates.
        let mut ranges = sites
            .iter()
            .map(|s| (s.location.line, s.location.column, &s.value, s.category))
            .collect::<Vec<_>>();
        let before = ranges.len();
        ranges.sort();
        ranges.dedup();
        assert_eq!(before, ranges.len(), "duplicate hardcoded sites: {sites:?}");
    }

    #[test]
    fn non_style_callees_do_not_emit_from_style_shaped_labels() {
        let source = r#"
import SwiftUI
struct Screen: View {
    var body: some View {
        Text("Hi")
    }
}
func track() {
    analytics.record(size: 14)
}
"#;
        let sites = extract_hardcoded(source);
        assert!(
            sites
                .iter()
                .all(|s| !(s.category == TokenCategory::Typography && s.value == "14")),
            "non-style callee labels must not emit style candidates: {sites:?}"
        );
    }

    #[test]
    fn nested_style_call_keeps_its_own_category() {
        let source = r#"
import SwiftUI
struct Screen: View {
    var body: some View {
        Text("Hi").background(Rectangle().frame(width: 8))
    }
}
"#;
        let sites = extract_hardcoded(source);
        assert!(
            sites
                .iter()
                .any(|s| s.category == TokenCategory::Spacing && s.value == "8"),
            "nested frame(width: 8) should remain Spacing: {sites:?}"
        );
        assert!(
            sites
                .iter()
                .all(|s| !(s.value == "8" && s.category == TokenCategory::Color)),
            "outer background must not reclassify nested spacing literal: {sites:?}"
        );
    }

    #[test]
    fn hardcoded_location_points_at_literal_not_callee() {
        let source = r#"
import SwiftUI
struct Screen: View {
    var body: some View {
        Text("Hi").cornerRadius(8)
    }

}
"#;
        let sites = extract_hardcoded(source);
        let site = sites
            .iter()
            .find(|s| s.category == TokenCategory::Radius && s.value == "8")
            .expect("cornerRadius literal");
        // "8" appears after "cornerRadius(" — column should not be the 'c' of cornerRadius.
        let corner_col = source
            .lines()
            .find(|l| l.contains("cornerRadius"))
            .and_then(|l| l.find("cornerRadius"))
            .map(|i| i as u32 + 1)
            .unwrap();
        let eight_col = source
            .lines()
            .find(|l| l.contains("cornerRadius"))
            .and_then(|l| l.find('8'))
            .map(|i| i as u32 + 1)
            .unwrap();
        assert_eq!(site.location.column, Some(eight_col));
        assert_ne!(site.location.column, Some(corner_col));
    }

    #[test]
    fn preview_macro_bodies_are_excluded_from_hardcoded_sites() {
        let sites = extract_hardcoded(r#"#Preview { Text("Preview only").padding(8) }"#);
        assert!(sites.is_empty());
    }
}
