//! Shared React component-detection helpers.

use std::collections::{HashMap, HashSet};

use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    ArrowExpr, AssignExpr, BinaryOp, BindingIdent, BlockStmt, BlockStmtOrExpr, CallExpr, Class,
    ClassMember, Decl, Expr, ForHead, Function, Id, ModuleDecl, ModuleExportName, ModuleItem, Pat,
    ReturnStmt, Stmt, UpdateExpr, VarDeclKind, VarDeclarator,
};
use swc_ecma_visit::{Visit, VisitWith};

/// Returns whether a function body contains a JSX-returning path.
pub(crate) fn function_returns_jsx(function: &Function) -> bool {
    function
        .body
        .as_ref()
        .is_some_and(|block| block_returns_element(block, &ignore_calls))
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
    expression_returns_element(expr, &ignore_calls)
}

fn ignore_calls(_: &CallExpr, _: &HashSet<Id>) -> bool {
    false
}

pub(crate) fn expression_returns_element(
    expr: &Expr,
    render_call: &impl Fn(&CallExpr, &HashSet<Id>) -> bool,
) -> bool {
    match expr {
        Expr::Arrow(arrow) => match &*arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => block_returns_element(block, render_call),
            BlockStmtOrExpr::Expr(expr) => {
                is_element_expression(expr, &HashSet::new(), render_call)
            }
        },
        Expr::Fn(function) => function
            .function
            .body
            .as_ref()
            .is_some_and(|block| block_returns_element(block, render_call)),
        Expr::Paren(paren) => expression_returns_element(&paren.expr, render_call),
        _ => false,
    }
}

pub(crate) fn block_returns_element(
    block: &BlockStmt,
    render_call: &impl Fn(&CallExpr, &HashSet<Id>) -> bool,
) -> bool {
    let mut jsx_bindings = HashSet::new();
    block
        .stmts
        .iter()
        .any(|stmt| stmt_returns_element(stmt, &mut jsx_bindings, render_call))
}

fn stmt_returns_element(
    stmt: &Stmt,
    jsx_bindings: &mut HashSet<Id>,
    render_call: &impl Fn(&CallExpr, &HashSet<Id>) -> bool,
) -> bool {
    match stmt {
        Stmt::Decl(swc_ecma_ast::Decl::Var(var_decl)) => {
            for declarator in &var_decl.decls {
                if let Pat::Ident(binding) = &declarator.name
                    && declarator
                        .init
                        .as_deref()
                        .is_some_and(|expr| is_element_expression(expr, jsx_bindings, render_call))
                {
                    jsx_bindings.insert(binding.id.to_id());
                }
            }
            false
        }
        Stmt::Return(return_stmt) => return_stmt
            .arg
            .as_deref()
            .is_some_and(|expr| is_element_expression(expr, jsx_bindings, render_call)),
        Stmt::Block(block) => {
            let mut nested_bindings = jsx_bindings.clone();
            block
                .stmts
                .iter()
                .any(|stmt| stmt_returns_element(stmt, &mut nested_bindings, render_call))
        }
        Stmt::If(if_stmt) => {
            let mut consequent_bindings = jsx_bindings.clone();
            let consequent_returns =
                stmt_returns_element(&if_stmt.cons, &mut consequent_bindings, render_call);
            let alternate_returns = if_stmt.alt.as_deref().is_some_and(|stmt| {
                let mut alternate_bindings = jsx_bindings.clone();
                stmt_returns_element(stmt, &mut alternate_bindings, render_call)
            });
            consequent_returns || alternate_returns
        }
        _ => false,
    }
}

pub(crate) fn is_jsx_expression(expr: &Expr, jsx_bindings: &HashSet<Id>) -> bool {
    is_element_expression(expr, jsx_bindings, &ignore_calls)
}

fn is_element_expression(
    expr: &Expr,
    jsx_bindings: &HashSet<Id>,
    render_call: &impl Fn(&CallExpr, &HashSet<Id>) -> bool,
) -> bool {
    match expr {
        Expr::JSXElement(_) | Expr::JSXFragment(_) => true,
        Expr::Ident(ident) => jsx_bindings.contains(&ident.to_id()),
        Expr::Paren(paren) => is_element_expression(&paren.expr, jsx_bindings, render_call),
        Expr::Cond(cond) => {
            is_element_expression(&cond.cons, jsx_bindings, render_call)
                || is_element_expression(&cond.alt, jsx_bindings, render_call)
        }
        Expr::Bin(binary)
            if matches!(
                binary.op,
                BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
            ) =>
        {
            is_element_expression(&binary.left, jsx_bindings, render_call)
                || is_element_expression(&binary.right, jsx_bindings, render_call)
        }
        Expr::Call(call) => render_call(call, jsx_bindings),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LocalValue<'a> {
    Function(&'a Function),
    Class(&'a Class),
    Constant(&'a Expr),
}

#[derive(Debug, Default)]
pub(crate) struct LocalValues<'a> {
    values: HashMap<Id, LocalValue<'a>>,
    writes: HashSet<Id>,
}

impl<'a> LocalValues<'a> {
    pub(crate) fn collect(items: &'a [ModuleItem]) -> Self {
        let mut locals = Self::default();
        for item in items {
            match item {
                ModuleItem::Stmt(Stmt::Decl(decl)) => locals.collect_decl(decl),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => {
                    locals.collect_decl(&export.decl);
                }
                _ => {}
            }
            item.visit_with(&mut locals);
        }
        locals
    }

    pub(crate) fn get(&self, id: &Id) -> Option<LocalValue<'a>> {
        if self.writes.contains(id) {
            return None;
        }
        self.values.get(id).copied()
    }

    pub(crate) fn factory_binding(&self, block: &'a BlockStmt, id: &Id) -> Option<LocalValue<'a>> {
        if self.writes.contains(id) {
            return None;
        }
        block
            .stmts
            .iter()
            .filter_map(|stmt| stmt.as_decl())
            .flat_map(declaration_values)
            .find_map(|(binding, value)| (binding == *id).then_some(value))
            .or_else(|| self.get(id))
    }

    fn collect_decl(&mut self, decl: &'a Decl) {
        for (id, value) in declaration_values(decl) {
            self.values.insert(id, value);
        }
    }
}

fn declaration_values(decl: &Decl) -> impl Iterator<Item = (Id, LocalValue<'_>)> {
    let single = match decl {
        Decl::Fn(function) if !function.declare => Some((
            function.ident.to_id(),
            LocalValue::Function(&function.function),
        )),
        Decl::Class(class) if !class.declare => {
            Some((class.ident.to_id(), LocalValue::Class(&class.class)))
        }
        _ => None,
    };
    let constants = match decl {
        Decl::Var(var) if var.kind == VarDeclKind::Const && !var.declare => var.decls.as_slice(),
        _ => &[],
    };
    single
        .into_iter()
        .chain(constants.iter().filter_map(|declarator| {
            let Pat::Ident(binding) = &declarator.name else {
                return None;
            };
            Some((
                binding.id.to_id(),
                LocalValue::Constant(declarator.init.as_deref()?),
            ))
        }))
}

impl Visit for LocalValues<'_> {
    fn visit_assign_expr(&mut self, assign: &AssignExpr) {
        assign.left.visit_with(&mut BindingWrites(&mut self.writes));
        assign.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, update: &UpdateExpr) {
        if let Expr::Ident(ident) = update.arg.unwrap_parens() {
            self.writes.insert(ident.to_id());
        }
        update.visit_children_with(self);
    }

    fn visit_for_head(&mut self, head: &ForHead) {
        if let ForHead::Pat(pattern) = head {
            pattern.visit_with(&mut BindingWrites(&mut self.writes));
        }
        head.visit_children_with(self);
    }
}

struct BindingWrites<'a>(&'a mut HashSet<Id>);

impl Visit for BindingWrites<'_> {
    fn visit_binding_ident(&mut self, binding: &BindingIdent) {
        self.0.insert(binding.id.to_id());
    }

    fn visit_expr(&mut self, _: &Expr) {}
}

impl<'a> LocalValue<'a> {
    pub(crate) fn factory_return(self) -> Option<(&'a BlockStmt, &'a Expr)> {
        let block = match self {
            Self::Function(function) if !function.is_async && !function.is_generator => {
                function.body.as_ref()?
            }
            Self::Constant(expr) => match expr.unwrap_parens() {
                Expr::Fn(function)
                    if !function.function.is_async && !function.function.is_generator =>
                {
                    function.function.body.as_ref()?
                }
                Expr::Arrow(arrow) if !arrow.is_async => arrow.body.as_block_stmt()?,
                _ => return None,
            },
            _ => return None,
        };
        if block.stmts.iter().map(count_returns).sum::<usize>() != 1 {
            return None;
        }
        block.stmts.iter().find_map(|stmt| match stmt {
            Stmt::Return(return_stmt) => Some((block, return_stmt.arg.as_deref()?)),
            _ => None,
        })
    }

    pub(crate) fn initialized_before(
        self,
        block: &BlockStmt,
        returned: &Expr,
        call: &CallExpr,
    ) -> bool {
        let span = match self {
            Self::Function(_) => return true,
            Self::Class(class) => class.span,
            Self::Constant(expr) => expr.span(),
        };
        let use_span = if block.span.contains(span) {
            returned.span()
        } else {
            call.span
        };
        span.hi <= use_span.lo
    }
}

fn count_returns(stmt: &Stmt) -> usize {
    #[derive(Default)]
    struct Returns(usize);

    impl Visit for Returns {
        fn visit_return_stmt(&mut self, _: &ReturnStmt) {
            self.0 += 1;
        }

        fn visit_function(&mut self, _: &Function) {}
        fn visit_arrow_expr(&mut self, _: &ArrowExpr) {}
        fn visit_class(&mut self, _: &Class) {}
    }

    let mut returns = Returns::default();
    stmt.visit_with(&mut returns);
    returns.0
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
