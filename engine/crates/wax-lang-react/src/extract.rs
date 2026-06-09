//! React component extraction from parsed SWC modules.

use std::collections::{BTreeMap, BTreeSet};

use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    BinaryOp, BlockStmt, BlockStmtOrExpr, Callee, Decl, DefaultDecl, ExportSpecifier, Expr,
    Function, MemberProp, ModuleDecl, ModuleExportName, ModuleItem, Pat, Stmt, VarDeclarator,
};
use wax_contract::{LocalComponent, SourceLocation};

use crate::swc_parse::ParsedReactModule;

/// Discovers repo-local React components in parsed modules.
#[must_use]
pub fn discover_local_components(parsed_modules: &[ParsedReactModule]) -> Vec<LocalComponent> {
    let mut components = BTreeMap::new();

    for parsed in parsed_modules {
        let mut detected = BTreeSet::new();

        for item in &parsed.module.body {
            collect_jsx_returning_declaration(parsed, item, &mut detected, &mut components);
        }

        let mut changed = true;
        while changed {
            changed = false;
            for item in &parsed.module.body {
                if collect_wrapper_declaration(parsed, item, &detected, &mut components) {
                    changed = true;
                }
            }
            detected.extend(
                components
                    .values()
                    .filter(|component| component.location.file == normalize_file(&parsed.file))
                    .map(|component| component.symbol.clone()),
            );
        }
    }

    let mut components = components.into_values().collect::<Vec<_>>();
    components.sort_by(|left, right| {
        left.symbol
            .cmp(&right.symbol)
            .then_with(|| left.location.file.cmp(&right.location.file))
            .then_with(|| left.location.line.cmp(&right.location.line))
            .then_with(|| left.location.column.cmp(&right.location.column))
    });
    components
}

fn collect_jsx_returning_declaration(
    parsed: &ParsedReactModule,
    item: &ModuleItem,
    detected: &mut BTreeSet<String>,
    components: &mut BTreeMap<(String, String), LocalComponent>,
) {
    match item {
        ModuleItem::Stmt(Stmt::Decl(decl)) => {
            collect_decl_component(parsed, decl, detected, components);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
            collect_decl_component(parsed, &export_decl.decl, detected, components);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
            if let DefaultDecl::Fn(fn_expr) = &default_decl.decl
                && let Some(ident) = &fn_expr.ident
                && is_pascal_case(&ident.sym)
                && function_returns_jsx(&fn_expr.function)
            {
                insert_component(
                    parsed,
                    ident.sym.to_string(),
                    ident.span,
                    detected,
                    components,
                );
            }
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr)) => {
            if let Expr::Ident(ident) = &*default_expr.expr
                && is_pascal_case(&ident.sym)
                && detected.contains(ident.sym.as_ref())
            {
                insert_component(
                    parsed,
                    ident.sym.to_string(),
                    ident.span,
                    detected,
                    components,
                );
            }
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named_export))
            if named_export.src.is_none() =>
        {
            for specifier in &named_export.specifiers {
                if let ExportSpecifier::Named(named) = specifier {
                    let local_name = module_export_name(&named.orig);
                    if is_pascal_case(&local_name) && detected.contains(&local_name) {
                        insert_component(
                            parsed,
                            local_name,
                            named.orig.span(),
                            detected,
                            components,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

fn collect_decl_component(
    parsed: &ParsedReactModule,
    decl: &Decl,
    detected: &mut BTreeSet<String>,
    components: &mut BTreeMap<(String, String), LocalComponent>,
) {
    match decl {
        Decl::Fn(fn_decl)
            if is_pascal_case(&fn_decl.ident.sym) && function_returns_jsx(&fn_decl.function) =>
        {
            insert_component(
                parsed,
                fn_decl.ident.sym.to_string(),
                fn_decl.ident.span,
                detected,
                components,
            );
        }
        Decl::Var(var_decl) => {
            for declarator in &var_decl.decls {
                if let Some((name, span)) = simple_binding_ident(declarator)
                    && is_pascal_case(&name)
                    && declarator
                        .init
                        .as_deref()
                        .is_some_and(expression_returns_jsx)
                {
                    insert_component(parsed, name, span, detected, components);
                }
            }
        }
        _ => {}
    }
}

fn collect_wrapper_declaration(
    parsed: &ParsedReactModule,
    item: &ModuleItem,
    detected: &BTreeSet<String>,
    components: &mut BTreeMap<(String, String), LocalComponent>,
) -> bool {
    match item {
        ModuleItem::Stmt(Stmt::Decl(Decl::Var(var_decl)))
        | ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(swc_ecma_ast::ExportDecl {
            decl: Decl::Var(var_decl),
            ..
        })) => var_decl.decls.iter().any(|declarator| {
            collect_wrapper_var_declarator(parsed, declarator, detected, components)
        }),
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr)) => {
            if let Some((name, span)) = forward_ref_function_component(&default_expr.expr)
                && is_pascal_case(&name)
            {
                return insert_component_without_detected(parsed, name, span, components);
            }
            false
        }
        _ => false,
    }
}

fn collect_wrapper_var_declarator(
    parsed: &ParsedReactModule,
    declarator: &VarDeclarator,
    detected: &BTreeSet<String>,
    components: &mut BTreeMap<(String, String), LocalComponent>,
) -> bool {
    let Some((binding_name, binding_span)) = simple_binding_ident(declarator) else {
        return false;
    };
    if !is_pascal_case(&binding_name) {
        return false;
    }

    let Some(init) = declarator.init.as_deref() else {
        return false;
    };

    if is_memo_call_of_detected_component(init, detected) {
        return insert_component_without_detected(parsed, binding_name, binding_span, components);
    }

    if forward_ref_function_component(init).is_some() {
        return insert_component_without_detected(parsed, binding_name, binding_span, components);
    }

    false
}

fn insert_component(
    parsed: &ParsedReactModule,
    symbol: String,
    span: Span,
    detected: &mut BTreeSet<String>,
    components: &mut BTreeMap<(String, String), LocalComponent>,
) {
    detected.insert(symbol.clone());
    insert_component_without_detected(parsed, symbol, span, components);
}

fn insert_component_without_detected(
    parsed: &ParsedReactModule,
    symbol: String,
    span: Span,
    components: &mut BTreeMap<(String, String), LocalComponent>,
) -> bool {
    let file = normalize_file(&parsed.file);
    let Some(location) = parsed.source_location_from_span(span) else {
        return false;
    };
    let key = (file.clone(), symbol.clone());
    if components.contains_key(&key) {
        return false;
    }

    components.insert(
        key,
        LocalComponent {
            id: local_component_id(&file, &symbol, &location),
            symbol,
            location,
        },
    );
    true
}

fn function_returns_jsx(function: &Function) -> bool {
    function.body.as_ref().is_some_and(block_returns_jsx)
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
                if let Some((name, _span)) = simple_binding_ident(declarator)
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

fn is_memo_call_of_detected_component(expr: &Expr, detected: &BTreeSet<String>) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    if !callee_matches(&call.callee, "memo") || call.args.len() != 1 {
        return false;
    }
    let Expr::Ident(component) = &*call.args[0].expr else {
        return false;
    };
    detected.contains(component.sym.as_ref())
}

fn forward_ref_function_component(expr: &Expr) -> Option<(String, Span)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if !callee_matches(&call.callee, "forwardRef") || call.args.len() != 1 {
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

fn callee_matches(callee: &Callee, expected: &str) -> bool {
    matches!(
        callee,
        Callee::Expr(expr)
            if matches!(&**expr, Expr::Ident(ident) if ident.sym.as_ref() == expected)
                || matches!(
                    &**expr,
                    Expr::Member(member)
                        if matches!(&member.prop, MemberProp::Ident(prop) if prop.sym.as_ref() == expected)
                )
    )
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

fn local_component_id(file: &str, symbol: &str, location: &SourceLocation) -> String {
    format!("local.{file}:{}:{symbol}", location.line)
}

fn normalize_file(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::discover_local_components;
    use crate::swc_parse::{ReactParseOutcome, parse_react_source_file};
    use std::fs;
    use std::path::Path;

    #[test]
    fn extract_local_component_detects_jsx_returning_declarations() {
        let parsed = parse_module(
            "src/components.tsx",
            r#"
            function Button() {
                return <button />;
            }
            const Card = () => <section />;
            let Panel = function () {
                return <aside />;
            };
            function helper() {
                return <span />;
            }
            const NotJsx = () => 42;
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button", "Card", "Panel"]);
        assert_eq!(locals[0].location.file, "src/components.tsx");
        assert_eq!(locals[0].location.line, 2);
    }

    #[test]
    fn extract_local_component_detects_nested_jsx_return() {
        let parsed = parse_module(
            "src/nested-return.tsx",
            r#"
            function Button({ loading }) {
                if (loading) return <Spinner />;
                return null;
            }
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button"]);
    }

    #[test]
    fn extract_local_component_detects_conditional_jsx_expression() {
        let parsed = parse_module(
            "src/conditional.tsx",
            r#"
            const Button = () => condition ? <A /> : null;
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button"]);
    }

    #[test]
    fn extract_local_component_detects_returned_jsx_variable() {
        let parsed = parse_module(
            "src/variable-return.tsx",
            r#"
            function Button() {
                const content = <button />;
                return content;
            }
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button"]);
    }

    #[test]
    fn extract_local_component_detects_logical_jsx_expression() {
        let parsed = parse_module(
            "src/logical.tsx",
            r#"
            const Spinner = () => loading && <Progress />;
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Spinner"]);
    }

    #[test]
    fn extract_local_component_detects_simple_exports() {
        let parsed = parse_module(
            "src/exports.tsx",
            r#"
            export function Button() {
                return <button />;
            }
            export const Card = () => <section />;
            const Panel = () => <aside />;
            export { Panel };
            function Modal() {
                return <dialog />;
            }
            export default Modal;
            export default function Toast() {
                return <output />;
            }
            export default function () {
                return <div />;
            }
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button", "Card", "Modal", "Panel", "Toast"]);
        let panel = locals
            .iter()
            .find(|component| component.symbol == "Panel")
            .expect("Panel should be detected");
        assert_eq!(panel.location.line, 6);
    }

    #[test]
    fn extract_local_component_detects_simple_wrappers() {
        let parsed = parse_module(
            "src/wrappers.tsx",
            r#"
            const ButtonBase = () => <button />;
            const Button = memo(ButtonBase);
            export const Card = memo(ButtonBase);
            export const Field = forwardRef(function Field() {
                return <input />;
            });
            const Missing = memo(Unknown);
            const lowercase = memo(ButtonBase);
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button", "ButtonBase", "Card", "Field"]);
    }

    #[test]
    fn extract_local_component_detects_qualified_wrappers() {
        let parsed = parse_module(
            "src/qualified-wrappers.tsx",
            r#"
            const ButtonBase = () => <button />;
            const Button = React.memo(ButtonBase);
            export const Field = React.forwardRef(function Field() {
                return <input />;
            });
            "#,
        );

        let locals = discover_local_components(&[parsed]);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Button", "ButtonBase", "Field"]);
    }

    #[test]
    fn extract_local_component_emits_stable_ids_and_symbol_order() {
        let first = parse_module(
            "src/aaa.tsx",
            r#"
            const Zeta = () => <div />;
            "#,
        );
        let second = parse_module(
            "src/zzz.tsx",
            r#"
            const Alpha = () => <div />;
            "#,
        );

        let locals = discover_local_components(&[first, second]);
        let ids = local_ids(&locals);
        let symbols = local_symbols(&locals);

        assert_eq!(symbols, vec!["Alpha", "Zeta"]);
        assert_eq!(
            ids,
            vec!["local.src/zzz.tsx:2:Alpha", "local.src/aaa.tsx:2:Zeta"]
        );
    }

    fn parse_module(file: &str, source: &str) -> crate::ParsedReactModule {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
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

    fn local_symbols(locals: &[wax_contract::LocalComponent]) -> Vec<&str> {
        locals
            .iter()
            .map(|component| component.symbol.as_str())
            .collect()
    }

    fn local_ids(locals: &[wax_contract::LocalComponent]) -> Vec<&str> {
        locals
            .iter()
            .map(|component| component.id.as_str())
            .collect()
    }
}
