//! React module graph indexing and resolver helpers.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use swc_common::Spanned;
use swc_ecma_ast::{
    Decl, DefaultDecl, ExportDecl, ExportDefaultDecl, ExportDefaultExpr, ExportSpecifier, Expr,
    ImportSpecifier, ModuleDecl, ModuleExportName, ModuleItem, Pat,
};
use wax_contract::{Diagnostic, DiagnosticSeverity};

use crate::config::{PackageConfig, ReactScanConfig};
use crate::diagnostics::{
    DS_EXPORT_UNRESOLVED, DS_IMPORT_UNRESOLVED, PACKAGE_ENTRYPOINT_UNRESOLVED,
};
use crate::files::ReactSourceFileCollection;
use crate::registry::ReactRegistryIndex;
use crate::swc_parse::ParsedReactModule;

const SOURCE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx"];

/// Resolver output for one imported or exported symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSymbol {
    /// Repo-relative source module containing the resolved symbol.
    pub module: PathBuf,
    /// Symbol name resolved within [`ResolvedSymbol::module`].
    pub symbol: String,
}

/// One import binding in a module, keyed by local name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    /// Local identifier bound inside the importing module.
    pub local_name: String,
    /// Raw source import specifier.
    pub source_specifier: String,
    /// Imported symbol kind from the source module.
    pub imported_symbol: ImportedSymbol,
    /// Resolved source module path when resolution succeeds.
    pub source_module: Option<PathBuf>,
}

/// Imported symbol kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedSymbol {
    /// `import Foo from "..."`
    Default,
    /// `import * as Foo from "..."`
    Namespace,
    /// `import { Foo } from "..."`
    Named(String),
}

/// One exported symbol binding in a module, keyed by export name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportBinding {
    /// Export points at a local binding in the same module.
    Local(String),
    /// Export points at a symbol from another module.
    ReExport {
        /// Raw source re-export specifier.
        source_specifier: String,
        /// Exported symbol name in the source module.
        source_export: String,
        /// Resolved source module path when resolution succeeds.
        source_module: Option<PathBuf>,
    },
}

/// Indexed import/export bindings for one module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactModuleRecord {
    /// Local name to import binding map.
    pub imports: BTreeMap<String, ImportBinding>,
    /// Export name to export binding map.
    pub exports: BTreeMap<String, ExportBinding>,
}

/// React import/export graph used by later extraction steps.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactModuleGraph {
    /// Per-module import/export records.
    pub modules: BTreeMap<PathBuf, ReactModuleRecord>,
}

impl ReactModuleGraph {
    /// Returns one local import binding for a module.
    #[must_use]
    pub fn import_binding(&self, module: &Path, local_name: &str) -> Option<&ImportBinding> {
        self.modules.get(module)?.imports.get(local_name)
    }

    /// Returns whether an unresolved import binding is design-system relevant.
    #[must_use]
    pub fn unresolved_import_is_design_system_relevant(
        &self,
        module: &Path,
        local_name: &str,
        registry: &ReactRegistryIndex,
        config: &ReactScanConfig,
    ) -> bool {
        let Some(import) = self.import_binding(module, local_name) else {
            return false;
        };
        import.source_module.is_none()
            && import_is_design_system_relevant(
                &import.source_specifier,
                &import.local_name,
                &import.imported_symbol,
                registry,
                config,
            )
    }

    /// Returns whether a resolved import binding comes through a configured design-system package.
    #[must_use]
    pub fn import_resolves_through_configured_package(
        &self,
        module: &Path,
        local_name: &str,
        config: &ReactScanConfig,
    ) -> bool {
        self.import_resolves_through_configured_package_internal(module, local_name, config, 0)
    }

    /// Resolves one local import binding to the final symbol location.
    #[must_use]
    pub fn resolve_import(&self, module: &Path, local_name: &str) -> Option<ResolvedSymbol> {
        let record = self.modules.get(module)?;
        let import = record.imports.get(local_name)?;
        let target_module = import.source_module.as_ref()?;
        let symbol = match &import.imported_symbol {
            ImportedSymbol::Default => "default".to_owned(),
            ImportedSymbol::Namespace => "*".to_owned(),
            ImportedSymbol::Named(name) => name.clone(),
        };
        self.resolve_export_internal(target_module, &symbol, 0)
            .or_else(|| {
                Some(ResolvedSymbol {
                    module: target_module.clone(),
                    symbol,
                })
            })
    }

    /// Resolves one export name to a source module and symbol.
    #[must_use]
    pub fn resolve_export(&self, module: &Path, export_name: &str) -> Option<ResolvedSymbol> {
        self.resolve_export_internal(module, export_name, 0)
    }

    fn resolve_export_internal(
        &self,
        module: &Path,
        export_name: &str,
        depth: usize,
    ) -> Option<ResolvedSymbol> {
        let record = self.modules.get(module)?;
        let binding = record.exports.get(export_name)?;
        match binding {
            ExportBinding::Local(local_name) => {
                if let Some(import_binding) = record.imports.get(local_name) {
                    let source_module = import_binding.source_module.as_ref()?;
                    let source_symbol = match &import_binding.imported_symbol {
                        ImportedSymbol::Default => "default".to_owned(),
                        ImportedSymbol::Namespace => "*".to_owned(),
                        ImportedSymbol::Named(name) => name.clone(),
                    };
                    if depth >= 1 {
                        return Some(ResolvedSymbol {
                            module: source_module.clone(),
                            symbol: source_symbol,
                        });
                    }
                    self.resolve_export_internal(source_module, &source_symbol, depth + 1)
                        .or_else(|| {
                            Some(ResolvedSymbol {
                                module: source_module.clone(),
                                symbol: source_symbol,
                            })
                        })
                } else {
                    Some(ResolvedSymbol {
                        module: module.to_path_buf(),
                        symbol: local_name.clone(),
                    })
                }
            }
            ExportBinding::ReExport {
                source_export,
                source_module,
                ..
            } => {
                let source_module = source_module.as_ref()?;
                if depth >= 1 {
                    return Some(ResolvedSymbol {
                        module: source_module.clone(),
                        symbol: source_export.clone(),
                    });
                }
                self.resolve_export_internal(source_module, source_export, depth + 1)
                    .or_else(|| {
                        Some(ResolvedSymbol {
                            module: source_module.clone(),
                            symbol: source_export.clone(),
                        })
                    })
            }
        }
    }

    fn import_resolves_through_configured_package_internal(
        &self,
        module: &Path,
        local_name: &str,
        config: &ReactScanConfig,
        depth: usize,
    ) -> bool {
        if depth > 2 {
            return false;
        }
        let Some(import) = self.import_binding(module, local_name) else {
            return false;
        };
        let Some(source_module) = import.source_module.as_deref() else {
            return false;
        };
        if configured_package_for_specifier(&import.source_specifier, &config.packages).is_some() {
            return true;
        }

        let source_export = match &import.imported_symbol {
            ImportedSymbol::Default => "default".to_owned(),
            ImportedSymbol::Namespace => "*".to_owned(),
            ImportedSymbol::Named(name) => name.clone(),
        };
        self.export_resolves_through_configured_package_internal(
            source_module,
            &source_export,
            config,
            depth + 1,
        )
    }

    fn export_resolves_through_configured_package_internal(
        &self,
        module: &Path,
        export_name: &str,
        config: &ReactScanConfig,
        depth: usize,
    ) -> bool {
        if depth > 2 {
            return false;
        }
        let Some(record) = self.modules.get(module) else {
            return false;
        };
        let Some(binding) = record.exports.get(export_name) else {
            return false;
        };

        match binding {
            ExportBinding::Local(local_name) => {
                record.imports.contains_key(local_name)
                    && self.import_resolves_through_configured_package_internal(
                        module,
                        local_name,
                        config,
                        depth + 1,
                    )
            }
            ExportBinding::ReExport {
                source_specifier,
                source_export,
                source_module,
            } => {
                let Some(source_module) = source_module.as_deref() else {
                    return false;
                };
                configured_package_for_specifier(source_specifier, &config.packages).is_some()
                    || self.export_resolves_through_configured_package_internal(
                        source_module,
                        source_export,
                        config,
                        depth + 1,
                    )
            }
        }
    }
}

/// Module-graph build output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactModuleGraphBuild {
    /// Indexed graph.
    pub graph: ReactModuleGraph,
    /// Recoverable graph-resolution diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// Indexes imports/exports and resolves source modules for React files.
#[must_use]
pub fn build_react_module_graph(
    repo_root: &Path,
    parsed_modules: &[ParsedReactModule],
    files: &ReactSourceFileCollection,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
) -> ReactModuleGraphBuild {
    let known_files = files.files.iter().cloned().collect::<BTreeSet<_>>();
    let tsconfig = load_tsconfig_options(repo_root, config.tsconfig.as_deref());
    let resolver = ModuleResolver::new(config, &known_files, tsconfig.as_ref());
    let mut diagnostics = Vec::new();
    diagnostics.extend(package_entrypoint_diagnostics(config, &known_files));

    let mut graph = ReactModuleGraph::default();
    for parsed in parsed_modules {
        let mut record = ReactModuleRecord::default();

        for item in &parsed.module.body {
            let ModuleItem::ModuleDecl(decl) = item else {
                continue;
            };
            match decl {
                ModuleDecl::Import(import_decl) => {
                    let source = import_decl.src.value.to_string_lossy().to_string();
                    for specifier in &import_decl.specifiers {
                        let (local_name, imported_symbol, span) = match specifier {
                            ImportSpecifier::Default(default_specifier) => (
                                default_specifier.local.sym.to_string(),
                                ImportedSymbol::Default,
                                default_specifier.span,
                            ),
                            ImportSpecifier::Namespace(namespace_specifier) => (
                                namespace_specifier.local.sym.to_string(),
                                ImportedSymbol::Namespace,
                                namespace_specifier.span,
                            ),
                            ImportSpecifier::Named(named_specifier) => {
                                let imported = named_specifier.imported.as_ref().map_or_else(
                                    || named_specifier.local.sym.to_string(),
                                    module_export_name,
                                );
                                (
                                    named_specifier.local.sym.to_string(),
                                    ImportedSymbol::Named(imported),
                                    named_specifier.span,
                                )
                            }
                        };
                        let source_module =
                            resolver.resolve_specifier(&parsed.file, &source, &imported_symbol);
                        if source_module.is_none()
                            && import_is_design_system_relevant(
                                &source,
                                &local_name,
                                &imported_symbol,
                                registry,
                                config,
                            )
                        {
                            diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::Warning,
                                code: DS_IMPORT_UNRESOLVED.to_owned(),
                                message: format!(
                                    "design-system-relevant import '{local_name}' from '{source}' could not be resolved"
                                ),
                                location: parsed.source_location_from_span(span),
                            });
                        }

                        record.imports.insert(
                            local_name.clone(),
                            ImportBinding {
                                local_name,
                                source_specifier: source.clone(),
                                imported_symbol,
                                source_module,
                            },
                        );
                    }
                }
                ModuleDecl::ExportDecl(export_decl) => {
                    record_export_decl(export_decl, &mut record);
                }
                ModuleDecl::ExportDefaultDecl(default_decl) => {
                    record_default_export(default_decl, &mut record);
                }
                ModuleDecl::ExportDefaultExpr(default_expr) => {
                    record_default_export_expr(default_expr, &mut record);
                }
                ModuleDecl::ExportNamed(named_export) => {
                    let source = named_export
                        .src
                        .as_ref()
                        .map(|src| src.value.to_string_lossy().to_string());
                    for export_specifier in &named_export.specifiers {
                        if let ExportSpecifier::Named(named_specifier) = export_specifier {
                            let source_name = module_export_name(&named_specifier.orig);
                            let exported_name = named_specifier
                                .exported
                                .as_ref()
                                .map_or_else(|| source_name.clone(), module_export_name);

                            if let Some(source) = &source {
                                let imported = if source_name == "default" {
                                    ImportedSymbol::Default
                                } else {
                                    ImportedSymbol::Named(source_name.clone())
                                };
                                let source_module =
                                    resolver.resolve_specifier(&parsed.file, source, &imported);
                                if source_module.is_none()
                                    && export_is_design_system_relevant(
                                        source,
                                        &exported_name,
                                        &source_name,
                                        registry,
                                        config,
                                    )
                                {
                                    diagnostics.push(Diagnostic {
                                        severity: DiagnosticSeverity::Warning,
                                        code: DS_EXPORT_UNRESOLVED.to_owned(),
                                        message: format!(
                                            "design-system-relevant re-export '{exported_name}' from '{source}' could not be resolved"
                                        ),
                                        location: parsed
                                            .source_location_from_span(named_specifier.span()),
                                    });
                                }
                                record.exports.insert(
                                    exported_name,
                                    ExportBinding::ReExport {
                                        source_specifier: source.clone(),
                                        source_export: source_name,
                                        source_module,
                                    },
                                );
                            } else {
                                record.exports.insert(
                                    exported_name,
                                    ExportBinding::Local(source_name.clone()),
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        graph.modules.insert(parsed.file.clone(), record);
    }

    ReactModuleGraphBuild { graph, diagnostics }
}

fn record_export_decl(export_decl: &ExportDecl, record: &mut ReactModuleRecord) {
    for name in declared_names(&export_decl.decl) {
        record
            .exports
            .insert(name.clone(), ExportBinding::Local(name.clone()));
    }
}

fn record_default_export(default_decl: &ExportDefaultDecl, record: &mut ReactModuleRecord) {
    let local_name = match &default_decl.decl {
        DefaultDecl::Fn(fn_expr) => fn_expr.ident.as_ref().map(|ident| ident.sym.to_string()),
        DefaultDecl::Class(class_expr) => {
            class_expr.ident.as_ref().map(|ident| ident.sym.to_string())
        }
        DefaultDecl::TsInterfaceDecl(_) => None,
    };
    record.exports.insert(
        "default".to_owned(),
        ExportBinding::Local(local_name.unwrap_or_else(|| "default".to_owned())),
    );
}

fn record_default_export_expr(default_expr: &ExportDefaultExpr, record: &mut ReactModuleRecord) {
    let local_name = match &*default_expr.expr {
        Expr::Ident(ident) => ident.sym.to_string(),
        _ => "default".to_owned(),
    };
    record
        .exports
        .insert("default".to_owned(), ExportBinding::Local(local_name));
}

fn declared_names(decl: &Decl) -> Vec<String> {
    match decl {
        Decl::Class(class_decl) => vec![class_decl.ident.sym.to_string()],
        Decl::Fn(fn_decl) => vec![fn_decl.ident.sym.to_string()],
        Decl::Var(var_decl) => {
            let mut names = Vec::new();
            for declarator in &var_decl.decls {
                collect_pattern_names(&declarator.name, &mut names);
            }
            names
        }
        _ => Vec::new(),
    }
}

fn collect_pattern_names(pat: &Pat, names: &mut Vec<String>) {
    match pat {
        Pat::Ident(binding_ident) => names.push(binding_ident.id.sym.to_string()),
        Pat::Array(array_pat) => {
            for entry in array_pat.elems.iter().flatten() {
                collect_pattern_names(entry, names);
            }
        }
        Pat::Object(object_pat) => {
            for prop in &object_pat.props {
                match prop {
                    swc_ecma_ast::ObjectPatProp::KeyValue(key_value) => {
                        collect_pattern_names(&key_value.value, names);
                    }
                    swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                        names.push(assign.key.sym.to_string());
                    }
                    swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                        collect_pattern_names(&rest.arg, names);
                    }
                }
            }
        }
        Pat::Rest(rest_pat) => collect_pattern_names(&rest_pat.arg, names),
        Pat::Assign(assign_pat) => collect_pattern_names(&assign_pat.left, names),
        _ => {}
    }
}

fn import_is_design_system_relevant(
    source: &str,
    local_name: &str,
    imported_symbol: &ImportedSymbol,
    registry: &ReactRegistryIndex,
    config: &ReactScanConfig,
) -> bool {
    if configured_package_for_specifier(source, &config.packages).is_some() {
        return true;
    }
    if is_unconfigured_bare_package_specifier(source, config) {
        return false;
    }
    match imported_symbol {
        ImportedSymbol::Named(name) => registry.resolve_targets.contains_key(name),
        ImportedSymbol::Default => registry.resolve_targets.contains_key(local_name),
        ImportedSymbol::Namespace => false,
    }
}

fn export_is_design_system_relevant(
    source: &str,
    exported_name: &str,
    source_name: &str,
    registry: &ReactRegistryIndex,
    config: &ReactScanConfig,
) -> bool {
    if configured_package_for_specifier(source, &config.packages).is_some() {
        return true;
    }
    if is_unconfigured_bare_package_specifier(source, config) {
        return false;
    }
    registry.resolve_targets.contains_key(exported_name)
        || registry.resolve_targets.contains_key(source_name)
}

fn is_unconfigured_bare_package_specifier(source: &str, config: &ReactScanConfig) -> bool {
    !source.starts_with('.')
        && configured_package_for_specifier(source, &config.packages).is_none()
        && !config
            .aliases
            .keys()
            .any(|pattern| match_wildcard_pattern(pattern, source).is_some())
}

fn package_entrypoint_diagnostics(
    config: &ReactScanConfig,
    known_files: &BTreeSet<PathBuf>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (package_name, package) in &config.packages {
        for (entrypoint, target) in &package.exports {
            if entrypoint.contains('*') || target.contains('*') {
                continue;
            }
            if resolve_repo_relative_target(target, known_files).is_none() {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: PACKAGE_ENTRYPOINT_UNRESOLVED.to_owned(),
                    message: format!(
                        "configured package entrypoint '{package_name}:{entrypoint}' target '{target}' could not be resolved to a source module"
                    ),
                    location: None,
                });
            }
        }
    }
    diagnostics
}

#[derive(Debug, Clone)]
struct ModuleResolver<'a> {
    config: &'a ReactScanConfig,
    known_files: &'a BTreeSet<PathBuf>,
    tsconfig: Option<&'a TsConfigOptions>,
}

impl<'a> ModuleResolver<'a> {
    fn new(
        config: &'a ReactScanConfig,
        known_files: &'a BTreeSet<PathBuf>,
        tsconfig: Option<&'a TsConfigOptions>,
    ) -> Self {
        Self {
            config,
            known_files,
            tsconfig,
        }
    }

    fn resolve_specifier(
        &self,
        importer_file: &Path,
        source_specifier: &str,
        imported_symbol: &ImportedSymbol,
    ) -> Option<PathBuf> {
        if source_specifier.starts_with('.') {
            return self.resolve_relative(importer_file, source_specifier);
        }
        if let Some(resolved) = self.resolve_alias(source_specifier, false) {
            return Some(resolved);
        }
        if let Some(resolved) = self.resolve_alias(source_specifier, true) {
            return Some(resolved);
        }
        self.resolve_package(source_specifier, imported_symbol)
    }

    fn resolve_relative(&self, importer_file: &Path, source_specifier: &str) -> Option<PathBuf> {
        let importer_dir = importer_file.parent().unwrap_or_else(|| Path::new(""));
        let joined = importer_dir.join(source_specifier);
        let normalized = normalize_relative_path(&joined)?;
        resolve_path_with_extensions_and_index(&normalized, self.known_files)
    }

    fn resolve_alias(&self, source_specifier: &str, tsconfig_aliases: bool) -> Option<PathBuf> {
        let empty_aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let alias_map = if tsconfig_aliases {
            self.tsconfig
                .map(|tsconfig| &tsconfig.paths)
                .unwrap_or(&empty_aliases)
        } else {
            &self.config.aliases
        };
        let base_url = if tsconfig_aliases {
            self.tsconfig
                .map(|tsconfig| tsconfig.base_url.as_path())
                .unwrap_or_else(|| Path::new(""))
        } else {
            Path::new("")
        };

        let mut best_match: Option<(usize, PathBuf)> = None;
        for (pattern, targets) in alias_map {
            let Some(wildcard) = match_wildcard_pattern(pattern, source_specifier) else {
                continue;
            };
            let specificity = alias_pattern_specificity(pattern);
            for target_pattern in targets {
                let substituted = apply_wildcard_target(target_pattern, wildcard);
                let candidate = base_url.join(substituted);
                if let Some(normalized) = normalize_relative_path(&candidate)
                    && let Some(resolved) =
                        resolve_path_with_extensions_and_index(&normalized, self.known_files)
                    && best_match
                        .as_ref()
                        .is_none_or(|(current, _)| specificity > *current)
                {
                    best_match = Some((specificity, resolved));
                }
            }
        }
        best_match.map(|(_, resolved)| resolved)
    }

    fn resolve_package(
        &self,
        source_specifier: &str,
        imported_symbol: &ImportedSymbol,
    ) -> Option<PathBuf> {
        let (package_config, remainder) =
            configured_package_for_specifier(source_specifier, &self.config.packages)?;

        let mut key_candidates = Vec::new();
        if remainder.is_empty() {
            if let ImportedSymbol::Named(symbol) = imported_symbol {
                key_candidates.push(symbol.clone());
            }
            key_candidates.push(".".to_owned());
        } else {
            key_candidates.push(format!("./{remainder}"));
        }

        for key in &key_candidates {
            if let Some(target) = package_config.exports.get(key)
                && let Some(resolved) = resolve_repo_relative_target(target, self.known_files)
            {
                return Some(resolved);
            }
        }

        for (pattern, target_pattern) in &package_config.exports {
            for key in &key_candidates {
                let Some(wildcard) = match_wildcard_pattern(pattern, key) else {
                    continue;
                };
                let target = apply_wildcard_target(target_pattern, wildcard);
                if let Some(resolved) = resolve_repo_relative_target(&target, self.known_files) {
                    return Some(resolved);
                }
            }
        }

        None
    }
}

fn configured_package_for_specifier<'a, 'b>(
    specifier: &'a str,
    packages: &'b BTreeMap<String, PackageConfig>,
) -> Option<(&'b PackageConfig, &'a str)> {
    let mut best_match: Option<(&'b PackageConfig, &'a str, usize)> = None;
    for (package_name, package) in packages {
        let (remainder, matched_len) = if specifier == package_name {
            ("", package_name.len())
        } else if let Some(remainder) = specifier.strip_prefix(package_name.as_str())
            && let Some(remainder) = remainder.strip_prefix('/')
        {
            (remainder, package_name.len())
        } else {
            continue;
        };
        if best_match.is_none_or(|(_, _, len)| matched_len > len) {
            best_match = Some((package, remainder, matched_len));
        }
    }
    best_match.map(|(package, remainder, _)| (package, remainder))
}

fn resolve_repo_relative_target(target: &str, known_files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let normalized = normalize_relative_path(Path::new(target))?;
    resolve_path_with_extensions_and_index(&normalized, known_files)
}

fn resolve_path_with_extensions_and_index(
    normalized: &Path,
    known_files: &BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    if known_files.contains(normalized) {
        return Some(normalized.to_path_buf());
    }

    let candidate_text = normalized.to_string_lossy();
    let has_supported_extension = SOURCE_EXTENSIONS
        .iter()
        .any(|extension| candidate_text.ends_with(extension));
    if !has_supported_extension {
        for extension in SOURCE_EXTENSIONS {
            let candidate = PathBuf::from(format!("{candidate_text}{extension}"));
            if known_files.contains(&candidate) {
                return Some(candidate);
            }
        }
        for extension in SOURCE_EXTENSIONS {
            let candidate = PathBuf::from(format!("{candidate_text}/index{extension}"));
            if known_files.contains(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn normalize_relative_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => return None,
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    Some(normalized)
}

fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_string_lossy().to_string(),
    }
}

fn match_wildcard_pattern<'a>(pattern: &'a str, value: &'a str) -> Option<&'a str> {
    if let Some(wildcard_index) = pattern.find('*') {
        let prefix = &pattern[..wildcard_index];
        let suffix = &pattern[wildcard_index + 1..];
        if value.starts_with(prefix)
            && value.ends_with(suffix)
            && value.len() >= prefix.len() + suffix.len()
        {
            let wildcard_start = prefix.len();
            let wildcard_end = value.len() - suffix.len();
            return Some(&value[wildcard_start..wildcard_end]);
        }
        return None;
    }
    if pattern == value {
        return Some("");
    }
    None
}

fn alias_pattern_specificity(pattern: &str) -> usize {
    pattern.find('*').unwrap_or(pattern.len())
}

fn apply_wildcard_target(pattern: &str, wildcard: &str) -> String {
    if let Some(index) = pattern.find('*') {
        format!("{}{}{}", &pattern[..index], wildcard, &pattern[index + 1..])
    } else {
        pattern.to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TsConfigOptions {
    base_url: PathBuf,
    paths: BTreeMap<String, Vec<String>>,
}

fn load_tsconfig_options(
    repo_root: &Path,
    tsconfig_path: Option<&Path>,
) -> Option<TsConfigOptions> {
    let tsconfig_path = tsconfig_path?;
    let tsconfig_file = repo_root.join(tsconfig_path);
    let raw = fs::read_to_string(tsconfig_file).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let compiler_options = value.get("compilerOptions")?;
    let tsconfig_dir = tsconfig_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(PathBuf::new);

    let base_url = compiler_options
        .get("baseUrl")
        .and_then(serde_json::Value::as_str)
        .and_then(|base_url| normalize_relative_path(&tsconfig_dir.join(base_url)))
        .unwrap_or(tsconfig_dir);

    let mut paths = BTreeMap::new();
    if let Some(path_map) = compiler_options
        .get("paths")
        .and_then(serde_json::Value::as_object)
    {
        for (key, targets) in path_map {
            let Some(targets) = targets.as_array() else {
                continue;
            };
            let parsed = targets
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if !parsed.is_empty() {
                paths.insert(key.clone(), parsed);
            }
        }
    }

    Some(TsConfigOptions { base_url, paths })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swc_parse::{ReactParseOutcome, parse_react_source_file};
    use std::fs;

    #[test]
    fn module_graph_indexes_import_shapes_and_aliases() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import React from "react";
            import { Button as DsButton } from "./Button";
            import * as Icons from "./icons";
            export const App = () => <DsButton icon={Icons.Check} />;
            "#,
        );
        fixture.write("src/Button.tsx", "export const Button = () => null;");
        fixture.write("src/icons/index.ts", "export const Check = 'check';");

        let build = fixture.build_graph(
            vec!["src/App.tsx", "src/Button.tsx", "src/icons/index.ts"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src")],
                ignore: Vec::new(),
                tsconfig: None,
                aliases: BTreeMap::new(),
                packages: BTreeMap::new(),
            },
            registry_with_symbols(&["Button"]),
        );

        let app = build
            .graph
            .modules
            .get(Path::new("src/App.tsx"))
            .expect("App module should exist");
        assert_eq!(
            app.imports["React"].imported_symbol,
            ImportedSymbol::Default
        );
        assert_eq!(
            app.imports["DsButton"].imported_symbol,
            ImportedSymbol::Named("Button".to_owned())
        );
        assert_eq!(
            app.imports["Icons"].imported_symbol,
            ImportedSymbol::Namespace
        );
        assert!(build.diagnostics.is_empty());
    }

    #[test]
    fn module_graph_resolves_relative_extensionless_and_index_imports() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"import { Button } from "./components"; export const App = () => <Button />;"#,
        );
        fixture.write(
            "src/components/index.ts",
            r#"export { Button } from "./Button";"#,
        );
        fixture.write(
            "src/components/Button.tsx",
            "export const Button = () => null;",
        );

        let build = fixture.build_graph(
            vec![
                "src/App.tsx",
                "src/components/index.ts",
                "src/components/Button.tsx",
            ],
            base_config(),
            registry_with_symbols(&["Button"]),
        );

        let resolved = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "Button")
            .expect("Button import should resolve");
        assert_eq!(resolved.module, PathBuf::from("src/components/Button.tsx"));
        assert_eq!(resolved.symbol, "Button");
        assert!(build.diagnostics.is_empty());
    }

    #[test]
    fn module_graph_resolves_explicit_aliases_and_tsconfig_paths() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "@/ui/Button";
            import { Card } from "@web/components/Card";
            export const App = () => <div><Button /><Card /></div>;
            "#,
        );
        fixture.write("src/ui/Button.tsx", "export const Button = () => null;");
        fixture.write(
            "apps/web/src/components/Card.tsx",
            "export const Card = () => null;",
        );
        fixture.write(
            "tsconfig.json",
            r#"{
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@web/*": ["apps/web/src/*"]
                }
              }
            }"#,
        );

        let build = fixture.build_graph(
            vec![
                "src/App.tsx",
                "src/ui/Button.tsx",
                "apps/web/src/components/Card.tsx",
            ],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src"), PathBuf::from("apps/web/src")],
                ignore: Vec::new(),
                tsconfig: Some(PathBuf::from("tsconfig.json")),
                aliases: BTreeMap::from([("@/*".to_owned(), vec!["src/*".to_owned()])]),
                packages: BTreeMap::new(),
            },
            registry_with_symbols(&["Button", "Card"]),
        );

        let button = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "Button")
            .expect("Button alias should resolve");
        let card = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "Card")
            .expect("Card alias should resolve");
        assert_eq!(button.module, PathBuf::from("src/ui/Button.tsx"));
        assert_eq!(
            card.module,
            PathBuf::from("apps/web/src/components/Card.tsx")
        );
        assert!(build.diagnostics.is_empty());
    }

    #[test]
    fn module_graph_resolves_package_entrypoints_and_emits_entrypoint_diagnostics() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"import { Button } from "@acme/design-system"; export const App = () => <Button />;"#,
        );
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let build = fixture.build_graph(
            vec!["src/App.tsx", "src/ds/Button.tsx"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src")],
                ignore: Vec::new(),
                tsconfig: None,
                aliases: BTreeMap::new(),
                packages: BTreeMap::from([(
                    "@acme/design-system".to_owned(),
                    PackageConfig {
                        exports: BTreeMap::from([
                            ("Button".to_owned(), "src/ds/Button".to_owned()),
                            ("Missing".to_owned(), "src/ds/Missing".to_owned()),
                        ]),
                    },
                )]),
            },
            registry_with_symbols(&["Button"]),
        );

        let resolved = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "Button")
            .expect("package entrypoint should resolve");
        assert_eq!(resolved.module, PathBuf::from("src/ds/Button.tsx"));
        assert!(
            build
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "package_entrypoint_unresolved")
        );
    }

    #[test]
    fn module_graph_emits_registry_diagnostic_for_unresolved_default_import() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"import Button from "@/missing/Button"; export const App = () => <Button />;"#,
        );

        let build = fixture.build_graph(
            vec!["src/App.tsx"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src")],
                ignore: Vec::new(),
                tsconfig: None,
                aliases: BTreeMap::from([("@/*".to_owned(), vec!["src/*".to_owned()])]),
                packages: BTreeMap::new(),
            },
            registry_with_symbols(&["Button"]),
        );

        assert!(
            build
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ds_import_unresolved")
        );
    }

    #[test]
    fn module_graph_skips_wildcard_package_entrypoint_validation() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"import { Button } from "@acme/design-system/Button"; export const App = () => <Button />;"#,
        );
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let build = fixture.build_graph(
            vec!["src/App.tsx", "src/ds/Button.tsx"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src")],
                ignore: Vec::new(),
                tsconfig: None,
                aliases: BTreeMap::new(),
                packages: BTreeMap::from([(
                    "@acme/design-system".to_owned(),
                    PackageConfig {
                        exports: BTreeMap::from([("./*".to_owned(), "src/ds/*".to_owned())]),
                    },
                )]),
            },
            registry_with_symbols(&["Button"]),
        );

        assert!(
            build
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "package_entrypoint_unresolved")
        );
        let resolved = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "Button")
            .expect("wildcard package entrypoint should resolve");
        assert_eq!(resolved.module, PathBuf::from("src/ds/Button.tsx"));
    }

    #[test]
    fn module_graph_resolves_nested_tsconfig_base_url() {
        let fixture = Fixture::new();
        fixture.write(
            "apps/web/src/App.tsx",
            r#"import { Card } from "@web/components/Card"; export const App = () => <Card />;"#,
        );
        fixture.write(
            "apps/web/src/components/Card.tsx",
            "export const Card = () => null;",
        );
        fixture.write(
            "apps/web/tsconfig.json",
            r#"{
              "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                  "@web/*": ["src/*"]
                }
              }
            }"#,
        );

        let build = fixture.build_graph(
            vec!["apps/web/src/App.tsx", "apps/web/src/components/Card.tsx"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("apps/web/src")],
                ignore: Vec::new(),
                tsconfig: Some(PathBuf::from("apps/web/tsconfig.json")),
                aliases: BTreeMap::new(),
                packages: BTreeMap::new(),
            },
            registry_with_symbols(&["Card"]),
        );

        let resolved = build
            .graph
            .resolve_import(Path::new("apps/web/src/App.tsx"), "Card")
            .expect("nested tsconfig baseUrl should resolve");
        assert_eq!(
            resolved.module,
            PathBuf::from("apps/web/src/components/Card.tsx")
        );
        assert!(build.diagnostics.is_empty());
    }

    #[test]
    fn module_graph_prefers_more_specific_alias_patterns() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"import { Button } from "@acme/ui/Button"; export const App = () => <Button />;"#,
        );
        fixture.write(
            "src/acme/ui/Button.tsx",
            "export const Button = () => null;",
        );
        fixture.write("src/ui/Button.tsx", "export const Button = () => null;");

        let build = fixture.build_graph(
            vec!["src/App.tsx", "src/acme/ui/Button.tsx", "src/ui/Button.tsx"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src")],
                ignore: Vec::new(),
                tsconfig: None,
                aliases: BTreeMap::from([
                    ("@/*".to_owned(), vec!["src/*".to_owned()]),
                    ("@acme/*".to_owned(), vec!["src/acme/*".to_owned()]),
                ]),
                packages: BTreeMap::new(),
            },
            registry_with_symbols(&["Button"]),
        );

        let resolved = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "Button")
            .expect("specific alias should win");
        assert_eq!(resolved.module, PathBuf::from("src/acme/ui/Button.tsx"));
    }

    #[test]
    fn module_graph_resolves_default_export_to_declared_symbol_name() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"import DsButton from "./Button"; export const App = () => <DsButton />;"#,
        );
        fixture.write(
            "src/Button.tsx",
            "export default function Button() { return null; }",
        );

        let build = fixture.build_graph(
            vec!["src/App.tsx", "src/Button.tsx"],
            base_config(),
            registry_with_symbols(&["Button"]),
        );

        let resolved = build
            .graph
            .resolve_import(Path::new("src/App.tsx"), "DsButton")
            .expect("default import should resolve to declared export name");
        assert_eq!(resolved.module, PathBuf::from("src/Button.tsx"));
        assert_eq!(resolved.symbol, "Button");
        assert!(build.diagnostics.is_empty());
    }

    #[test]
    fn module_graph_emits_design_system_unresolved_import_and_export_diagnostics() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "@acme/design-system";
            export { Card } from "@acme/design-system";
            export const App = () => <Button />;
            "#,
        );

        let build = fixture.build_graph(
            vec!["src/App.tsx"],
            ReactScanConfig {
                design_system_registry: PathBuf::from("design-system/registry.json"),
                roots: vec![PathBuf::from("src")],
                ignore: Vec::new(),
                tsconfig: None,
                aliases: BTreeMap::new(),
                packages: BTreeMap::from([(
                    "@acme/design-system".to_owned(),
                    PackageConfig {
                        exports: BTreeMap::from([(".".to_owned(), "src/ds/index.ts".to_owned())]),
                    },
                )]),
            },
            registry_with_symbols(&["Button", "Card"]),
        );

        assert!(
            build
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ds_import_unresolved")
        );
        assert!(
            build
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ds_export_unresolved")
        );
    }

    fn base_config() -> ReactScanConfig {
        ReactScanConfig {
            design_system_registry: PathBuf::from("design-system/registry.json"),
            roots: vec![PathBuf::from("src")],
            ignore: Vec::new(),
            tsconfig: None,
            aliases: BTreeMap::new(),
            packages: BTreeMap::new(),
        }
    }

    fn registry_with_symbols(symbols: &[&str]) -> ReactRegistryIndex {
        let mut resolve_targets = BTreeMap::new();
        let mut component_packages = BTreeMap::new();
        for symbol in symbols {
            resolve_targets.insert((*symbol).to_owned(), (*symbol).to_owned());
            component_packages.insert((*symbol).to_owned(), None);
        }
        ReactRegistryIndex {
            design_system_components: Vec::new(),
            resolve_targets,
            component_packages,
            design_system_tokens: Vec::new(),
            token_index: Default::default(),
        }
    }

    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().expect("tempdir"),
            }
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.root.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("fixture parent dir");
            }
            fs::write(path, contents).expect("fixture write");
        }

        fn build_graph(
            &self,
            module_files: Vec<&str>,
            config: ReactScanConfig,
            registry: ReactRegistryIndex,
        ) -> ReactModuleGraphBuild {
            let parsed_modules = module_files
                .iter()
                .map(|file| {
                    match parse_react_source_file(self.root.path(), Path::new(file))
                        .expect("parse should not fail fatally")
                    {
                        ReactParseOutcome::Parsed(parsed) => parsed,
                        ReactParseOutcome::Failed(diagnostic) => {
                            panic!("unexpected parse failure: {diagnostic:?}")
                        }
                    }
                })
                .collect::<Vec<_>>();
            let files = ReactSourceFileCollection {
                files: module_files
                    .into_iter()
                    .map(PathBuf::from)
                    .collect::<Vec<PathBuf>>(),
                root_diagnostics: Vec::new(),
            };
            build_react_module_graph(
                self.root.path(),
                &parsed_modules,
                &files,
                &config,
                &registry,
            )
        }
    }
}
