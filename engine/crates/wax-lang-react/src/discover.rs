//! React registry symbol discovery.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use swc_common::Span;
use swc_ecma_ast::{
    Callee, Decl, DefaultDecl, ExportSpecifier, Expr, ImportSpecifier, MemberProp, ModuleDecl,
    ModuleItem, Stmt, VarDeclarator,
};
use wax_contract::Diagnostic;
use wax_lang_api::{DiscoveredRegistrySymbol, npm_package_name_for_roots};

use crate::component_detect::{
    class_returns_jsx, expression_returns_jsx, function_returns_jsx, is_pascal_case,
    module_export_name, simple_binding_ident,
};
use crate::swc_parse::{ReactParseOutcome, parse_react_source_file};

/// Result of discovering React registry symbols from source roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverRegistryResult {
    /// Discovered design-system symbols with optional package identity.
    pub components: Vec<DiscoveredRegistrySymbol>,
    /// Structured diagnostics emitted while discovering symbols.
    pub diagnostics: Vec<Diagnostic>,
}

impl DiscoverRegistryResult {
    /// Returns discovered symbol names in stable order.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        DiscoveredRegistrySymbol::symbol_names(&self.components)
    }
}

/// Errors produced while discovering React registry symbols.
#[derive(Debug)]
pub enum ReactDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// A configured discovery root does not exist.
    MissingRoot(PathBuf),
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
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReactDiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::MissingRoot(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Discovers likely public React design-system component symbols from source roots.
///
/// Files that fail to parse are skipped and reported as diagnostics so discovery can
/// continue with the remaining React sources.
pub fn discover_registry_symbols(
    parse_root: &Path,
    roots: &[PathBuf],
) -> Result<DiscoverRegistryResult, ReactDiscoverError> {
    let mut source_files = Vec::new();
    for root in roots {
        if !root.exists() {
            return Err(ReactDiscoverError::MissingRoot(root.clone()));
        }
        collect_react_files(root, &mut source_files).map_err(|source| ReactDiscoverError::Io {
            context: format!("read React root {}", root.display()),
            source,
        })?;
    }
    source_files.sort();

    let package = npm_package_name_for_roots(parse_root, roots);
    let mut symbols = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for file_path in source_files {
        let relative_path = repo_relative_path(parse_root, &file_path);
        let parsed = match parse_react_source_file(parse_root, Path::new(&relative_path)).map_err(
            |source| ReactDiscoverError::Io {
                context: format!("read React source {relative_path}"),
                source: match source {
                    crate::ReactParseError::Io { source, .. } => source,
                },
            },
        )? {
            ReactParseOutcome::Parsed(parsed) => parsed,
            ReactParseOutcome::Failed(diagnostic) => {
                diagnostics.push(diagnostic);
                continue;
            }
        };
        collect_exported_component_symbols(&parsed.module.body, &mut symbols);
    }

    Ok(DiscoverRegistryResult {
        components: symbols
            .into_iter()
            .map(|symbol| DiscoveredRegistrySymbol::new(symbol, package.clone()))
            .collect(),
        diagnostics,
    })
}

fn repo_relative_path(parse_root: &Path, file_path: &Path) -> String {
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

fn collect_react_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
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
            collect_react_files(&path, files)?;
        } else if is_supported_react_source(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn path_components_contain(path: &Path, name: &str) -> bool {
    path.components()
        .any(|component| component.as_os_str() == name)
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

fn collect_exported_component_symbols(items: &[ModuleItem], symbols: &mut BTreeSet<String>) {
    let wrapper_callees = react_wrapper_callees(items);
    let local_components = local_component_bindings(items, &wrapper_callees);

    for item in items {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
                collect_export_decl_symbols(&export_decl.decl, &local_components, symbols);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
                collect_default_decl_symbol(default_decl, symbols);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr)) => {
                match &*default_expr.expr {
                    Expr::Ident(ident)
                        if is_pascal_case(ident.sym.as_ref())
                            && local_components.contains(ident.sym.as_ref()) =>
                    {
                        symbols.insert(ident.sym.to_string());
                    }
                    expr => {
                        if let Some((name, _)) =
                            default_wrapper_component(expr, &local_components, &wrapper_callees)
                            && is_pascal_case(&name)
                        {
                            symbols.insert(name);
                        }
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named_export))
                if named_export.src.is_none() =>
            {
                for specifier in &named_export.specifiers {
                    if let ExportSpecifier::Named(named) = specifier {
                        let local_name = module_export_name(&named.orig);
                        if is_pascal_case(&local_name)
                            && local_components.contains(&local_name)
                            && named.exported.is_none()
                        {
                            symbols.insert(local_name);
                            // Aliased local exports are intentionally left for a follow-up pass:
                            // registry discover v1 favors unambiguous public symbol names.
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_export_decl_symbols(
    decl: &Decl,
    local_components: &BTreeSet<String>,
    symbols: &mut BTreeSet<String>,
) {
    match decl {
        Decl::Fn(fn_decl)
            if is_pascal_case(fn_decl.ident.sym.as_ref())
                && function_returns_jsx(&fn_decl.function) =>
        {
            symbols.insert(fn_decl.ident.sym.to_string());
        }
        Decl::Class(class_decl)
            if is_pascal_case(class_decl.ident.sym.as_ref())
                && class_returns_jsx(&class_decl.class) =>
        {
            symbols.insert(class_decl.ident.sym.to_string());
        }
        Decl::Var(var_decl) => {
            for declarator in &var_decl.decls {
                if let Some((name, _)) = simple_binding_ident(declarator)
                    && is_pascal_case(&name)
                    && local_components.contains(&name)
                {
                    symbols.insert(name);
                }
            }
        }
        _ => {}
    }
}

fn collect_default_decl_symbol(
    default_decl: &swc_ecma_ast::ExportDefaultDecl,
    symbols: &mut BTreeSet<String>,
) {
    match &default_decl.decl {
        DefaultDecl::Fn(fn_expr)
            if let Some(ident) = &fn_expr.ident
                && is_pascal_case(ident.sym.as_ref())
                && function_returns_jsx(&fn_expr.function) =>
        {
            symbols.insert(ident.sym.to_string());
        }
        DefaultDecl::Class(class_expr)
            if let Some(ident) = &class_expr.ident
                && is_pascal_case(ident.sym.as_ref())
                && class_returns_jsx(&class_expr.class) =>
        {
            symbols.insert(ident.sym.to_string());
        }
        _ => {}
    }
}

#[derive(Debug, Default)]
struct ReactWrapperCallees {
    direct_memo: BTreeSet<String>,
    direct_forward_ref: BTreeSet<String>,
    react_namespaces: BTreeSet<String>,
}

fn react_wrapper_callees(items: &[ModuleItem]) -> ReactWrapperCallees {
    let mut callees = ReactWrapperCallees::default();

    for item in items {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import_decl)) = item else {
            continue;
        };
        if import_decl.src.value.to_string_lossy() != "react" {
            continue;
        }

        for specifier in &import_decl.specifiers {
            match specifier {
                ImportSpecifier::Named(named) => {
                    let imported_name = named
                        .imported
                        .as_ref()
                        .map(module_export_name)
                        .unwrap_or_else(|| named.local.sym.to_string());
                    match imported_name.as_str() {
                        "memo" => {
                            callees.direct_memo.insert(named.local.sym.to_string());
                        }
                        "forwardRef" => {
                            callees
                                .direct_forward_ref
                                .insert(named.local.sym.to_string());
                        }
                        _ => {}
                    }
                }
                ImportSpecifier::Default(default) => {
                    callees
                        .react_namespaces
                        .insert(default.local.sym.to_string());
                }
                ImportSpecifier::Namespace(namespace) => {
                    callees
                        .react_namespaces
                        .insert(namespace.local.sym.to_string());
                }
            }
        }
    }

    callees
}

fn local_component_bindings(
    items: &[ModuleItem],
    wrapper_callees: &ReactWrapperCallees,
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
                        && is_pascal_case(ident.sym.as_ref())
                        && function_returns_jsx(&fn_expr.function) =>
                {
                    detected.insert(ident.sym.to_string());
                }
                DefaultDecl::Class(class_expr)
                    if let Some(ident) = &class_expr.ident
                        && is_pascal_case(ident.sym.as_ref())
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
        Decl::Fn(fn_decl)
            if is_pascal_case(fn_decl.ident.sym.as_ref())
                && function_returns_jsx(&fn_decl.function) =>
        {
            detected.insert(fn_decl.ident.sym.to_string());
        }
        Decl::Class(class_decl)
            if is_pascal_case(class_decl.ident.sym.as_ref())
                && class_returns_jsx(&class_decl.class) =>
        {
            detected.insert(class_decl.ident.sym.to_string());
        }
        Decl::Var(var_decl) => {
            for declarator in &var_decl.decls {
                if let Some((name, _)) = simple_binding_ident(declarator)
                    && is_pascal_case(&name)
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
    wrapper_callees: &ReactWrapperCallees,
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
    wrapper_callees: &ReactWrapperCallees,
) -> bool {
    let Some((binding_name, _)) = simple_binding_ident(declarator) else {
        return false;
    };
    if !is_pascal_case(&binding_name) || detected.contains(&binding_name) {
        return false;
    }

    let Some(init) = declarator.init.as_deref() else {
        return false;
    };

    if is_memo_call_of_detected_component(init, detected, wrapper_callees)
        || is_memo_call_of_inline_component(init, wrapper_callees)
        || is_forward_ref_call_of_inline_component(init, wrapper_callees)
    {
        return detected.insert(binding_name);
    }

    false
}

fn is_memo_call_of_detected_component(
    expr: &Expr,
    detected: &BTreeSet<String>,
    wrapper_callees: &ReactWrapperCallees,
) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    if !callee_matches(&call.callee, "memo", wrapper_callees) || call.args.len() != 1 {
        return false;
    }
    let Expr::Ident(component) = &*call.args[0].expr else {
        return false;
    };
    detected.contains(component.sym.as_ref())
}

fn is_memo_call_of_inline_component(expr: &Expr, wrapper_callees: &ReactWrapperCallees) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    if !callee_matches(&call.callee, "memo", wrapper_callees) || call.args.len() != 1 {
        return false;
    }
    expression_returns_jsx(&call.args[0].expr)
        || is_forward_ref_call_of_inline_component(&call.args[0].expr, wrapper_callees)
}

fn is_forward_ref_call_of_inline_component(
    expr: &Expr,
    wrapper_callees: &ReactWrapperCallees,
) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    if !callee_matches(&call.callee, "forwardRef", wrapper_callees) || call.args.len() != 1 {
        return false;
    }
    expression_returns_jsx(&call.args[0].expr)
}

fn default_wrapper_component(
    expr: &Expr,
    detected: &BTreeSet<String>,
    wrapper_callees: &ReactWrapperCallees,
) -> Option<(String, Span)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }

    if callee_matches(&call.callee, "memo", wrapper_callees) {
        match &*call.args[0].expr {
            Expr::Ident(component) if detected.contains(component.sym.as_ref()) => {
                return Some((component.sym.to_string(), component.span));
            }
            Expr::Fn(fn_expr) => {
                let ident = fn_expr.ident.as_ref()?;
                if function_returns_jsx(&fn_expr.function) {
                    return Some((ident.sym.to_string(), ident.span));
                }
            }
            Expr::Call(_) => {
                if let Some(component) =
                    forward_ref_function_component(&call.args[0].expr, wrapper_callees)
                {
                    return Some(component);
                }
            }
            _ => {}
        }
    }

    if let Some(component) = forward_ref_function_component(expr, wrapper_callees) {
        return Some(component);
    }

    None
}

fn forward_ref_function_component(
    expr: &Expr,
    wrapper_callees: &ReactWrapperCallees,
) -> Option<(String, Span)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if !callee_matches(&call.callee, "forwardRef", wrapper_callees) || call.args.len() != 1 {
        return None;
    }
    let Expr::Fn(fn_expr) = &*call.args[0].expr else {
        return None;
    };
    let ident = fn_expr.ident.as_ref()?;
    if !function_returns_jsx(&fn_expr.function) {
        return None;
    }
    Some((ident.sym.to_string(), ident.span))
}

fn callee_matches(callee: &Callee, expected: &str, wrapper_callees: &ReactWrapperCallees) -> bool {
    let Callee::Expr(expr) = callee else {
        return false;
    };

    // Discover is import-aware so registry authoring does not treat arbitrary
    // local memo/forwardRef helpers as public React component wrappers. Scan
    // remains permissive for compatibility with existing local-component facts.
    match &**expr {
        Expr::Ident(ident) => match expected {
            "memo" => wrapper_callees.direct_memo.contains(ident.sym.as_ref()),
            "forwardRef" => wrapper_callees
                .direct_forward_ref
                .contains(ident.sym.as_ref()),
            _ => false,
        },
        Expr::Member(member) => {
            let Expr::Ident(object) = &*member.obj else {
                return false;
            };
            wrapper_callees
                .react_namespaces
                .contains(object.sym.as_ref())
                && matches!(&member.prop, MemberProp::Ident(prop) if prop.sym.as_ref() == expected)
        }
        _ => false,
    }
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
        collect_react_files(root, &mut files)?;
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("src/Button.tsx"));
        Ok(())
    }

    #[test]
    fn react_wrapper_callees_tracks_import_bindings() {
        let parsed = parse_module(
            "src/wrappers.tsx",
            r#"
            import React, { memo, forwardRef as fr } from "react";
            "#,
        );

        let callees = react_wrapper_callees(&parsed.module.body);

        assert!(callees.direct_memo.contains("memo"));
        assert!(callees.direct_forward_ref.contains("fr"));
        assert!(callees.react_namespaces.contains("React"));
    }

    #[test]
    fn exported_symbols_ignore_memo_without_react_import() {
        let parsed = parse_module(
            "src/untracked-memo.tsx",
            r#"
            function memo(_component) {
                return _component;
            }
            function Base() {
                return <button />;
            }
            export const FakeMemo = memo(Base);
            "#,
        );

        let mut symbols = BTreeSet::new();
        collect_exported_component_symbols(&parsed.module.body, &mut symbols);

        assert!(symbols.is_empty());
    }

    #[test]
    fn exported_symbols_include_memo_with_named_react_import() {
        let parsed = parse_module(
            "src/memo-button.tsx",
            r#"
            import { memo } from "react";
            function ButtonBase() {
                return <button />;
            }
            export const MemoButton = memo(ButtonBase);
            "#,
        );

        let mut symbols = BTreeSet::new();
        collect_exported_component_symbols(&parsed.module.body, &mut symbols);

        assert_eq!(symbols.into_iter().collect::<Vec<_>>(), vec!["MemoButton"]);
    }

    #[test]
    fn exported_symbols_skip_barrel_reexports_without_local_implementation() {
        let parsed = parse_module("src/index.ts", r#"export { Button } from "./components";"#);

        let mut symbols = BTreeSet::new();
        collect_exported_component_symbols(&parsed.module.body, &mut symbols);

        assert!(symbols.is_empty());
    }

    fn parse_module(file: &str, source: &str) -> crate::ParsedReactModule {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, source).unwrap();

        match parse_react_source_file(tempdir.path(), Path::new(file))
            .expect("parse should not fail fatally")
        {
            ReactParseOutcome::Parsed(parsed) => parsed,
            ReactParseOutcome::Failed(diagnostic) => {
                panic!("expected parse success, got {}", diagnostic.message)
            }
        }
    }
}
