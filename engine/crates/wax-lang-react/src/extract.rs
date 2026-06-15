//! React component extraction from parsed SWC modules.

use std::collections::{BTreeMap, BTreeSet};

use swc_common::{Span, Spanned};
use swc_ecma_ast::{
    AssignTarget, BlockStmt, BlockStmtOrExpr, Callee, Class, ClassMember, Decl, DefaultDecl,
    ExportSpecifier, Expr, ForHead, Function, JSXAttrOrSpread, JSXAttrValue, JSXElement,
    JSXElementChild, JSXElementName, JSXExpr, JSXFragment, JSXMemberExpr, JSXObject, Key,
    MemberProp, ModuleDecl, ModuleItem, Pat, Prop, PropName, PropOrSpread, SimpleAssignTarget,
    Stmt, VarDecl, VarDeclOrExpr, VarDeclarator,
};
use wax_contract::{
    Diagnostic, DiagnosticSeverity, LocalComponent, MatchStatus, SourceLocation, UsageSite,
};
use wax_lang_api::{
    import_matches_framework_package, npm_import_package_root, resolve_import_aware_match,
};

use crate::component_detect::{
    expression_returns_jsx, function_returns_jsx, is_pascal_case, module_export_name,
    simple_binding_ident,
};
use crate::config::ReactScanConfig;
use crate::diagnostics::DS_USAGE_UNRESOLVED;
use crate::module_graph::ReactModuleGraph;
use crate::registry::ReactRegistryIndex;
use crate::swc_parse::ParsedReactModule;

/// JSX usage extraction output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactUsageExtraction {
    /// Resolved registry-backed JSX usage sites.
    pub usage_sites: Vec<UsageSite>,
    /// Recoverable JSX usage diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

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

/// Collects registry-backed JSX usage sites from parsed modules.
#[must_use]
pub fn collect_usage_sites(
    parsed_modules: &[ParsedReactModule],
    module_graph: &ReactModuleGraph,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
) -> ReactUsageExtraction {
    let mut extraction = ReactUsageExtraction::default();

    for parsed in parsed_modules {
        let local_bindings = local_declared_bindings(parsed);
        let mut candidates = Vec::new();
        for item in &parsed.module.body {
            collect_jsx_usage_candidates_from_module_item(parsed, item, &mut candidates);
        }

        for candidate in candidates {
            if let Some((registry_symbol, match_status)) =
                classify_jsx_usage(parsed, module_graph, config, registry, &candidate)
            {
                if let Some(location) = parsed.source_location_from_span(candidate.span) {
                    extraction.usage_sites.push(UsageSite {
                        id: usage_site_id(&location, &candidate.symbol),
                        location,
                        symbol: candidate.symbol,
                        match_status,
                        registry_symbol: Some(registry_symbol),
                    });
                }
            } else if unresolved_usage_is_design_system_relevant(
                parsed,
                module_graph,
                config,
                registry,
                &candidate,
                &local_bindings,
            ) {
                extraction.diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: DS_USAGE_UNRESOLVED.to_owned(),
                    message: format!(
                        "design-system-relevant JSX usage '{}' could not be resolved",
                        candidate.symbol
                    ),
                    location: parsed.source_location_from_span(candidate.span),
                });
            }
        }
    }

    extraction
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JsxUsageCandidate {
    symbol: String,
    binding_name: String,
    span: Span,
    shadowed: bool,
}

type UsageScopes = Vec<BTreeSet<String>>;

fn collect_jsx_usage_candidates_from_module_item(
    parsed: &ParsedReactModule,
    item: &ModuleItem,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    let mut scopes = Vec::new();
    match item {
        ModuleItem::Stmt(stmt) => {
            collect_jsx_usage_candidates_from_stmt(parsed, stmt, &mut scopes, candidates);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
            collect_jsx_usage_candidates_from_decl(
                parsed,
                &export_decl.decl,
                &mut scopes,
                candidates,
            );
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
            if let DefaultDecl::Fn(fn_expr) = &default_decl.decl {
                collect_jsx_usage_candidates_from_function(
                    parsed,
                    &fn_expr.function,
                    &mut scopes,
                    candidates,
                );
            } else if let DefaultDecl::Class(class_expr) = &default_decl.decl {
                collect_jsx_usage_candidates_from_class(
                    parsed,
                    &class_expr.class,
                    &mut scopes,
                    candidates,
                );
            }
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr)) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &default_expr.expr,
                &mut scopes,
                candidates,
            );
        }
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_decl(
    parsed: &ParsedReactModule,
    decl: &Decl,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match decl {
        Decl::Fn(fn_decl) => {
            collect_jsx_usage_candidates_from_function(
                parsed,
                &fn_decl.function,
                scopes,
                candidates,
            );
        }
        Decl::Var(var_decl) => {
            collect_jsx_usage_candidates_from_var_decl(parsed, var_decl, scopes, candidates);
        }
        Decl::Class(class_decl) => {
            collect_jsx_usage_candidates_from_class(parsed, &class_decl.class, scopes, candidates);
        }
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_function(
    parsed: &ParsedReactModule,
    function: &Function,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    let mut function_bindings = BTreeSet::new();
    for param in &function.params {
        collect_pat_bindings(&param.pat, &mut function_bindings);
    }
    scopes.push(function_bindings);
    if let Some(body) = &function.body {
        collect_jsx_usage_candidates_from_block(parsed, body, scopes, candidates);
    }
    scopes.pop();
}

fn collect_jsx_usage_candidates_from_block(
    parsed: &ParsedReactModule,
    block: &BlockStmt,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    scopes.push(block_declared_bindings(block));
    for stmt in &block.stmts {
        collect_jsx_usage_candidates_from_stmt(parsed, stmt, scopes, candidates);
    }
    scopes.pop();
}

fn collect_jsx_usage_candidates_from_stmt(
    parsed: &ParsedReactModule,
    stmt: &Stmt,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match stmt {
        Stmt::Decl(decl) => {
            collect_jsx_usage_candidates_from_decl(parsed, decl, scopes, candidates)
        }
        Stmt::Return(return_stmt) => {
            if let Some(arg) = return_stmt.arg.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, arg, scopes, candidates);
            }
        }
        Stmt::Expr(expr_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &expr_stmt.expr, scopes, candidates);
        }
        Stmt::Block(block) => {
            collect_jsx_usage_candidates_from_block(parsed, block, scopes, candidates);
        }
        Stmt::If(if_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &if_stmt.test, scopes, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &if_stmt.cons, scopes, candidates);
            if let Some(alt) = if_stmt.alt.as_deref() {
                collect_jsx_usage_candidates_from_stmt(parsed, alt, scopes, candidates);
            }
        }
        Stmt::With(with_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &with_stmt.obj, scopes, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &with_stmt.body, scopes, candidates);
        }
        Stmt::Labeled(labeled) => {
            collect_jsx_usage_candidates_from_stmt(parsed, &labeled.body, scopes, candidates);
        }
        Stmt::Throw(throw_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &throw_stmt.arg, scopes, candidates);
        }
        Stmt::Switch(switch_stmt) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &switch_stmt.discriminant,
                scopes,
                candidates,
            );
            for case in &switch_stmt.cases {
                if let Some(test) = case.test.as_deref() {
                    collect_jsx_usage_candidates_from_expr(parsed, test, scopes, candidates);
                }
                let mut case_bindings = BTreeSet::new();
                for stmt in &case.cons {
                    collect_stmt_declared_bindings(stmt, &mut case_bindings);
                }
                scopes.push(case_bindings);
                for stmt in &case.cons {
                    collect_jsx_usage_candidates_from_stmt(parsed, stmt, scopes, candidates);
                }
                scopes.pop();
            }
        }
        Stmt::Try(try_stmt) => {
            collect_jsx_usage_candidates_from_block(parsed, &try_stmt.block, scopes, candidates);
            if let Some(handler) = &try_stmt.handler {
                let mut catch_bindings = BTreeSet::new();
                if let Some(param) = &handler.param {
                    collect_pat_bindings(param, &mut catch_bindings);
                }
                scopes.push(catch_bindings);
                collect_jsx_usage_candidates_from_block(parsed, &handler.body, scopes, candidates);
                scopes.pop();
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                collect_jsx_usage_candidates_from_block(parsed, finalizer, scopes, candidates);
            }
        }
        Stmt::While(while_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &while_stmt.test, scopes, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &while_stmt.body, scopes, candidates);
        }
        Stmt::DoWhile(do_while) => {
            collect_jsx_usage_candidates_from_stmt(parsed, &do_while.body, scopes, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &do_while.test, scopes, candidates);
        }
        Stmt::For(for_stmt) => {
            scopes.push(for_declared_bindings(for_stmt));
            if let Some(init) = &for_stmt.init {
                collect_jsx_usage_candidates_from_for_init(parsed, init, scopes, candidates);
            }
            if let Some(test) = for_stmt.test.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, test, scopes, candidates);
            }
            if let Some(update) = for_stmt.update.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, update, scopes, candidates);
            }
            collect_jsx_usage_candidates_from_stmt(parsed, &for_stmt.body, scopes, candidates);
            scopes.pop();
        }
        Stmt::ForIn(for_in) => {
            scopes.push(for_head_declared_bindings(&for_in.left));
            collect_jsx_usage_candidates_from_expr(parsed, &for_in.right, scopes, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &for_in.body, scopes, candidates);
            scopes.pop();
        }
        Stmt::ForOf(for_of) => {
            scopes.push(for_head_declared_bindings(&for_of.left));
            collect_jsx_usage_candidates_from_expr(parsed, &for_of.right, scopes, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &for_of.body, scopes, candidates);
            scopes.pop();
        }
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_expr(
    parsed: &ParsedReactModule,
    expr: &Expr,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match expr {
        Expr::JSXElement(element) => {
            collect_jsx_usage_candidates_from_jsx_element(parsed, element, scopes, candidates);
        }
        Expr::JSXFragment(fragment) => {
            collect_jsx_usage_candidates_from_jsx_fragment(parsed, fragment, scopes, candidates);
        }
        Expr::Arrow(arrow) => match &*arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => {
                let mut arrow_bindings = BTreeSet::new();
                for param in &arrow.params {
                    collect_pat_bindings(param, &mut arrow_bindings);
                }
                scopes.push(arrow_bindings);
                collect_jsx_usage_candidates_from_block(parsed, block, scopes, candidates);
                scopes.pop();
            }
            BlockStmtOrExpr::Expr(expr) => {
                let mut arrow_bindings = BTreeSet::new();
                for param in &arrow.params {
                    collect_pat_bindings(param, &mut arrow_bindings);
                }
                scopes.push(arrow_bindings);
                collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
                scopes.pop();
            }
        },
        Expr::Fn(fn_expr) => {
            let mut fn_bindings = BTreeSet::new();
            if let Some(ident) = &fn_expr.ident {
                fn_bindings.insert(ident.sym.to_string());
            }
            scopes.push(fn_bindings);
            collect_jsx_usage_candidates_from_function(
                parsed,
                &fn_expr.function,
                scopes,
                candidates,
            );
            scopes.pop();
        }
        Expr::Class(class_expr) => {
            let mut class_bindings = BTreeSet::new();
            if let Some(ident) = &class_expr.ident {
                class_bindings.insert(ident.sym.to_string());
            }
            scopes.push(class_bindings);
            collect_jsx_usage_candidates_from_class(parsed, &class_expr.class, scopes, candidates);
            scopes.pop();
        }
        Expr::Paren(paren) => {
            collect_jsx_usage_candidates_from_expr(parsed, &paren.expr, scopes, candidates);
        }
        Expr::Cond(cond) => {
            collect_jsx_usage_candidates_from_expr(parsed, &cond.test, scopes, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &cond.cons, scopes, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &cond.alt, scopes, candidates);
        }
        Expr::Bin(binary) => {
            collect_jsx_usage_candidates_from_expr(parsed, &binary.left, scopes, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &binary.right, scopes, candidates);
        }
        Expr::Call(call) => {
            if let Callee::Expr(callee) = &call.callee {
                collect_jsx_usage_candidates_from_expr(parsed, callee, scopes, candidates);
            }
            for arg in &call.args {
                collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, scopes, candidates);
            }
        }
        Expr::Array(array) => {
            for elem in array.elems.iter().flatten() {
                collect_jsx_usage_candidates_from_expr(parsed, &elem.expr, scopes, candidates);
            }
        }
        Expr::Object(object) => {
            for prop in &object.props {
                collect_jsx_usage_candidates_from_object_prop(parsed, prop, scopes, candidates);
            }
        }
        Expr::Assign(assign) => {
            collect_jsx_usage_candidates_from_assign_target(
                parsed,
                &assign.left,
                scopes,
                candidates,
            );
            collect_jsx_usage_candidates_from_expr(parsed, &assign.right, scopes, candidates);
        }
        Expr::Seq(seq) => {
            for expr in &seq.exprs {
                collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
            }
        }
        Expr::Await(await_expr) => {
            collect_jsx_usage_candidates_from_expr(parsed, &await_expr.arg, scopes, candidates);
        }
        Expr::Yield(yield_expr) => {
            if let Some(arg) = yield_expr.arg.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, arg, scopes, candidates);
            }
        }
        Expr::Tpl(tpl) => {
            for expr in &tpl.exprs {
                collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
            }
        }
        Expr::TaggedTpl(tagged) => {
            collect_jsx_usage_candidates_from_expr(parsed, &tagged.tag, scopes, candidates);
            for expr in &tagged.tpl.exprs {
                collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
            }
        }
        Expr::Unary(unary) => {
            collect_jsx_usage_candidates_from_expr(parsed, &unary.arg, scopes, candidates);
        }
        Expr::Update(update) => {
            collect_jsx_usage_candidates_from_expr(parsed, &update.arg, scopes, candidates);
        }
        Expr::Member(member) => {
            collect_jsx_usage_candidates_from_expr(parsed, &member.obj, scopes, candidates);
            if let MemberProp::Computed(computed) = &member.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, scopes, candidates);
            }
        }
        Expr::SuperProp(super_prop) => {
            if let swc_ecma_ast::SuperProp::Computed(computed) = &super_prop.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, scopes, candidates);
            }
        }
        Expr::New(new_expr) => {
            collect_jsx_usage_candidates_from_expr(parsed, &new_expr.callee, scopes, candidates);
            if let Some(args) = &new_expr.args {
                for arg in args {
                    collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, scopes, candidates);
                }
            }
        }
        Expr::TsAs(ts_as) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_as.expr, scopes, candidates);
        }
        Expr::TsSatisfies(ts_satisfies) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_satisfies.expr, scopes, candidates);
        }
        Expr::TsNonNull(ts_non_null) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_non_null.expr, scopes, candidates);
        }
        Expr::TsTypeAssertion(ts_assertion) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_assertion.expr, scopes, candidates);
        }
        Expr::TsConstAssertion(ts_const) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_const.expr, scopes, candidates);
        }
        Expr::TsInstantiation(ts_instantiation) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &ts_instantiation.expr,
                scopes,
                candidates,
            );
        }
        Expr::OptChain(opt_chain) => match &*opt_chain.base {
            swc_ecma_ast::OptChainBase::Member(member) => {
                collect_jsx_usage_candidates_from_expr(parsed, &member.obj, scopes, candidates);
                if let MemberProp::Computed(computed) = &member.prop {
                    collect_jsx_usage_candidates_from_expr(
                        parsed,
                        &computed.expr,
                        scopes,
                        candidates,
                    );
                }
            }
            swc_ecma_ast::OptChainBase::Call(call) => {
                collect_jsx_usage_candidates_from_expr(parsed, &call.callee, scopes, candidates);
                for arg in &call.args {
                    collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, scopes, candidates);
                }
            }
        },
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_jsx_element(
    parsed: &ParsedReactModule,
    element: &JSXElement,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let Some((symbol, binding_name)) = jsx_element_name(&element.opening.name)
        && is_pascal_case(&binding_name)
    {
        let shadowed = binding_is_shadowed(&binding_name, scopes);
        candidates.push(JsxUsageCandidate {
            symbol,
            binding_name,
            span: element.opening.span,
            shadowed,
        });
    }

    for attr in &element.opening.attrs {
        collect_jsx_usage_candidates_from_jsx_attr(parsed, attr, scopes, candidates);
    }
    for child in &element.children {
        collect_jsx_usage_candidates_from_jsx_child(parsed, child, scopes, candidates);
    }
}

fn collect_jsx_usage_candidates_from_jsx_fragment(
    parsed: &ParsedReactModule,
    fragment: &JSXFragment,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    for child in &fragment.children {
        collect_jsx_usage_candidates_from_jsx_child(parsed, child, scopes, candidates);
    }
}

fn collect_jsx_usage_candidates_from_jsx_child(
    parsed: &ParsedReactModule,
    child: &JSXElementChild,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match child {
        JSXElementChild::JSXElement(element) => {
            collect_jsx_usage_candidates_from_jsx_element(parsed, element, scopes, candidates);
        }
        JSXElementChild::JSXFragment(fragment) => {
            collect_jsx_usage_candidates_from_jsx_fragment(parsed, fragment, scopes, candidates);
        }
        JSXElementChild::JSXExprContainer(container) => {
            if let JSXExpr::Expr(expr) = &container.expr {
                collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
            }
        }
        JSXElementChild::JSXSpreadChild(spread) => {
            collect_jsx_usage_candidates_from_expr(parsed, &spread.expr, scopes, candidates);
        }
        JSXElementChild::JSXText(_) => {}
    }
}

fn collect_jsx_usage_candidates_from_jsx_attr(
    parsed: &ParsedReactModule,
    attr: &JSXAttrOrSpread,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match attr {
        JSXAttrOrSpread::JSXAttr(attr) => match &attr.value {
            Some(JSXAttrValue::JSXExprContainer(container)) => {
                if let JSXExpr::Expr(expr) = &container.expr {
                    collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
                }
            }
            Some(JSXAttrValue::JSXElement(element)) => {
                collect_jsx_usage_candidates_from_jsx_element(parsed, element, scopes, candidates);
            }
            Some(JSXAttrValue::JSXFragment(fragment)) => {
                collect_jsx_usage_candidates_from_jsx_fragment(
                    parsed, fragment, scopes, candidates,
                );
            }
            Some(JSXAttrValue::Str(_)) | None => {}
        },
        JSXAttrOrSpread::SpreadElement(spread) => {
            collect_jsx_usage_candidates_from_expr(parsed, &spread.expr, scopes, candidates);
        }
    }
}

fn collect_jsx_usage_candidates_from_var_decl(
    parsed: &ParsedReactModule,
    var_decl: &VarDecl,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    for declarator in &var_decl.decls {
        if let Some(init) = declarator.init.as_deref() {
            collect_jsx_usage_candidates_from_expr(parsed, init, scopes, candidates);
        }
    }
}

fn collect_jsx_usage_candidates_from_for_init(
    parsed: &ParsedReactModule,
    init: &VarDeclOrExpr,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match init {
        VarDeclOrExpr::VarDecl(var_decl) => {
            collect_jsx_usage_candidates_from_var_decl(parsed, var_decl, scopes, candidates);
        }
        VarDeclOrExpr::Expr(expr) => {
            collect_jsx_usage_candidates_from_expr(parsed, expr, scopes, candidates);
        }
    }
}

fn collect_jsx_usage_candidates_from_object_prop(
    parsed: &ParsedReactModule,
    prop: &PropOrSpread,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match prop {
        PropOrSpread::Spread(spread) => {
            collect_jsx_usage_candidates_from_expr(parsed, &spread.expr, scopes, candidates);
        }
        PropOrSpread::Prop(prop) => match &**prop {
            Prop::KeyValue(key_value) => {
                collect_jsx_usage_candidates_from_prop_name(
                    parsed,
                    &key_value.key,
                    scopes,
                    candidates,
                );
                collect_jsx_usage_candidates_from_expr(
                    parsed,
                    &key_value.value,
                    scopes,
                    candidates,
                );
            }
            Prop::Assign(assign) => {
                collect_jsx_usage_candidates_from_expr(parsed, &assign.value, scopes, candidates);
            }
            Prop::Getter(getter) => {
                collect_jsx_usage_candidates_from_prop_name(
                    parsed,
                    &getter.key,
                    scopes,
                    candidates,
                );
                if let Some(body) = &getter.body {
                    collect_jsx_usage_candidates_from_block(parsed, body, scopes, candidates);
                }
            }
            Prop::Setter(setter) => {
                collect_jsx_usage_candidates_from_prop_name(
                    parsed,
                    &setter.key,
                    scopes,
                    candidates,
                );
                let mut setter_bindings = BTreeSet::new();
                collect_pat_bindings(&setter.param, &mut setter_bindings);
                scopes.push(setter_bindings);
                if let Some(body) = &setter.body {
                    collect_jsx_usage_candidates_from_block(parsed, body, scopes, candidates);
                }
                scopes.pop();
            }
            Prop::Method(method) => {
                collect_jsx_usage_candidates_from_prop_name(
                    parsed,
                    &method.key,
                    scopes,
                    candidates,
                );
                collect_jsx_usage_candidates_from_function(
                    parsed,
                    &method.function,
                    scopes,
                    candidates,
                );
            }
            Prop::Shorthand(_) => {}
        },
    }
}

fn collect_jsx_usage_candidates_from_prop_name(
    parsed: &ParsedReactModule,
    prop_name: &PropName,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let PropName::Computed(computed) = prop_name {
        collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, scopes, candidates);
    }
}

fn collect_jsx_usage_candidates_from_assign_target(
    parsed: &ParsedReactModule,
    target: &AssignTarget,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let AssignTarget::Simple(simple) = target {
        collect_jsx_usage_candidates_from_simple_assign_target(parsed, simple, scopes, candidates);
    }
}

fn collect_jsx_usage_candidates_from_simple_assign_target(
    parsed: &ParsedReactModule,
    target: &SimpleAssignTarget,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match target {
        SimpleAssignTarget::Member(member) => {
            collect_jsx_usage_candidates_from_expr(parsed, &member.obj, scopes, candidates);
            if let MemberProp::Computed(computed) = &member.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, scopes, candidates);
            }
        }
        SimpleAssignTarget::SuperProp(super_prop) => {
            if let swc_ecma_ast::SuperProp::Computed(computed) = &super_prop.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, scopes, candidates);
            }
        }
        SimpleAssignTarget::Paren(paren) => {
            collect_jsx_usage_candidates_from_expr(parsed, &paren.expr, scopes, candidates);
        }
        SimpleAssignTarget::TsAs(ts_as) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_as.expr, scopes, candidates);
        }
        SimpleAssignTarget::TsSatisfies(ts_satisfies) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_satisfies.expr, scopes, candidates);
        }
        SimpleAssignTarget::TsNonNull(ts_non_null) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_non_null.expr, scopes, candidates);
        }
        SimpleAssignTarget::TsTypeAssertion(ts_assertion) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_assertion.expr, scopes, candidates);
        }
        SimpleAssignTarget::TsInstantiation(ts_instantiation) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &ts_instantiation.expr,
                scopes,
                candidates,
            );
        }
        SimpleAssignTarget::OptChain(opt_chain) => match &*opt_chain.base {
            swc_ecma_ast::OptChainBase::Member(member) => {
                collect_jsx_usage_candidates_from_expr(parsed, &member.obj, scopes, candidates);
                if let MemberProp::Computed(computed) = &member.prop {
                    collect_jsx_usage_candidates_from_expr(
                        parsed,
                        &computed.expr,
                        scopes,
                        candidates,
                    );
                }
            }
            swc_ecma_ast::OptChainBase::Call(call) => {
                collect_jsx_usage_candidates_from_expr(parsed, &call.callee, scopes, candidates);
                for arg in &call.args {
                    collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, scopes, candidates);
                }
            }
        },
        SimpleAssignTarget::Ident(_) | SimpleAssignTarget::Invalid(_) => {}
    }
}

fn collect_jsx_usage_candidates_from_class(
    parsed: &ParsedReactModule,
    class: &Class,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let Some(super_class) = class.super_class.as_deref() {
        collect_jsx_usage_candidates_from_expr(parsed, super_class, scopes, candidates);
    }
    for member in &class.body {
        collect_jsx_usage_candidates_from_class_member(parsed, member, scopes, candidates);
    }
}

fn collect_jsx_usage_candidates_from_class_member(
    parsed: &ParsedReactModule,
    member: &ClassMember,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match member {
        ClassMember::Constructor(constructor) => {
            collect_jsx_usage_candidates_from_prop_name(
                parsed,
                &constructor.key,
                scopes,
                candidates,
            );
            let mut constructor_bindings = BTreeSet::new();
            for param in &constructor.params {
                if let swc_ecma_ast::ParamOrTsParamProp::Param(param) = param {
                    collect_pat_bindings(&param.pat, &mut constructor_bindings);
                }
            }
            scopes.push(constructor_bindings);
            if let Some(body) = &constructor.body {
                collect_jsx_usage_candidates_from_block(parsed, body, scopes, candidates);
            }
            scopes.pop();
        }
        ClassMember::Method(method) => {
            collect_jsx_usage_candidates_from_prop_name(parsed, &method.key, scopes, candidates);
            collect_jsx_usage_candidates_from_function(
                parsed,
                &method.function,
                scopes,
                candidates,
            );
        }
        ClassMember::PrivateMethod(method) => {
            collect_jsx_usage_candidates_from_function(
                parsed,
                &method.function,
                scopes,
                candidates,
            );
        }
        ClassMember::ClassProp(class_prop) => {
            collect_jsx_usage_candidates_from_prop_name(
                parsed,
                &class_prop.key,
                scopes,
                candidates,
            );
            if let Some(value) = class_prop.value.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, value, scopes, candidates);
            }
        }
        ClassMember::PrivateProp(private_prop) => {
            if let Some(value) = private_prop.value.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, value, scopes, candidates);
            }
        }
        ClassMember::StaticBlock(static_block) => {
            collect_jsx_usage_candidates_from_block(parsed, &static_block.body, scopes, candidates);
        }
        ClassMember::AutoAccessor(accessor) => {
            collect_jsx_usage_candidates_from_key(parsed, &accessor.key, scopes, candidates);
            if let Some(value) = accessor.value.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, value, scopes, candidates);
            }
        }
        ClassMember::TsIndexSignature(_) | ClassMember::Empty(_) => {}
    }
}

fn collect_jsx_usage_candidates_from_key(
    parsed: &ParsedReactModule,
    key: &Key,
    scopes: &mut UsageScopes,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let Key::Public(prop_name) = key {
        collect_jsx_usage_candidates_from_prop_name(parsed, prop_name, scopes, candidates);
    }
}

fn classify_jsx_usage(
    parsed: &ParsedReactModule,
    module_graph: &ReactModuleGraph,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
    candidate: &JsxUsageCandidate,
) -> Option<(String, MatchStatus)> {
    if candidate.shadowed {
        return None;
    }

    let import_package = module_graph
        .import_binding(&parsed.file, &candidate.binding_name)
        .map(|import| npm_import_package_root(&import.source_specifier));

    if let Some(registry_symbol) =
        resolve_usage_registry_symbol(parsed, module_graph, config, registry, candidate)
    {
        let registry_package = registry
            .component_packages
            .get(&registry_symbol)
            .and_then(|package| package.as_deref());
        if registry_package.is_none() {
            return Some((registry_symbol, MatchStatus::Resolved));
        }
        return resolve_import_aware_match(
            registry_package,
            import_package.as_deref(),
            &config.framework_packages,
        )
        .map(|match_status| (registry_symbol, match_status));
    }

    let registry_symbol =
        lookup_registry_symbol_by_name(parsed, module_graph, registry, candidate)?;
    let registry_package = registry
        .component_packages
        .get(&registry_symbol)
        .and_then(|package| package.as_deref());

    if let Some(match_status) = resolve_import_aware_match(
        registry_package,
        import_package.as_deref(),
        &config.framework_packages,
    ) {
        return Some((registry_symbol, match_status));
    }

    if registry_package.is_none()
        && import_package.as_deref().is_some_and(|package| {
            import_matches_framework_package(package, &config.framework_packages)
        })
    {
        return Some((registry_symbol, MatchStatus::FrameworkShadow));
    }

    None
}

fn lookup_registry_symbol_by_name(
    parsed: &ParsedReactModule,
    module_graph: &ReactModuleGraph,
    registry: &ReactRegistryIndex,
    candidate: &JsxUsageCandidate,
) -> Option<String> {
    if let Some(resolved) = module_graph.resolve_import(&parsed.file, &candidate.binding_name)
        && let Some(registry_symbol) = registry.resolve_targets.get(&resolved.symbol)
    {
        return Some(registry_symbol.clone());
    }

    registry
        .resolve_targets
        .get(&candidate.symbol)
        .or_else(|| registry.resolve_targets.get(&candidate.binding_name))
        .cloned()
}

fn resolve_usage_registry_symbol(
    parsed: &ParsedReactModule,
    module_graph: &ReactModuleGraph,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
    candidate: &JsxUsageCandidate,
) -> Option<String> {
    if candidate.shadowed {
        return None;
    }

    if !module_graph.import_resolves_through_configured_package(
        &parsed.file,
        &candidate.binding_name,
        config,
    ) {
        return None;
    }

    let resolved = module_graph.resolve_import(&parsed.file, &candidate.binding_name)?;

    if resolved.symbol == "*" {
        if let Some(member_symbol) = namespace_member_symbol(candidate) {
            if let Some(member_resolved) =
                module_graph.resolve_export(&resolved.module, &member_symbol)
                && let Some(registry_symbol) = registry.resolve_targets.get(&member_resolved.symbol)
            {
                return Some(registry_symbol.clone());
            }
            if let Some(registry_symbol) = registry.resolve_targets.get(&member_symbol) {
                return Some(registry_symbol.clone());
            }
        }
        return None;
    }

    registry
        .resolve_targets
        .get(&resolved.symbol)
        .or_else(|| registry.resolve_targets.get(&candidate.symbol))
        .or_else(|| registry.resolve_targets.get(&candidate.binding_name))
        .cloned()
}

fn namespace_member_symbol(candidate: &JsxUsageCandidate) -> Option<String> {
    let prefix = format!("{}.", candidate.binding_name);
    candidate.symbol.strip_prefix(&prefix).map(str::to_owned)
}

fn unresolved_usage_is_design_system_relevant(
    parsed: &ParsedReactModule,
    module_graph: &ReactModuleGraph,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
    candidate: &JsxUsageCandidate,
    local_bindings: &BTreeSet<String>,
) -> bool {
    if candidate.shadowed || local_bindings.contains(&candidate.binding_name) {
        return false;
    }

    if let Some(import) = module_graph.import_binding(&parsed.file, &candidate.binding_name) {
        if import.source_module.is_some() {
            return module_graph.import_resolves_through_configured_package(
                &parsed.file,
                &candidate.binding_name,
                config,
            );
        }

        return module_graph.unresolved_import_is_design_system_relevant(
            &parsed.file,
            &candidate.binding_name,
            registry,
            config,
        );
    }

    registry.resolve_targets.contains_key(&candidate.symbol)
        || registry
            .resolve_targets
            .contains_key(&candidate.binding_name)
}

fn binding_is_shadowed(binding_name: &str, scopes: &UsageScopes) -> bool {
    scopes
        .iter()
        .rev()
        .any(|scope| scope.contains(binding_name))
}

fn block_declared_bindings(block: &BlockStmt) -> BTreeSet<String> {
    let mut bindings = BTreeSet::new();
    for stmt in &block.stmts {
        collect_stmt_declared_bindings(stmt, &mut bindings);
    }
    bindings
}

fn collect_stmt_declared_bindings(stmt: &Stmt, bindings: &mut BTreeSet<String>) {
    if let Stmt::Decl(decl) = stmt {
        collect_declared_bindings(decl, bindings);
    }
}

fn for_declared_bindings(for_stmt: &swc_ecma_ast::ForStmt) -> BTreeSet<String> {
    let mut bindings = BTreeSet::new();
    if let Some(VarDeclOrExpr::VarDecl(var_decl)) = &for_stmt.init {
        collect_var_decl_bindings(var_decl, &mut bindings);
    }
    bindings
}

fn for_head_declared_bindings(for_head: &ForHead) -> BTreeSet<String> {
    let mut bindings = BTreeSet::new();
    match for_head {
        ForHead::VarDecl(var_decl) => collect_var_decl_bindings(var_decl, &mut bindings),
        ForHead::UsingDecl(_) => {}
        ForHead::Pat(pat) => collect_pat_bindings(pat, &mut bindings),
    }
    bindings
}

fn local_declared_bindings(parsed: &ParsedReactModule) -> BTreeSet<String> {
    let mut bindings = BTreeSet::new();
    for item in &parsed.module.body {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => collect_declared_bindings(decl, &mut bindings),
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
                collect_declared_bindings(&export_decl.decl, &mut bindings);
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
                match &default_decl.decl {
                    DefaultDecl::Fn(fn_expr) => {
                        if let Some(ident) = &fn_expr.ident {
                            bindings.insert(ident.sym.to_string());
                        }
                    }
                    DefaultDecl::Class(class_expr) => {
                        if let Some(ident) = &class_expr.ident {
                            bindings.insert(ident.sym.to_string());
                        }
                    }
                    DefaultDecl::TsInterfaceDecl(_) => {}
                }
            }
            _ => {}
        }
    }
    bindings
}

fn collect_declared_bindings(decl: &Decl, bindings: &mut BTreeSet<String>) {
    match decl {
        Decl::Class(class_decl) => {
            bindings.insert(class_decl.ident.sym.to_string());
        }
        Decl::Fn(fn_decl) => {
            bindings.insert(fn_decl.ident.sym.to_string());
        }
        Decl::Var(var_decl) => {
            collect_var_decl_bindings(var_decl, bindings);
        }
        _ => {}
    }
}

fn collect_var_decl_bindings(var_decl: &VarDecl, bindings: &mut BTreeSet<String>) {
    for declarator in &var_decl.decls {
        collect_pat_bindings(&declarator.name, bindings);
    }
}

fn collect_pat_bindings(pat: &Pat, bindings: &mut BTreeSet<String>) {
    match pat {
        Pat::Ident(binding) => {
            bindings.insert(binding.id.sym.to_string());
        }
        Pat::Array(array) => {
            for elem in array.elems.iter().flatten() {
                collect_pat_bindings(elem, bindings);
            }
        }
        Pat::Object(object) => {
            for prop in &object.props {
                match prop {
                    swc_ecma_ast::ObjectPatProp::KeyValue(key_value) => {
                        collect_pat_bindings(&key_value.value, bindings);
                    }
                    swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                        bindings.insert(assign.key.sym.to_string());
                    }
                    swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                        collect_pat_bindings(&rest.arg, bindings);
                    }
                }
            }
        }
        Pat::Rest(rest) => collect_pat_bindings(&rest.arg, bindings),
        Pat::Assign(assign) => collect_pat_bindings(&assign.left, bindings),
        Pat::Expr(_) | Pat::Invalid(_) => {}
    }
}

fn jsx_element_name(name: &JSXElementName) -> Option<(String, String)> {
    match name {
        JSXElementName::Ident(ident) => {
            let symbol = ident.sym.to_string();
            Some((symbol.clone(), symbol))
        }
        JSXElementName::JSXMemberExpr(member) => {
            let parts = jsx_member_expr_parts(member)?;
            let binding_name = parts.first()?.clone();
            Some((parts.join("."), binding_name))
        }
        JSXElementName::JSXNamespacedName(_) => None,
    }
}

fn jsx_member_expr_parts(member: &JSXMemberExpr) -> Option<Vec<String>> {
    let mut parts = jsx_object_parts(&member.obj)?;
    parts.push(member.prop.sym.to_string());
    Some(parts)
}

fn jsx_object_parts(object: &JSXObject) -> Option<Vec<String>> {
    match object {
        JSXObject::Ident(ident) => Some(vec![ident.sym.to_string()]),
        JSXObject::JSXMemberExpr(member) => jsx_member_expr_parts(member),
    }
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

fn local_component_id(file: &str, symbol: &str, location: &SourceLocation) -> String {
    format!("local.{file}:{}:{symbol}", location.line)
}

fn usage_site_id(location: &SourceLocation, symbol: &str) -> String {
    let column = location.column.unwrap_or(0);
    format!(
        "usage.{}:{}:{column}:{symbol}",
        location.file, location.line
    )
}

fn normalize_file(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{collect_usage_sites, discover_local_components};
    use crate::config::{PackageConfig, ReactScanConfig};
    use crate::files::ReactSourceFileCollection;
    use crate::module_graph::build_react_module_graph;
    use crate::registry::ReactRegistryIndex;
    use crate::swc_parse::{ReactParseOutcome, parse_react_source_file};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

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

    #[test]
    fn extract_usage_resolves_named_import_alias_and_registry_alias() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button as DsButton, PrimaryButton } from "@acme/design-system";

            export const App = () => (
                <>
                    <DsButton />
                    <PrimaryButton />
                </>
            );
            "#,
        );
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");
        fixture.write(
            "src/ds/PrimaryButton.tsx",
            "export const PrimaryButton = () => null;",
        );

        let extraction = fixture.extract_usage(
            vec![
                "src/App.tsx",
                "src/ds/Button.tsx",
                "src/ds/PrimaryButton.tsx",
            ],
            config_with_package(BTreeMap::from([
                ("Button".to_owned(), "src/ds/Button".to_owned()),
                (
                    "PrimaryButton".to_owned(),
                    "src/ds/PrimaryButton".to_owned(),
                ),
            ])),
            registry_with_aliases(&[("Button", &["PrimaryButton"])]),
        );

        assert_eq!(
            usage_symbols(&extraction.usage_sites),
            vec![
                ("DsButton".to_owned(), Some("Button".to_owned())),
                ("PrimaryButton".to_owned(), Some("Button".to_owned())),
            ]
        );
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_resolves_package_entrypoint_one_hop_re_export_and_member_jsx() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "@acme/design-system";

            export const App = () => <Button.Primary />;
            "#,
        );
        fixture.write("src/ds/index.ts", r#"export { Button } from "./Button";"#);
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/ds/index.ts", "src/ds/Button.tsx"],
            config_with_package(BTreeMap::from([(
                "Button".to_owned(),
                "src/ds/index".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert_eq!(
            usage_symbols(&extraction.usage_sites),
            vec![("Button.Primary".to_owned(), Some("Button".to_owned()))]
        );
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_ignores_lowercase_intrinsics_and_fragments() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            export const App = () => (
                <>
                    <div />
                    <section><span /></section>
                </>
            );
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx"],
            base_config(),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_scopes_unresolved_diagnostics_to_design_system_candidates() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "@acme/design-system";
            import { External } from "third-party";

            const LocalCard = () => <article />;
            export const App = () => (
                <>
                    <Button />
                    <PrimaryButton />
                    <External />
                    <LocalCard />
                </>
            );
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx"],
            config_with_package(BTreeMap::from([(
                "Button".to_owned(),
                "src/ds/MissingButton".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &["PrimaryButton"])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert_eq!(
            diagnostic_codes(&extraction.diagnostics),
            vec!["ds_usage_unresolved", "ds_usage_unresolved"]
        );
        assert!(
            extraction
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Button"))
        );
        assert!(
            extraction
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("PrimaryButton"))
        );
        assert!(
            extraction
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("External")
                    && !diagnostic.message.contains("LocalCard"))
        );
    }

    #[test]
    fn extract_usage_does_not_warn_for_local_registry_named_component() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            function Button() {
                return <button />;
            }

            export const App = () => <Button />;
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx"],
            base_config(),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_does_not_emit_for_relative_local_import_named_like_registry() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "./LocalButton";

            export const App = () => <Button />;
            "#,
        );
        fixture.write("src/LocalButton.tsx", "export const Button = () => null;");

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/LocalButton.tsx"],
            base_config(),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_does_not_warn_for_unresolved_third_party_import_named_like_registry() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "third-party";

            export const App = () => <Button />;
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx"],
            base_config(),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_respects_block_local_shadow_of_imported_design_system_component() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "@acme/design-system";

            const LocalButton = () => <button />;
            export function App() {
                const Button = LocalButton;
                return <Button />;
            }
            "#,
        );
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/ds/Button.tsx"],
            config_with_package(BTreeMap::from([(
                "Button".to_owned(),
                "src/ds/Button".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_collects_jsx_inside_object_literals_and_assignments() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import { Button } from "@acme/design-system";

            export function App() {
                const slots = { cta: <Button /> };
                let slot;
                slot = <Button />;
                return <>{slots.cta}{slot}</>;
            }
            "#,
        );
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/ds/Button.tsx"],
            config_with_package(BTreeMap::from([(
                "Button".to_owned(),
                "src/ds/Button".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert_eq!(
            usage_symbols(&extraction.usage_sites),
            vec![
                ("Button".to_owned(), Some("Button".to_owned())),
                ("Button".to_owned(), Some("Button".to_owned())),
            ]
        );
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_resolves_default_import_with_aliased_local_name() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import DsButton from "@acme/design-system";

            export const App = () => <DsButton />;
            "#,
        );
        fixture.write(
            "src/ds/Button.tsx",
            "export default function Button() { return null; }",
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/ds/Button.tsx"],
            config_with_package(BTreeMap::from([(
                ".".to_owned(),
                "src/ds/Button".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert_eq!(
            usage_symbols(&extraction.usage_sites),
            vec![("DsButton".to_owned(), Some("Button".to_owned()))]
        );
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_suppresses_self_reference_in_default_export_class() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import React from "react";

            export default class Button extends React.Component {
                render() {
                    return <Button />;
                }
            }
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx"],
            base_config(),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert!(extraction.usage_sites.is_empty());
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_resolves_namespace_import_member_jsx() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import * as DS from "@acme/design-system";

            export const App = () => <DS.Button />;
            "#,
        );
        fixture.write("src/ds/index.ts", r#"export { Button } from "./Button";"#);
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/ds/index.ts", "src/ds/Button.tsx"],
            config_with_package(BTreeMap::from([(
                ".".to_owned(),
                "src/ds/index".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert_eq!(
            usage_symbols(&extraction.usage_sites),
            vec![("DS.Button".to_owned(), Some("Button".to_owned()))]
        );
        assert!(extraction.diagnostics.is_empty());
    }

    #[test]
    fn extract_usage_collects_jsx_inside_class_render_method() {
        let fixture = Fixture::new();
        fixture.write(
            "src/App.tsx",
            r#"
            import React from "react";
            import { Button } from "@acme/design-system";

            export class App extends React.Component {
                render() {
                    return <Button />;
                }
            }
            "#,
        );
        fixture.write("src/ds/Button.tsx", "export const Button = () => null;");

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/ds/Button.tsx"],
            config_with_package(BTreeMap::from([(
                "Button".to_owned(),
                "src/ds/Button".to_owned(),
            )])),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert_eq!(
            usage_symbols(&extraction.usage_sites),
            vec![("Button".to_owned(), Some("Button".to_owned()))]
        );
        assert!(extraction.diagnostics.is_empty());
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

    fn usage_symbols(usage_sites: &[wax_contract::UsageSite]) -> Vec<(String, Option<String>)> {
        usage_sites
            .iter()
            .map(|site| (site.symbol.clone(), site.registry_symbol.clone()))
            .collect()
    }

    fn diagnostic_codes(diagnostics: &[wax_contract::Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    fn base_config() -> ReactScanConfig {
        ReactScanConfig {
            design_system_registry: PathBuf::from("design-system/registry.json"),
            roots: vec![PathBuf::from("src")],
            ignore: Vec::new(),
            tsconfig: None,
            aliases: BTreeMap::new(),
            packages: BTreeMap::new(),
            framework_packages: Vec::new(),
        }
    }

    fn config_with_package(exports: BTreeMap<String, String>) -> ReactScanConfig {
        ReactScanConfig {
            packages: BTreeMap::from([(
                "@acme/design-system".to_owned(),
                PackageConfig { exports },
            )]),
            ..base_config()
        }
    }

    fn registry_with_aliases(symbols: &[(&str, &[&str])]) -> ReactRegistryIndex {
        let mut resolve_targets = BTreeMap::new();
        let mut component_packages = BTreeMap::new();
        for (symbol, aliases) in symbols {
            resolve_targets.insert((*symbol).to_owned(), (*symbol).to_owned());
            component_packages.insert((*symbol).to_owned(), None);
            for alias in *aliases {
                resolve_targets.insert((*alias).to_owned(), (*symbol).to_owned());
            }
        }
        ReactRegistryIndex {
            design_system_components: Vec::new(),
            resolve_targets,
            component_packages,
        }
    }

    fn registry_with_package(symbol: &str, package: &str) -> ReactRegistryIndex {
        ReactRegistryIndex {
            design_system_components: Vec::new(),
            resolve_targets: BTreeMap::from([(symbol.to_owned(), symbol.to_owned())]),
            component_packages: BTreeMap::from([(symbol.to_owned(), Some(package.to_owned()))]),
        }
    }

    #[test]
    fn framework_import_becomes_framework_shadow_when_registry_package_is_set() {
        let fixture = Fixture::new();
        fixture.write(
            "src/Screen.tsx",
            r#"
            import { Button } from "@foundation/ui";
            export function Screen() {
                return <Button />;
            }
            "#,
        );

        let mut config = base_config();
        config.framework_packages = vec!["@foundation/ui".to_owned()];
        let extraction = fixture.extract_usage(
            vec!["src/Screen.tsx"],
            config,
            registry_with_package("Button", "@acme/design-system"),
        );

        assert_eq!(extraction.usage_sites.len(), 1);
        assert_eq!(
            extraction.usage_sites[0].match_status,
            wax_contract::MatchStatus::FrameworkShadow
        );
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

        fn extract_usage(
            &self,
            module_files: Vec<&str>,
            config: ReactScanConfig,
            registry: ReactRegistryIndex,
        ) -> crate::extract::ReactUsageExtraction {
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
            let graph_build = build_react_module_graph(
                self.root.path(),
                &parsed_modules,
                &files,
                &config,
                &registry,
            );
            collect_usage_sites(&parsed_modules, &graph_build.graph, &config, &registry)
        }
    }
}
