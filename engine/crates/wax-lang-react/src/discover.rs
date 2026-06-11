//! React registry symbol discovery.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use swc_common::Span;
use swc_ecma_ast::{
    BinaryOp, BlockStmt, BlockStmtOrExpr, Callee, Class, ClassMember, Decl, DefaultDecl,
    ExportSpecifier, Expr, Function, ImportSpecifier, MemberProp, ModuleDecl, ModuleExportName,
    ModuleItem, Pat, Stmt, VarDeclarator,
};

use crate::swc_parse::{ReactParseOutcome, parse_react_source_file};

/// Errors produced while discovering React registry symbols.
#[derive(Debug)]
pub enum ReactDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// A configured discovery root does not exist.
    MissingRoot(PathBuf),
    /// A React source file could not be parsed successfully.
    ParseFailed(PathBuf),
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
            Self::ParseFailed(path) => {
                write!(f, "failed to parse React source {}", path.display())
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReactDiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::MissingRoot(_) | Self::ParseFailed(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Discovers likely public React design-system component symbols from source roots.
pub fn discover_registry_symbols(roots: &[PathBuf]) -> Result<Vec<String>, ReactDiscoverError> {
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

    let mut symbols = BTreeSet::new();
    for file_path in source_files {
        let parse_root = parse_root_for_file(&file_path);
        let relative_path = file_path.strip_prefix(&parse_root).unwrap_or(&file_path);
        let parsed =
            match parse_react_source_file(&parse_root, relative_path).map_err(|source| {
                ReactDiscoverError::Io {
                    context: format!("read React source {}", file_path.display()),
                    source: match source {
                        crate::ReactParseError::Io { source, .. } => source,
                    },
                }
            })? {
                ReactParseOutcome::Parsed(parsed) => parsed,
                ReactParseOutcome::Failed(_) => {
                    return Err(ReactDiscoverError::ParseFailed(file_path));
                }
            };
        collect_exported_component_symbols(&parsed.module.body, &mut symbols);
    }

    Ok(symbols.into_iter().collect())
}

fn parse_root_for_file(file_path: &Path) -> PathBuf {
    file_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
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

fn function_returns_jsx(function: &Function) -> bool {
    function.body.as_ref().is_some_and(block_returns_jsx)
}

fn class_returns_jsx(class: &Class) -> bool {
    class.body.iter().any(|member| {
        let ClassMember::Method(method) = member else {
            return false;
        };
        if !matches!(&method.key, swc_ecma_ast::PropName::Ident(ident) if ident.sym.as_ref() == "render")
        {
            return false;
        }
        function_returns_jsx(&method.function)
    })
}

fn expression_returns_jsx(expr: &Expr) -> bool {
    match expr {
        Expr::Arrow(arrow) => match &*arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => block_returns_jsx(block),
            BlockStmtOrExpr::Expr(expr) => is_jsx_expression(expr, &BTreeSet::new()),
        },
        Expr::Fn(fn_expr) => function_returns_jsx(&fn_expr.function),
        _ => false,
    }
}

fn block_returns_jsx(block: &BlockStmt) -> bool {
    let mut jsx_bindings = BTreeSet::new();
    block
        .stmts
        .iter()
        .any(|stmt| stmt_returns_jsx(stmt, &mut jsx_bindings))
}

fn stmt_returns_jsx(stmt: &Stmt, jsx_bindings: &mut BTreeSet<String>) -> bool {
    match stmt {
        Stmt::Decl(Decl::Var(var_decl)) => {
            for declarator in &var_decl.decls {
                if let Some((name, _)) = simple_binding_ident(declarator)
                    && declarator
                        .init
                        .as_deref()
                        .is_some_and(|expr| is_jsx_expression(expr, jsx_bindings))
                {
                    jsx_bindings.insert(name);
                }
            }
            false
        }
        Stmt::Return(return_stmt) => return_stmt
            .arg
            .as_deref()
            .is_some_and(|expr| is_jsx_expression(expr, jsx_bindings)),
        Stmt::Block(block) => {
            let mut nested_bindings = jsx_bindings.clone();
            block
                .stmts
                .iter()
                .any(|stmt| stmt_returns_jsx(stmt, &mut nested_bindings))
        }
        Stmt::If(if_stmt) => {
            let mut consequent_bindings = jsx_bindings.clone();
            let consequent_returns = stmt_returns_jsx(&if_stmt.cons, &mut consequent_bindings);
            let alternate_returns = if_stmt.alt.as_deref().is_some_and(|stmt| {
                let mut alternate_bindings = jsx_bindings.clone();
                stmt_returns_jsx(stmt, &mut alternate_bindings)
            });
            consequent_returns || alternate_returns
        }
        _ => false,
    }
}

fn is_jsx_expression(expr: &Expr, jsx_bindings: &BTreeSet<String>) -> bool {
    match expr {
        Expr::JSXElement(_) | Expr::JSXFragment(_) => true,
        Expr::Ident(ident) => jsx_bindings.contains(ident.sym.as_ref()),
        Expr::Paren(paren) => is_jsx_expression(&paren.expr, jsx_bindings),
        Expr::Cond(cond) => {
            is_jsx_expression(&cond.cons, jsx_bindings)
                || is_jsx_expression(&cond.alt, jsx_bindings)
        }
        Expr::Bin(binary)
            if matches!(
                binary.op,
                BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
            ) =>
        {
            is_jsx_expression(&binary.left, jsx_bindings)
                || is_jsx_expression(&binary.right, jsx_bindings)
        }
        _ => false,
    }
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
            _ => {}
        }
    }

    if callee_matches(&call.callee, "forwardRef", wrapper_callees)
        && let Expr::Fn(fn_expr) = &*call.args[0].expr
    {
        let ident = fn_expr.ident.as_ref()?;
        if function_returns_jsx(&fn_expr.function) {
            return Some((ident.sym.to_string(), ident.span));
        }
    }

    None
}

fn callee_matches(callee: &Callee, expected: &str, wrapper_callees: &ReactWrapperCallees) -> bool {
    let Callee::Expr(expr) = callee else {
        return false;
    };

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

fn simple_binding_ident(declarator: &VarDeclarator) -> Option<(String, Span)> {
    match &declarator.name {
        Pat::Ident(binding) => Some((binding.id.sym.to_string(), binding.id.span)),
        _ => None,
    }
}

fn is_pascal_case(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_string_lossy().to_string(),
    }
}
