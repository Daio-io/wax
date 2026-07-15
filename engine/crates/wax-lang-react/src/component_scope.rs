//! Component identity and span index for React fact parent attribution.

use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    Callee, ClassDecl, DefaultDecl, Expr, FnDecl, MemberProp, ModuleDecl, ModuleItem, VarDeclarator,
};
use swc_ecma_visit::{Visit, VisitWith};
use wax_contract::{IdentityStability, ParentScope};

use crate::component_detect::{
    class_returns_jsx, expression_returns_jsx, function_returns_jsx, is_pascal_case,
    simple_binding_ident,
};
use crate::swc_parse::ParsedReactModule;

/// Local React component definition used for parent attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDefinition {
    /// Component symbol (binding or function name).
    pub name: String,
    /// AST span covering the full component initializer/body.
    pub span: Span,
}

/// Collects PascalCase JSX-returning component definitions in a module.
#[must_use]
pub fn collect_component_definitions(parsed: &ParsedReactModule) -> Vec<ComponentDefinition> {
    let mut collector = ComponentDefinitionCollector { out: Vec::new() };
    parsed.module.visit_with(&mut collector);
    collector.out
}

/// Chooses the narrowest component whose span fully contains `target`.
#[must_use]
pub fn parent_for_span(
    parsed: &ParsedReactModule,
    components: &[ComponentDefinition],
    target: Span,
) -> Option<ParentScope> {
    components
        .iter()
        .filter(|component| span_contains(component.span, target))
        .min_by_key(|component| span_byte_len(component.span))
        .and_then(|component| parent_scope_for_component(parsed, &component.name, component.span))
}

/// Builds a [`ParentScope`] for a component definition.
#[must_use]
pub fn parent_scope_for_component(
    parsed: &ParsedReactModule,
    symbol: &str,
    span: Span,
) -> Option<ParentScope> {
    let file = normalize_file(&parsed.file);
    let module_identity = module_identity_for_file(&file);
    let location = parsed.source_location_from_span(span)?;
    Some(ParentScope {
        parent_id: local_definition_id(&module_identity, symbol),
        symbol: symbol.to_owned(),
        qualified_symbol: Some(qualified_component_symbol(&module_identity, symbol)),
        scope_kind: "component".to_owned(),
        identity_basis: "module_path_and_symbol".to_owned(),
        identity_stability: IdentityStability::PathSensitive,
        location: Some(location),
    })
}

/// Unwraps TypeScript / paren expression wrappers.
pub(crate) fn peel_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => peel_expr(&paren.expr),
        Expr::TsAs(ts_as) => peel_expr(&ts_as.expr),
        Expr::TsSatisfies(ts_satisfies) => peel_expr(&ts_satisfies.expr),
        Expr::TsConstAssertion(assertion) => peel_expr(&assertion.expr),
        Expr::TsTypeAssertion(assertion) => peel_expr(&assertion.expr),
        Expr::TsNonNull(non_null) => peel_expr(&non_null.expr),
        Expr::TsInstantiation(instantiation) => peel_expr(&instantiation.expr),
        other => other,
    }
}

struct ComponentDefinitionCollector {
    out: Vec<ComponentDefinition>,
}

impl Visit for ComponentDefinitionCollector {
    fn visit_fn_decl(&mut self, node: &FnDecl) {
        let name = node.ident.sym.to_string();
        if is_pascal_case(&name) && function_returns_jsx(&node.function) {
            self.out.push(ComponentDefinition {
                name,
                span: node.function.span,
            });
        }
        node.visit_children_with(self);
    }

    fn visit_class_decl(&mut self, node: &ClassDecl) {
        let name = node.ident.sym.to_string();
        if is_pascal_case(&name) && class_returns_jsx(&node.class) {
            self.out.push(ComponentDefinition {
                name,
                span: node.class.span,
            });
        }
        node.visit_children_with(self);
    }

    fn visit_var_declarator(&mut self, node: &VarDeclarator) {
        if let Some((name, _)) = simple_binding_ident(node)
            && is_pascal_case(&name)
            && let Some(init) = node.init.as_deref()
            && let Some(span) = component_initializer_span(init)
        {
            self.out.push(ComponentDefinition { name, span });
        }
        node.visit_children_with(self);
    }

    fn visit_module_item(&mut self, node: &ModuleItem) {
        if let ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) = node {
            match &default_decl.decl {
                DefaultDecl::Fn(fn_expr) => {
                    let name = fn_expr
                        .ident
                        .as_ref()
                        .map(|ident| ident.sym.to_string())
                        .unwrap_or_else(|| "default".to_owned());
                    if is_pascal_case(&name) && function_returns_jsx(&fn_expr.function) {
                        self.out.push(ComponentDefinition {
                            name,
                            span: fn_expr.function.span,
                        });
                    }
                }
                DefaultDecl::Class(class_expr) => {
                    let name = class_expr
                        .ident
                        .as_ref()
                        .map(|ident| ident.sym.to_string())
                        .unwrap_or_else(|| "default".to_owned());
                    if is_pascal_case(&name) && class_returns_jsx(&class_expr.class) {
                        self.out.push(ComponentDefinition {
                            name,
                            span: class_expr.class.span,
                        });
                    }
                }
                _ => {}
            }
        }
        node.visit_children_with(self);
    }
}

fn component_initializer_span(expr: &Expr) -> Option<Span> {
    let peeled = peel_expr(expr);
    if expression_returns_jsx(peeled) {
        return Some(peeled.span());
    }
    wrapper_component_span(peeled)
}

fn wrapper_component_span(expr: &Expr) -> Option<Span> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if !is_wrapper_callee(&call.callee) {
        return None;
    }
    let arg = call.args.first()?;
    if component_initializer_span(&arg.expr).is_some() || function_expr_returns_jsx(&arg.expr) {
        return Some(call.span());
    }
    None
}

fn function_expr_returns_jsx(expr: &Expr) -> bool {
    match peel_expr(expr) {
        Expr::Fn(fn_expr) => function_returns_jsx(&fn_expr.function),
        Expr::Arrow(_) => expression_returns_jsx(expr),
        _ => false,
    }
}

fn is_wrapper_callee(callee: &Callee) -> bool {
    matches!(
        callee,
        Callee::Expr(expr)
            if matches!(
                peel_expr(expr),
                Expr::Ident(ident)
                    if matches!(ident.sym.as_ref(), "forwardRef" | "memo")
            ) || matches!(
                peel_expr(expr),
                Expr::Member(member)
                    if matches!(
                        &member.prop,
                        MemberProp::Ident(prop)
                            if matches!(prop.sym.as_ref(), "forwardRef" | "memo")
                    )
            )
    )
}

fn span_contains(outer: Span, inner: Span) -> bool {
    !outer.is_dummy() && !inner.is_dummy() && outer.lo() <= inner.lo() && inner.hi() <= outer.hi()
}

fn span_byte_len(span: Span) -> u32 {
    span.hi().0.saturating_sub(span.lo().0)
}

fn module_identity_for_file(file: &str) -> String {
    std::path::Path::new(file)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/")
}

fn qualified_component_symbol(module_identity: &str, symbol: &str) -> String {
    format!("{module_identity}#{symbol}")
}

fn local_definition_id(module_identity: &str, symbol: &str) -> String {
    format!("react:component:{module_identity}#{symbol}")
}

fn normalize_file(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::collect_component_definitions;
    use crate::swc_parse::{ParsedReactModule, ReactParseOutcome, parse_react_source_file};
    use std::path::Path;

    struct Hold {
        _root: tempfile::TempDir,
        parsed: ParsedReactModule,
    }

    fn parse(source: &str) -> Hold {
        let root = tempfile::tempdir().expect("tempdir");
        let relative = Path::new("src/Comp.tsx");
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        let parsed = match parse_react_source_file(root.path(), relative).unwrap() {
            ReactParseOutcome::Parsed(parsed) => parsed,
            ReactParseOutcome::Failed(diag) => panic!("{diag:?}"),
        };
        Hold {
            _root: root,
            parsed,
        }
    }

    #[test]
    fn forward_ref_binding_is_collected() {
        let hold = parse(
            r##"
            import { forwardRef } from "react";
            export const Card = forwardRef(function Card(_props, _ref) {
                return <div />;
            });
            "##,
        );
        let components = collect_component_definitions(&hold.parsed);
        assert!(
            components.iter().any(|component| component.name == "Card"),
            "{components:?}"
        );
    }
}
