//! React registry symbol discovery.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use swc_common::{GLOBALS, Globals, Mark};
use swc_ecma_ast::{
    Callee, Decl, DefaultDecl, Expr, Id, ImportSpecifier, MemberProp, ModuleDecl, ModuleItem, Stmt,
    VarDeclarator,
};
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::VisitMutWith;
use wax_contract::{Diagnostic, DiagnosticSeverity};
use wax_lang_api::DiscoveredRegistrySymbol;

use crate::component_detect::{
    LocalValue, LocalValues, class_returns_jsx, expression_returns_element, expression_returns_jsx,
    function_returns_jsx, is_jsx_expression, module_export_name, simple_binding_ident,
};
use crate::public_exports::{PackageManifest, package_for_file, public_bindings};
use crate::swc_parse::{ParsedReactModule, ReactParseOutcome, parse_react_source_file};

/// Result of discovering React registry symbols from source roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverSymbolsResult {
    /// Discovered design-system symbols with optional package identity.
    pub components: Vec<DiscoveredRegistrySymbol>,
    /// Structured diagnostics emitted while discovering symbols.
    pub diagnostics: Vec<Diagnostic>,
}

impl DiscoverSymbolsResult {
    /// Returns discovered symbol names in stable order.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        DiscoveredRegistrySymbol::symbol_names(&self.components)
    }
}

/// Errors produced while discovering React registry symbols or enriching
/// registry details from source.
#[derive(Debug)]
pub enum ReactDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// A configured discovery root does not exist.
    MissingRoot(PathBuf),
    /// The input or enriched registry violates the shared registry contract.
    InvalidRegistry(wax_contract::RegistryError),
    /// A filesystem operation failed.
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for ReactDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid react language id: {id}"),
            Self::MissingRoot(path) => {
                write!(f, "discovery root does not exist: {}", path.display())
            }
            Self::InvalidRegistry(error) => write!(f, "invalid React registry: {error}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReactDiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::MissingRoot(_) => None,
            Self::InvalidRegistry(error) => Some(error),
            Self::Io { source, .. } => Some(source),
        }
    }
}

pub(crate) type DamagedReactSources = BTreeMap<String, Option<Rc<PackageManifest>>>;

/// Successfully parsed React source file with package metadata.
#[derive(Debug)]
pub(crate) struct ParsedReactSourceFile {
    /// Nearest named npm package, when available.
    pub package: Option<Rc<PackageManifest>>,
    /// Parsed module AST and source metadata (`parsed.file` is repo-relative).
    pub parsed: ParsedReactModule,
}

/// Discovers likely public React design-system component symbols from source roots.
///
/// Files that fail to parse are skipped and reported as diagnostics so discovery can
/// continue with the remaining React sources.
///
/// # Errors
///
/// Returns [`ReactDiscoverError::MissingRoot`] for a nonexistent root or
/// [`ReactDiscoverError::Io`] when source discovery or reading fails.
pub fn discover_registry_symbols(
    parse_root: &Path,
    roots: &[PathBuf],
) -> Result<DiscoverSymbolsResult, ReactDiscoverError> {
    let mut diagnostics = Vec::new();
    let (parsed_files, damaged_paths) =
        collect_and_parse_react_sources(parse_root, roots, false, &mut diagnostics)?;

    let mut components = BTreeMap::<String, Option<String>>::new();
    for binding in public_bindings(
        parse_root,
        roots,
        &parsed_files,
        &damaged_paths,
        &mut diagnostics,
    )
    .bindings
    {
        if let Some(symbol) = binding.component_symbol {
            insert_discovered_symbol(
                &mut components,
                &mut diagnostics,
                symbol,
                binding.package.map(str::to_owned),
            );
        }
    }

    Ok(DiscoverSymbolsResult {
        components: components
            .into_iter()
            .map(|(symbol, package)| DiscoveredRegistrySymbol::new(symbol, package))
            .collect(),
        diagnostics,
    })
}

/// Walks React roots, parses each file, appends parse diagnostics, and returns
/// successful parses plus repo-relative paths that failed to parse.
///
/// # Errors
///
/// Returns [`ReactDiscoverError::MissingRoot`] for a nonexistent root or
/// [`ReactDiscoverError::Io`] when source discovery or reading fails.
pub(crate) fn collect_and_parse_react_sources(
    parse_root: &Path,
    roots: &[PathBuf],
    include_declarations: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(Vec<ParsedReactSourceFile>, DamagedReactSources), ReactDiscoverError> {
    let mut source_files = Vec::new();
    for root in roots {
        if !root.exists() {
            return Err(ReactDiscoverError::MissingRoot(root.clone()));
        }
        collect_react_files(root, &mut source_files, include_declarations).map_err(|source| {
            ReactDiscoverError::Io {
                context: format!("read React root {}", root.display()),
                source,
            }
        })?;
    }
    source_files.sort();
    source_files.dedup();

    let mut parsed_files = Vec::new();
    let mut damaged_paths = BTreeMap::new();
    let mut manifests = BTreeMap::new();
    for file_path in source_files {
        let relative_path = repo_relative_path(parse_root, &file_path);
        let package = package_for_file(parse_root, Path::new(&relative_path), &mut manifests);
        match parse_react_source_file(parse_root, Path::new(&relative_path)).map_err(|err| {
            let source = match err {
                crate::ReactParseError::Io { source, .. } => source,
                other => std::io::Error::other(other),
            };
            ReactDiscoverError::Io {
                context: format!("read React source {relative_path}"),
                source,
            }
        })? {
            ReactParseOutcome::Parsed(mut parsed) => {
                GLOBALS.set(&Globals::new(), || {
                    parsed
                        .module
                        .visit_mut_with(&mut resolver(Mark::new(), Mark::new(), true));
                });
                parsed_files.push(ParsedReactSourceFile { package, parsed });
            }
            ReactParseOutcome::Failed(diagnostic) => {
                damaged_paths.insert(relative_path, package);
                diagnostics.push(diagnostic);
            }
        }
    }

    Ok((parsed_files, damaged_paths))
}

fn insert_discovered_symbol(
    components: &mut BTreeMap<String, Option<String>>,
    diagnostics: &mut Vec<Diagnostic>,
    symbol: String,
    package: Option<String>,
) {
    if let Some(existing) = components.get(&symbol) {
        if existing.as_ref() != package.as_ref() && existing.is_some() && package.is_some() {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "discover_package_conflict".to_owned(),
                message: format!(
                    "symbol '{symbol}' was discovered in multiple packages; omitting package metadata"
                ),
                location: None,
            });
            components.insert(symbol, None);
        }
        return;
    }

    components.insert(symbol, package);
}

pub(crate) fn repo_relative_path(parse_root: &Path, file_path: &Path) -> String {
    file_path
        .strip_prefix(parse_root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| {
            file_path
                .file_name()
                .map(|name| name.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| file_path.to_string_lossy().replace('\\', "/"))
        })
}

fn collect_react_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    include_declarations: bool,
) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)?.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if path_components_contain(&path, "node_modules")
                || path_components_contain(&path, "generated")
                || path_components_contain(&path, "__generated__")
            {
                continue;
            }
            collect_react_files(&path, files, include_declarations)?;
        } else if is_supported_react_source(&path)
            || (include_declarations && is_declaration_file(&path))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn path_components_contain(path: &Path, name: &str) -> bool {
    path.components()
        .any(|component| component.as_os_str() == name)
}

pub(crate) fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".d.ts"))
}

fn is_supported_react_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.ends_with(".d.ts")
        || name.ends_with(".stories.js")
        || name.ends_with(".stories.jsx")
        || name.ends_with(".stories.ts")
        || name.ends_with(".stories.tsx")
        || name.ends_with(".spec.js")
        || name.ends_with(".spec.jsx")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.tsx")
        || name.ends_with(".test.js")
        || name.ends_with(".test.jsx")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
    {
        return false;
    }
    name.ends_with(".jsx")
        || name.ends_with(".tsx")
        || name.ends_with(".js")
        || name.ends_with(".ts")
}

#[derive(Debug, Default)]
pub(crate) struct ReactWrapperCallees<'a> {
    locals: LocalValues<'a>,
    imports: HashMap<Id, (&'static str, Option<String>)>,
}

pub(crate) fn react_wrapper_callees(items: &[ModuleItem]) -> ReactWrapperCallees<'_> {
    let mut callees = ReactWrapperCallees {
        locals: LocalValues::collect(items),
        ..ReactWrapperCallees::default()
    };

    for item in items {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
            continue;
        };
        let package = match import.src.value.to_string_lossy().as_ref() {
            "react" if !import.type_only => "react",
            "react-dom" if !import.type_only => "react-dom",
            _ => continue,
        };
        for specifier in &import.specifiers {
            let (local, name) = match specifier {
                ImportSpecifier::Named(named) if !named.is_type_only => (
                    &named.local,
                    Some(
                        named
                            .imported
                            .as_ref()
                            .map_or_else(|| named.local.sym.to_string(), module_export_name),
                    ),
                ),
                ImportSpecifier::Named(_) => continue,
                ImportSpecifier::Default(default) => (&default.local, None),
                ImportSpecifier::Namespace(namespace) => (&namespace.local, None),
            };
            callees.imports.insert(local.to_id(), (package, name));
        }
    }

    callees
}

impl<'a> ReactWrapperCallees<'a> {
    pub(crate) fn callback_returns_element(&self, expr: &Expr) -> bool {
        expression_returns_element(expr, &|call, jsx_bindings| {
            let Some(first) = call.args.first().filter(|arg| arg.spread.is_none()) else {
                return false;
            };
            self.matches(&call.callee, "react", "cloneElement")
                || (self.matches(&call.callee, "react-dom", "createPortal")
                    && is_jsx_expression(&first.expr, jsx_bindings))
        })
    }

    pub(crate) fn factory_component(&self, expr: &'a Expr) -> Option<LocalValue<'a>> {
        let Expr::Call(call) = expr.unwrap_parens() else {
            return None;
        };
        let Callee::Expr(callee) = &call.callee else {
            return None;
        };
        let Expr::Ident(factory) = callee.unwrap_parens() else {
            return None;
        };
        let factory = self.locals.get(&factory.to_id())?;
        let (block, returned) = factory.factory_return()?;
        if !factory.initialized_before(block, returned, call) {
            return None;
        }
        let binding = |ident: &swc_ecma_ast::Ident| {
            let value = self.locals.factory_binding(block, &ident.to_id())?;
            value
                .initialized_before(block, returned, call)
                .then_some(value)
        };
        let value = match returned.unwrap_parens() {
            Expr::Ident(ident) => binding(ident)?,
            expr @ Expr::Call(_) => LocalValue::Constant(expr),
            _ => return None,
        };
        match value {
            LocalValue::Function(function) if function_returns_jsx(function) => Some(value),
            LocalValue::Class(class) if class_returns_jsx(class) => Some(value),
            LocalValue::Constant(expr) => {
                let expr = expr.unwrap_parens();
                if expression_returns_jsx(expr) {
                    return Some(LocalValue::Constant(expr));
                }
                match self.wrapper_target(expr)? {
                    Expr::Ident(ident) => match binding(ident)? {
                        value @ LocalValue::Function(function)
                            if function_returns_jsx(function) =>
                        {
                            Some(value)
                        }
                        value @ LocalValue::Class(class) if class_returns_jsx(class) => Some(value),
                        value @ LocalValue::Constant(expr) if expression_returns_jsx(expr) => {
                            Some(value)
                        }
                        _ => None,
                    },
                    callback => Some(LocalValue::Constant(callback)),
                }
            }
            _ => None,
        }
    }

    pub(crate) fn wrapper_target<'expr>(&self, mut expr: &'expr Expr) -> Option<&'expr Expr> {
        loop {
            let Expr::Call(call) = expr.unwrap_parens() else {
                return None;
            };
            let [arg] = call.args.as_slice() else {
                return None;
            };
            if arg.spread.is_some() {
                return None;
            }
            let memo = self.matches(&call.callee, "react", "memo");
            if !memo && !self.matches(&call.callee, "react", "forwardRef") {
                return None;
            }
            expr = arg.expr.unwrap_parens();
            match expr {
                Expr::Call(_) if memo => continue,
                Expr::Ident(_) if memo => return Some(expr),
                _ => return self.callback_returns_element(expr).then_some(expr),
            }
        }
    }

    fn matches(&self, callee: &Callee, package: &str, expected: &str) -> bool {
        let Callee::Expr(expr) = callee else {
            return false;
        };
        match &**expr {
            Expr::Ident(ident) => self
                .imports
                .get(&ident.to_id())
                .is_some_and(|(source, name)| {
                    *source == package && name.as_deref() == Some(expected)
                }),
            Expr::Member(member)
                if let Expr::Ident(object) = &*member.obj
                    && matches!(&member.prop, MemberProp::Ident(prop) if prop.sym == expected) =>
            {
                self.imports
                    .get(&object.to_id())
                    .is_some_and(|(source, name)| *source == package && name.is_none())
            }
            _ => false,
        }
    }
}

pub(crate) fn local_component_bindings(
    items: &[ModuleItem],
    wrapper_callees: &ReactWrapperCallees<'_>,
) -> BTreeSet<String> {
    let mut detected = BTreeSet::new();

    for item in items {
        collect_jsx_returning_declaration(item, &mut detected);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for item in items {
            if collect_wrapper_declaration(item, &mut detected, wrapper_callees) {
                changed = true;
            }
        }
    }

    detected
}

fn collect_jsx_returning_declaration(item: &ModuleItem, detected: &mut BTreeSet<String>) {
    match item {
        ModuleItem::Stmt(Stmt::Decl(decl)) => collect_decl_component(decl, detected),
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
            collect_decl_component(&export_decl.decl, detected);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
            match &default_decl.decl {
                DefaultDecl::Fn(fn_expr)
                    if let Some(ident) = &fn_expr.ident
                        && function_returns_jsx(&fn_expr.function) =>
                {
                    detected.insert(ident.sym.to_string());
                }
                DefaultDecl::Class(class_expr)
                    if let Some(ident) = &class_expr.ident
                        && class_returns_jsx(&class_expr.class) =>
                {
                    detected.insert(ident.sym.to_string());
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn collect_decl_component(decl: &Decl, detected: &mut BTreeSet<String>) {
    match decl {
        Decl::Fn(fn_decl) if function_returns_jsx(&fn_decl.function) => {
            detected.insert(fn_decl.ident.sym.to_string());
        }
        Decl::Class(class_decl) if class_returns_jsx(&class_decl.class) => {
            detected.insert(class_decl.ident.sym.to_string());
        }
        Decl::Var(var_decl) => {
            for declarator in &var_decl.decls {
                if let Some((name, _)) = simple_binding_ident(declarator)
                    && declarator
                        .init
                        .as_deref()
                        .is_some_and(expression_returns_jsx)
                {
                    detected.insert(name);
                }
            }
        }
        _ => {}
    }
}

fn collect_wrapper_declaration(
    item: &ModuleItem,
    detected: &mut BTreeSet<String>,
    wrapper_callees: &ReactWrapperCallees<'_>,
) -> bool {
    match item {
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(var_decl)))
        | ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(swc_ecma_ast::ExportDecl {
            decl: Decl::Var(var_decl),
            ..
        })) => var_decl.decls.iter().any(|declarator| {
            collect_wrapper_var_declarator(declarator, detected, wrapper_callees)
        }),
        _ => false,
    }
}

fn collect_wrapper_var_declarator(
    declarator: &VarDeclarator,
    detected: &mut BTreeSet<String>,
    wrapper_callees: &ReactWrapperCallees<'_>,
) -> bool {
    let Some((binding_name, _)) = simple_binding_ident(declarator) else {
        return false;
    };
    if detected.contains(&binding_name) {
        return false;
    }

    let Some(init) = declarator.init.as_deref() else {
        return false;
    };

    if wrapper_callees.factory_component(init).is_some()
        || detected_wrapper_target(init, detected, wrapper_callees).is_some()
    {
        return detected.insert(binding_name);
    }

    false
}

fn detected_wrapper_target<'a>(
    expr: &'a Expr,
    detected: &BTreeSet<String>,
    wrapper_callees: &ReactWrapperCallees<'_>,
) -> Option<&'a Expr> {
    let target = wrapper_callees.wrapper_target(expr)?;
    if let Expr::Ident(ident) = target
        && !detected.contains(ident.sym.as_ref())
    {
        return None;
    }
    Some(target)
}

pub(crate) fn default_expression_component(
    expr: &Expr,
    detected: &BTreeSet<String>,
    wrapper_callees: &ReactWrapperCallees<'_>,
) -> Option<String> {
    if expression_returns_jsx(expr) {
        return Some(expression_component_name(expr));
    }
    detected_wrapper_target(expr, detected, wrapper_callees).map(expression_component_name)
}

fn expression_component_name(expr: &Expr) -> String {
    let ident = match expr {
        Expr::Fn(function) => function.ident.as_ref(),
        Expr::Ident(ident) => Some(ident),
        _ => None,
    };
    ident.map_or_else(|| "default".to_owned(), |ident| ident.sym.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::path::Path;

    #[test]
    fn is_supported_react_source_excludes_tests_stories_and_d_ts() {
        assert!(is_supported_react_source(Path::new("Button.tsx")));
        assert!(is_supported_react_source(Path::new("Button.jsx")));
        assert!(is_supported_react_source(Path::new("Button.js")));
        assert!(!is_supported_react_source(Path::new("Button.test.tsx")));
        assert!(!is_supported_react_source(Path::new("Button.spec.ts")));
        assert!(!is_supported_react_source(Path::new("Button.stories.tsx")));
        assert!(!is_supported_react_source(Path::new("types.d.ts")));
    }

    #[test]
    fn collect_react_files_skips_node_modules_and_generated_dirs() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let root = tempdir.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("node_modules/pkg"))?;
        fs::create_dir_all(root.join("generated"))?;
        fs::create_dir_all(root.join("__generated__"))?;
        fs::write(
            root.join("src/Button.tsx"),
            "export function Button() { return <button />; }",
        )?;
        fs::write(
            root.join("node_modules/pkg/Hidden.tsx"),
            "export function Hidden() { return <div />; }",
        )?;
        fs::write(
            root.join("generated/Gen.tsx"),
            "export function Gen() { return <div />; }",
        )?;
        fs::write(
            root.join("__generated__/Auto.tsx"),
            "export function Auto() { return <div />; }",
        )?;

        let mut files = Vec::new();
        collect_react_files(root, &mut files, false)?;
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("src/Button.tsx"));
        Ok(())
    }
}
