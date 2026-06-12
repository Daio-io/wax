//! Shared React component-detection helpers.

use std::collections::BTreeSet;

use swc_common::Span;
use swc_ecma_ast::{
    BinaryOp, BlockStmt, BlockStmtOrExpr, Class, ClassMember, Expr, Function, ModuleExportName,
    Pat, Stmt, VarDeclarator,
};

/// Returns whether a function body contains a JSX-returning path.
pub(crate) fn function_returns_jsx(function: &Function) -> bool {
    function.body.as_ref().is_some_and(block_returns_jsx)
}

/// Returns whether a class has a `render` method that returns JSX.
pub(crate) fn class_returns_jsx(class: &Class) -> bool {
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

/// Returns whether a function-like expression returns JSX.
pub(crate) fn expression_returns_jsx(expr: &Expr) -> bool {
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
        Stmt::Decl(swc_ecma_ast::Decl::Var(var_decl)) => {
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

/// Returns a simple identifier binding from a variable declarator.
pub(crate) fn simple_binding_ident(declarator: &VarDeclarator) -> Option<(String, Span)> {
    match &declarator.name {
        Pat::Ident(binding) => Some((binding.id.sym.to_string(), binding.id.span)),
        _ => None,
    }
}

/// Returns whether a symbol starts with an ASCII uppercase letter.
pub(crate) fn is_pascal_case(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

/// Converts an SWC module export name into its text form.
pub(crate) fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{class_returns_jsx, expression_returns_jsx, function_returns_jsx, is_pascal_case};
    use crate::swc_parse::{ReactParseOutcome, parse_react_source_file};
    use swc_ecma_ast::{Decl, ModuleItem};

    #[test]
    fn detects_jsx_returning_function_arrow_and_class_render() {
        let parsed = parse_module(
            r#"
            function Button() {
                return <button />;
            }
            const Card = () => <section />;
            class Dialog {
                render() {
                    return <div role="dialog" />;
                }
            }
            "#,
        );

        let mut saw_function = false;
        let mut saw_arrow = false;
        let mut saw_class = false;
        for item in &parsed.module.body {
            match item {
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Fn(fn_decl)))
                    if fn_decl.ident.sym.as_ref() == "Button" =>
                {
                    saw_function = function_returns_jsx(&fn_decl.function);
                }
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Var(var_decl))) => {
                    for declarator in &var_decl.decls {
                        if declarator
                            .init
                            .as_deref()
                            .is_some_and(expression_returns_jsx)
                        {
                            saw_arrow = true;
                        }
                    }
                }
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Class(class_decl)))
                    if class_decl.ident.sym.as_ref() == "Dialog" =>
                {
                    saw_class = class_returns_jsx(&class_decl.class);
                }
                _ => {}
            }
        }

        assert!(saw_function);
        assert!(saw_arrow);
        assert!(saw_class);
    }

    #[test]
    fn detects_conditional_early_and_variable_jsx_returns() {
        let parsed = parse_module(
            r#"
            function Early({ loading }) {
                if (loading) return <Spinner />;
                return null;
            }
            const Conditional = () => ready ? <Ready /> : null;
            function Variable() {
                const content = <span />;
                return content;
            }
            "#,
        );

        let detected = parsed
            .module
            .body
            .iter()
            .filter(|item| match item {
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Fn(fn_decl))) => {
                    function_returns_jsx(&fn_decl.function)
                }
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Var(var_decl))) => {
                    var_decl.decls.iter().any(|declarator| {
                        declarator
                            .init
                            .as_deref()
                            .is_some_and(expression_returns_jsx)
                    })
                }
                _ => false,
            })
            .count();

        assert_eq!(detected, 3);
    }

    #[test]
    fn excludes_non_jsx_functions() {
        let parsed = parse_module(
            r#"
            function makeNumber() {
                return 42;
            }
            const label = () => "not jsx";
            "#,
        );

        for item in &parsed.module.body {
            match item {
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Fn(fn_decl))) => {
                    assert!(!function_returns_jsx(&fn_decl.function));
                }
                ModuleItem::Stmt(swc_ecma_ast::Stmt::Decl(Decl::Var(var_decl))) => {
                    assert!(var_decl.decls.iter().all(|declarator| {
                        !declarator
                            .init
                            .as_deref()
                            .is_some_and(expression_returns_jsx)
                    }));
                }
                _ => {}
            }
        }
    }

    #[test]
    fn identifies_pascal_case_symbols() {
        assert!(is_pascal_case("Button"));
        assert!(is_pascal_case("A"));
        assert!(!is_pascal_case("button"));
        assert!(!is_pascal_case("_Button"));
        assert!(!is_pascal_case(""));
    }

    fn parse_module(source: &str) -> crate::ParsedReactModule {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("src/input.tsx");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();

        match parse_react_source_file(tempdir.path(), std::path::Path::new("src/input.tsx"))
            .expect("parse should not fail fatally")
        {
            ReactParseOutcome::Parsed(parsed) => parsed,
            ReactParseOutcome::Failed(diagnostic) => {
                panic!("expected parse success, got {}", diagnostic.message)
            }
        }
    }
}
