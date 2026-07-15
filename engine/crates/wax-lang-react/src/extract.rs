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
    Diagnostic, HardcodedStyleSite, IdentityStability, LocalComponent, MatchStatus, ParentScope,
    SourceLocation, TokenSite, UsageSite,
};
use wax_lang_api::{npm_import_package_root, resolve_import_aware_match};

use crate::component_detect::{
    class_returns_jsx, expression_returns_jsx, function_returns_jsx, is_pascal_case,
    module_export_name, simple_binding_ident,
};
use crate::component_scope::{
    collect_component_definitions, parent_scope_for_component as parent_scope_for_component_def,
};
use crate::config::ReactScanConfig;
use crate::module_graph::{ImportedSymbol, ReactModuleGraph};
use crate::registry::ReactRegistryIndex;
use crate::style_extract::collect_hardcoded_style_sites;
use crate::swc_parse::ParsedReactModule;
use crate::token_extract::collect_token_sites;

/// JSX usage extraction output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReactUsageExtraction {
    /// Resolved registry-backed JSX usage sites.
    pub usage_sites: Vec<UsageSite>,
    /// Exact token reference sites discovered in source.
    pub token_sites: Vec<TokenSite>,
    /// Hard-coded styling candidates discovered in JSX `style` props.
    pub hardcoded_style_sites: Vec<HardcodedStyleSite>,
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

#[derive(Debug, Default)]
struct LocalComponentIndex {
    by_file_symbol: BTreeMap<(String, String), LocalComponent>,
    by_qualified: BTreeMap<String, LocalComponent>,
}

impl LocalComponentIndex {
    fn from_components(components: &[LocalComponent]) -> Self {
        let mut index = Self::default();
        for component in components {
            let file = normalize_file(std::path::Path::new(&component.location.file));
            if let Some(qualified) = &component.qualified_symbol {
                index
                    .by_qualified
                    .insert(qualified.clone(), component.clone());
            }
            index
                .by_file_symbol
                .insert((file, component.symbol.clone()), component.clone());
        }
        index
    }

    fn resolve(
        &self,
        file: &std::path::Path,
        binding_name: &str,
        symbol: &str,
    ) -> Option<&LocalComponent> {
        let file = normalize_file(file);
        if let Some(component) = self
            .by_file_symbol
            .get(&(file.clone(), binding_name.to_owned()))
        {
            return Some(component);
        }
        if let Some(component) = self.by_file_symbol.get(&(file.clone(), symbol.to_owned())) {
            return Some(component);
        }
        let module_identity = module_identity_for_file(&file);
        let qualified = qualified_component_symbol(&module_identity, symbol);
        self.by_qualified.get(&qualified)
    }

    fn resolve_with_import(
        &self,
        module_graph: &ReactModuleGraph,
        parsed: &ParsedReactModule,
        binding_name: &str,
        symbol: &str,
    ) -> Option<&LocalComponent> {
        if let Some(local) = self.resolve(&parsed.file, binding_name, symbol) {
            return Some(local);
        }
        let resolved = module_graph.resolve_import(&parsed.file, binding_name)?;
        self.resolve(&resolved.module, &resolved.symbol, &resolved.symbol)
    }
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

fn parent_scope_for_component(
    parsed: &ParsedReactModule,
    symbol: &str,
    span: Span,
) -> Option<ParentScope> {
    parent_scope_for_component_def(parsed, symbol, span)
}

/// Collects registry-backed JSX usage sites from parsed modules.
#[must_use]
pub fn collect_usage_sites(
    parsed_modules: &[ParsedReactModule],
    module_graph: &ReactModuleGraph,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
    local_components: &[LocalComponent],
) -> ReactUsageExtraction {
    let local_index = LocalComponentIndex::from_components(local_components);
    let mut extraction = ReactUsageExtraction::default();

    for parsed in parsed_modules {
        let mut candidates = Vec::new();
        for item in &parsed.module.body {
            collect_jsx_usage_candidates_from_module_item(parsed, item, &mut candidates);
        }

        for candidate in &candidates {
            if candidate.shadowed {
                continue;
            }

            let Some(location) = parsed.source_location_from_span(candidate.span) else {
                continue;
            };
            let parent = candidate
                .parent_component
                .as_ref()
                .and_then(|(name, span)| parent_scope_for_component(parsed, name, *span));

            if let Some(local) =
                local_index.resolve(&parsed.file, &candidate.binding_name, &candidate.symbol)
            {
                extraction.usage_sites.push(UsageSite {
                    id: usage_site_id(&location, &candidate.symbol),
                    location,
                    symbol: candidate.symbol.clone(),
                    qualified_symbol: local.qualified_symbol.clone(),
                    match_status: MatchStatus::Local,
                    registry_symbol: None,
                    local_definition_id: Some(local.id.clone()),
                    parent,
                });
            } else if let Some((registry_symbol, match_status)) =
                classify_jsx_usage(parsed, module_graph, config, registry, candidate)
            {
                extraction.usage_sites.push(UsageSite {
                    id: usage_site_id(&location, &candidate.symbol),
                    location,
                    symbol: candidate.symbol.clone(),
                    qualified_symbol: None,
                    match_status,
                    registry_symbol: Some(registry_symbol),
                    local_definition_id: None,
                    parent,
                });
            } else if let Some(local) = local_index.resolve_with_import(
                module_graph,
                parsed,
                &candidate.binding_name,
                &candidate.symbol,
            ) {
                extraction.usage_sites.push(UsageSite {
                    id: usage_site_id(&location, &candidate.symbol),
                    location,
                    symbol: candidate.symbol.clone(),
                    qualified_symbol: local.qualified_symbol.clone(),
                    match_status: MatchStatus::Local,
                    registry_symbol: None,
                    local_definition_id: Some(local.id.clone()),
                    parent,
                });
            } else if unresolved_usage_is_design_system_relevant(
                parsed,
                module_graph,
                config,
                registry,
                candidate,
                &local_declared_bindings(parsed),
            ) {
                extraction.usage_sites.push(UsageSite {
                    id: usage_site_id(&location, &candidate.symbol),
                    location,
                    symbol: candidate.symbol.clone(),
                    qualified_symbol: None,
                    match_status: MatchStatus::Unresolved,
                    registry_symbol: None,
                    local_definition_id: None,
                    parent,
                });
            }
        }

        let components = collect_component_definitions(parsed);
        extraction.token_sites.extend(collect_token_sites(
            parsed,
            &registry.token_index,
            &components,
        ));
        extraction
            .hardcoded_style_sites
            .extend(collect_hardcoded_style_sites(parsed, &components));
    }

    extraction.token_sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.location.column.cmp(&right.location.column))
            .then(left.token_id.cmp(&right.token_id))
    });
    extraction.hardcoded_style_sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.location.column.cmp(&right.location.column))
            .then(left.value.cmp(&right.value))
    });

    extraction
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JsxUsageCandidate {
    symbol: String,
    binding_name: String,
    span: Span,
    shadowed: bool,
    parent_component: Option<(String, Span)>,
}

#[derive(Debug, Default)]
struct UsageWalkState {
    scopes: Vec<BTreeSet<String>>,
    parent_stack: Vec<(String, Span)>,
}

fn collect_jsx_usage_candidates_from_module_item(
    parsed: &ParsedReactModule,
    item: &ModuleItem,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    let mut state = UsageWalkState::default();
    match item {
        ModuleItem::Stmt(stmt) => {
            collect_jsx_usage_candidates_from_stmt(parsed, stmt, &mut state, candidates);
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
            collect_jsx_usage_candidates_from_decl(
                parsed,
                &export_decl.decl,
                &mut state,
                candidates,
            );
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
            if let DefaultDecl::Fn(fn_expr) = &default_decl.decl {
                let component_name = fn_expr.ident.as_ref().map(|ident| ident.sym.as_ref());
                collect_jsx_usage_candidates_from_function(
                    parsed,
                    &fn_expr.function,
                    &mut state,
                    candidates,
                    component_name,
                );
            } else if let DefaultDecl::Class(class_expr) = &default_decl.decl {
                collect_jsx_usage_candidates_from_class(
                    parsed,
                    &class_expr.class,
                    &mut state,
                    candidates,
                    class_expr.ident.as_ref().map(|ident| ident.sym.as_ref()),
                );
            }
        }
        ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr)) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &default_expr.expr,
                &mut state,
                candidates,
            );
        }
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_decl(
    parsed: &ParsedReactModule,
    decl: &Decl,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match decl {
        Decl::Fn(fn_decl) => {
            collect_jsx_usage_candidates_from_function(
                parsed,
                &fn_decl.function,
                state,
                candidates,
                Some(fn_decl.ident.sym.as_ref()),
            );
        }
        Decl::Var(var_decl) => {
            collect_jsx_usage_candidates_from_var_decl(parsed, var_decl, state, candidates);
        }
        Decl::Class(class_decl) => {
            collect_jsx_usage_candidates_from_class(
                parsed,
                &class_decl.class,
                state,
                candidates,
                Some(class_decl.ident.sym.as_ref()),
            );
        }
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_function(
    parsed: &ParsedReactModule,
    function: &Function,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
    component_name: Option<&str>,
) {
    let parent_frame = component_name
        .filter(|name| is_pascal_case(name))
        .filter(|_| function_returns_jsx(function))
        .map(|name| (name.to_owned(), function.span));
    if let Some(frame) = parent_frame.clone() {
        state.parent_stack.push(frame);
    }

    let mut function_bindings = BTreeSet::new();
    for param in &function.params {
        collect_pat_bindings(&param.pat, &mut function_bindings);
    }
    state.scopes.push(function_bindings);
    if let Some(body) = &function.body {
        collect_jsx_usage_candidates_from_block(parsed, body, state, candidates);
    }
    state.scopes.pop();

    if parent_frame.is_some() {
        state.parent_stack.pop();
    }
}

fn collect_jsx_usage_candidates_from_block(
    parsed: &ParsedReactModule,
    block: &BlockStmt,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    state.scopes.push(block_declared_bindings(block));
    for stmt in &block.stmts {
        collect_jsx_usage_candidates_from_stmt(parsed, stmt, state, candidates);
    }
    state.scopes.pop();
}

fn collect_jsx_usage_candidates_from_stmt(
    parsed: &ParsedReactModule,
    stmt: &Stmt,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match stmt {
        Stmt::Decl(decl) => collect_jsx_usage_candidates_from_decl(parsed, decl, state, candidates),
        Stmt::Return(return_stmt) => {
            if let Some(arg) = return_stmt.arg.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, arg, state, candidates);
            }
        }
        Stmt::Expr(expr_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &expr_stmt.expr, state, candidates);
        }
        Stmt::Block(block) => {
            collect_jsx_usage_candidates_from_block(parsed, block, state, candidates);
        }
        Stmt::If(if_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &if_stmt.test, state, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &if_stmt.cons, state, candidates);
            if let Some(alt) = if_stmt.alt.as_deref() {
                collect_jsx_usage_candidates_from_stmt(parsed, alt, state, candidates);
            }
        }
        Stmt::With(with_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &with_stmt.obj, state, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &with_stmt.body, state, candidates);
        }
        Stmt::Labeled(labeled) => {
            collect_jsx_usage_candidates_from_stmt(parsed, &labeled.body, state, candidates);
        }
        Stmt::Throw(throw_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &throw_stmt.arg, state, candidates);
        }
        Stmt::Switch(switch_stmt) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &switch_stmt.discriminant,
                state,
                candidates,
            );
            for case in &switch_stmt.cases {
                if let Some(test) = case.test.as_deref() {
                    collect_jsx_usage_candidates_from_expr(parsed, test, state, candidates);
                }
                let mut case_bindings = BTreeSet::new();
                for stmt in &case.cons {
                    collect_stmt_declared_bindings(stmt, &mut case_bindings);
                }
                state.scopes.push(case_bindings);
                for stmt in &case.cons {
                    collect_jsx_usage_candidates_from_stmt(parsed, stmt, state, candidates);
                }
                state.scopes.pop();
            }
        }
        Stmt::Try(try_stmt) => {
            collect_jsx_usage_candidates_from_block(parsed, &try_stmt.block, state, candidates);
            if let Some(handler) = &try_stmt.handler {
                let mut catch_bindings = BTreeSet::new();
                if let Some(param) = &handler.param {
                    collect_pat_bindings(param, &mut catch_bindings);
                }
                state.scopes.push(catch_bindings);
                collect_jsx_usage_candidates_from_block(parsed, &handler.body, state, candidates);
                state.scopes.pop();
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                collect_jsx_usage_candidates_from_block(parsed, finalizer, state, candidates);
            }
        }
        Stmt::While(while_stmt) => {
            collect_jsx_usage_candidates_from_expr(parsed, &while_stmt.test, state, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &while_stmt.body, state, candidates);
        }
        Stmt::DoWhile(do_while) => {
            collect_jsx_usage_candidates_from_stmt(parsed, &do_while.body, state, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &do_while.test, state, candidates);
        }
        Stmt::For(for_stmt) => {
            state.scopes.push(for_declared_bindings(for_stmt));
            if let Some(init) = &for_stmt.init {
                collect_jsx_usage_candidates_from_for_init(parsed, init, state, candidates);
            }
            if let Some(test) = for_stmt.test.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, test, state, candidates);
            }
            if let Some(update) = for_stmt.update.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, update, state, candidates);
            }
            collect_jsx_usage_candidates_from_stmt(parsed, &for_stmt.body, state, candidates);
            state.scopes.pop();
        }
        Stmt::ForIn(for_in) => {
            state.scopes.push(for_head_declared_bindings(&for_in.left));
            collect_jsx_usage_candidates_from_expr(parsed, &for_in.right, state, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &for_in.body, state, candidates);
            state.scopes.pop();
        }
        Stmt::ForOf(for_of) => {
            state.scopes.push(for_head_declared_bindings(&for_of.left));
            collect_jsx_usage_candidates_from_expr(parsed, &for_of.right, state, candidates);
            collect_jsx_usage_candidates_from_stmt(parsed, &for_of.body, state, candidates);
            state.scopes.pop();
        }
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_expr(
    parsed: &ParsedReactModule,
    expr: &Expr,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match expr {
        Expr::JSXElement(element) => {
            collect_jsx_usage_candidates_from_jsx_element(parsed, element, state, candidates);
        }
        Expr::JSXFragment(fragment) => {
            collect_jsx_usage_candidates_from_jsx_fragment(parsed, fragment, state, candidates);
        }
        Expr::Arrow(arrow) => match &*arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => {
                let mut arrow_bindings = BTreeSet::new();
                for param in &arrow.params {
                    collect_pat_bindings(param, &mut arrow_bindings);
                }
                state.scopes.push(arrow_bindings);
                collect_jsx_usage_candidates_from_block(parsed, block, state, candidates);
                state.scopes.pop();
            }
            BlockStmtOrExpr::Expr(expr) => {
                let mut arrow_bindings = BTreeSet::new();
                for param in &arrow.params {
                    collect_pat_bindings(param, &mut arrow_bindings);
                }
                state.scopes.push(arrow_bindings);
                collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
                state.scopes.pop();
            }
        },
        Expr::Fn(fn_expr) => {
            let mut fn_bindings = BTreeSet::new();
            if let Some(ident) = &fn_expr.ident {
                fn_bindings.insert(ident.sym.to_string());
            }
            state.scopes.push(fn_bindings);
            collect_jsx_usage_candidates_from_function(
                parsed,
                &fn_expr.function,
                state,
                candidates,
                fn_expr.ident.as_ref().map(|ident| ident.sym.as_ref()),
            );
            state.scopes.pop();
        }
        Expr::Class(class_expr) => {
            let mut class_bindings = BTreeSet::new();
            if let Some(ident) = &class_expr.ident {
                class_bindings.insert(ident.sym.to_string());
            }
            state.scopes.push(class_bindings);
            collect_jsx_usage_candidates_from_class(
                parsed,
                &class_expr.class,
                state,
                candidates,
                class_expr.ident.as_ref().map(|ident| ident.sym.as_ref()),
            );
            state.scopes.pop();
        }
        Expr::Paren(paren) => {
            collect_jsx_usage_candidates_from_expr(parsed, &paren.expr, state, candidates);
        }
        Expr::Cond(cond) => {
            collect_jsx_usage_candidates_from_expr(parsed, &cond.test, state, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &cond.cons, state, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &cond.alt, state, candidates);
        }
        Expr::Bin(binary) => {
            collect_jsx_usage_candidates_from_expr(parsed, &binary.left, state, candidates);
            collect_jsx_usage_candidates_from_expr(parsed, &binary.right, state, candidates);
        }
        Expr::Call(call) => {
            if let Callee::Expr(callee) = &call.callee {
                collect_jsx_usage_candidates_from_expr(parsed, callee, state, candidates);
            }
            for arg in &call.args {
                collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, state, candidates);
            }
        }
        Expr::Array(array) => {
            for elem in array.elems.iter().flatten() {
                collect_jsx_usage_candidates_from_expr(parsed, &elem.expr, state, candidates);
            }
        }
        Expr::Object(object) => {
            for prop in &object.props {
                collect_jsx_usage_candidates_from_object_prop(parsed, prop, state, candidates);
            }
        }
        Expr::Assign(assign) => {
            collect_jsx_usage_candidates_from_assign_target(
                parsed,
                &assign.left,
                state,
                candidates,
            );
            collect_jsx_usage_candidates_from_expr(parsed, &assign.right, state, candidates);
        }
        Expr::Seq(seq) => {
            for expr in &seq.exprs {
                collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
            }
        }
        Expr::Await(await_expr) => {
            collect_jsx_usage_candidates_from_expr(parsed, &await_expr.arg, state, candidates);
        }
        Expr::Yield(yield_expr) => {
            if let Some(arg) = yield_expr.arg.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, arg, state, candidates);
            }
        }
        Expr::Tpl(tpl) => {
            for expr in &tpl.exprs {
                collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
            }
        }
        Expr::TaggedTpl(tagged) => {
            collect_jsx_usage_candidates_from_expr(parsed, &tagged.tag, state, candidates);
            for expr in &tagged.tpl.exprs {
                collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
            }
        }
        Expr::Unary(unary) => {
            collect_jsx_usage_candidates_from_expr(parsed, &unary.arg, state, candidates);
        }
        Expr::Update(update) => {
            collect_jsx_usage_candidates_from_expr(parsed, &update.arg, state, candidates);
        }
        Expr::Member(member) => {
            collect_jsx_usage_candidates_from_expr(parsed, &member.obj, state, candidates);
            if let MemberProp::Computed(computed) = &member.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, state, candidates);
            }
        }
        Expr::SuperProp(super_prop) => {
            if let swc_ecma_ast::SuperProp::Computed(computed) = &super_prop.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, state, candidates);
            }
        }
        Expr::New(new_expr) => {
            collect_jsx_usage_candidates_from_expr(parsed, &new_expr.callee, state, candidates);
            if let Some(args) = &new_expr.args {
                for arg in args {
                    collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, state, candidates);
                }
            }
        }
        Expr::TsAs(ts_as) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_as.expr, state, candidates);
        }
        Expr::TsSatisfies(ts_satisfies) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_satisfies.expr, state, candidates);
        }
        Expr::TsNonNull(ts_non_null) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_non_null.expr, state, candidates);
        }
        Expr::TsTypeAssertion(ts_assertion) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_assertion.expr, state, candidates);
        }
        Expr::TsConstAssertion(ts_const) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_const.expr, state, candidates);
        }
        Expr::TsInstantiation(ts_instantiation) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &ts_instantiation.expr,
                state,
                candidates,
            );
        }
        Expr::OptChain(opt_chain) => match &*opt_chain.base {
            swc_ecma_ast::OptChainBase::Member(member) => {
                collect_jsx_usage_candidates_from_expr(parsed, &member.obj, state, candidates);
                if let MemberProp::Computed(computed) = &member.prop {
                    collect_jsx_usage_candidates_from_expr(
                        parsed,
                        &computed.expr,
                        state,
                        candidates,
                    );
                }
            }
            swc_ecma_ast::OptChainBase::Call(call) => {
                collect_jsx_usage_candidates_from_expr(parsed, &call.callee, state, candidates);
                for arg in &call.args {
                    collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, state, candidates);
                }
            }
        },
        _ => {}
    }
}

fn collect_jsx_usage_candidates_from_jsx_element(
    parsed: &ParsedReactModule,
    element: &JSXElement,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let Some((symbol, binding_name)) = jsx_element_name(&element.opening.name)
        && is_pascal_case(&binding_name)
    {
        let shadowed = binding_is_shadowed(&binding_name, &state.scopes);
        candidates.push(JsxUsageCandidate {
            symbol,
            binding_name,
            span: element.opening.span,
            shadowed,
            parent_component: state.parent_stack.last().cloned(),
        });
    }

    for attr in &element.opening.attrs {
        collect_jsx_usage_candidates_from_jsx_attr(parsed, attr, state, candidates);
    }
    for child in &element.children {
        collect_jsx_usage_candidates_from_jsx_child(parsed, child, state, candidates);
    }
}

fn collect_jsx_usage_candidates_from_jsx_fragment(
    parsed: &ParsedReactModule,
    fragment: &JSXFragment,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    for child in &fragment.children {
        collect_jsx_usage_candidates_from_jsx_child(parsed, child, state, candidates);
    }
}

fn collect_jsx_usage_candidates_from_jsx_child(
    parsed: &ParsedReactModule,
    child: &JSXElementChild,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match child {
        JSXElementChild::JSXElement(element) => {
            collect_jsx_usage_candidates_from_jsx_element(parsed, element, state, candidates);
        }
        JSXElementChild::JSXFragment(fragment) => {
            collect_jsx_usage_candidates_from_jsx_fragment(parsed, fragment, state, candidates);
        }
        JSXElementChild::JSXExprContainer(container) => {
            if let JSXExpr::Expr(expr) = &container.expr {
                collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
            }
        }
        JSXElementChild::JSXSpreadChild(spread) => {
            collect_jsx_usage_candidates_from_expr(parsed, &spread.expr, state, candidates);
        }
        JSXElementChild::JSXText(_) => {}
    }
}

fn collect_jsx_usage_candidates_from_jsx_attr(
    parsed: &ParsedReactModule,
    attr: &JSXAttrOrSpread,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match attr {
        JSXAttrOrSpread::JSXAttr(attr) => match &attr.value {
            Some(JSXAttrValue::JSXExprContainer(container)) => {
                if let JSXExpr::Expr(expr) = &container.expr {
                    collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
                }
            }
            Some(JSXAttrValue::JSXElement(element)) => {
                collect_jsx_usage_candidates_from_jsx_element(parsed, element, state, candidates);
            }
            Some(JSXAttrValue::JSXFragment(fragment)) => {
                collect_jsx_usage_candidates_from_jsx_fragment(parsed, fragment, state, candidates);
            }
            Some(JSXAttrValue::Str(_)) | None => {}
        },
        JSXAttrOrSpread::SpreadElement(spread) => {
            collect_jsx_usage_candidates_from_expr(parsed, &spread.expr, state, candidates);
        }
    }
}

fn collect_jsx_usage_candidates_from_var_decl(
    parsed: &ParsedReactModule,
    var_decl: &VarDecl,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    for declarator in &var_decl.decls {
        if let Some(init) = declarator.init.as_deref() {
            collect_jsx_usage_candidates_from_expr(parsed, init, state, candidates);
        }
    }
}

fn collect_jsx_usage_candidates_from_for_init(
    parsed: &ParsedReactModule,
    init: &VarDeclOrExpr,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match init {
        VarDeclOrExpr::VarDecl(var_decl) => {
            collect_jsx_usage_candidates_from_var_decl(parsed, var_decl, state, candidates);
        }
        VarDeclOrExpr::Expr(expr) => {
            collect_jsx_usage_candidates_from_expr(parsed, expr, state, candidates);
        }
    }
}

fn collect_jsx_usage_candidates_from_object_prop(
    parsed: &ParsedReactModule,
    prop: &PropOrSpread,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match prop {
        PropOrSpread::Spread(spread) => {
            collect_jsx_usage_candidates_from_expr(parsed, &spread.expr, state, candidates);
        }
        PropOrSpread::Prop(prop) => match &**prop {
            Prop::KeyValue(key_value) => {
                collect_jsx_usage_candidates_from_prop_name(
                    parsed,
                    &key_value.key,
                    state,
                    candidates,
                );
                collect_jsx_usage_candidates_from_expr(parsed, &key_value.value, state, candidates);
            }
            Prop::Assign(assign) => {
                collect_jsx_usage_candidates_from_expr(parsed, &assign.value, state, candidates);
            }
            Prop::Getter(getter) => {
                collect_jsx_usage_candidates_from_prop_name(parsed, &getter.key, state, candidates);
                if let Some(body) = &getter.body {
                    collect_jsx_usage_candidates_from_block(parsed, body, state, candidates);
                }
            }
            Prop::Setter(setter) => {
                collect_jsx_usage_candidates_from_prop_name(parsed, &setter.key, state, candidates);
                let mut setter_bindings = BTreeSet::new();
                collect_pat_bindings(&setter.param, &mut setter_bindings);
                state.scopes.push(setter_bindings);
                if let Some(body) = &setter.body {
                    collect_jsx_usage_candidates_from_block(parsed, body, state, candidates);
                }
                state.scopes.pop();
            }
            Prop::Method(method) => {
                collect_jsx_usage_candidates_from_prop_name(parsed, &method.key, state, candidates);
                collect_jsx_usage_candidates_from_function(
                    parsed,
                    &method.function,
                    state,
                    candidates,
                    None,
                );
            }
            Prop::Shorthand(_) => {}
        },
    }
}

fn collect_jsx_usage_candidates_from_prop_name(
    parsed: &ParsedReactModule,
    prop_name: &PropName,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let PropName::Computed(computed) = prop_name {
        collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, state, candidates);
    }
}

fn collect_jsx_usage_candidates_from_assign_target(
    parsed: &ParsedReactModule,
    target: &AssignTarget,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let AssignTarget::Simple(simple) = target {
        collect_jsx_usage_candidates_from_simple_assign_target(parsed, simple, state, candidates);
    }
}

fn collect_jsx_usage_candidates_from_simple_assign_target(
    parsed: &ParsedReactModule,
    target: &SimpleAssignTarget,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match target {
        SimpleAssignTarget::Member(member) => {
            collect_jsx_usage_candidates_from_expr(parsed, &member.obj, state, candidates);
            if let MemberProp::Computed(computed) = &member.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, state, candidates);
            }
        }
        SimpleAssignTarget::SuperProp(super_prop) => {
            if let swc_ecma_ast::SuperProp::Computed(computed) = &super_prop.prop {
                collect_jsx_usage_candidates_from_expr(parsed, &computed.expr, state, candidates);
            }
        }
        SimpleAssignTarget::Paren(paren) => {
            collect_jsx_usage_candidates_from_expr(parsed, &paren.expr, state, candidates);
        }
        SimpleAssignTarget::TsAs(ts_as) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_as.expr, state, candidates);
        }
        SimpleAssignTarget::TsSatisfies(ts_satisfies) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_satisfies.expr, state, candidates);
        }
        SimpleAssignTarget::TsNonNull(ts_non_null) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_non_null.expr, state, candidates);
        }
        SimpleAssignTarget::TsTypeAssertion(ts_assertion) => {
            collect_jsx_usage_candidates_from_expr(parsed, &ts_assertion.expr, state, candidates);
        }
        SimpleAssignTarget::TsInstantiation(ts_instantiation) => {
            collect_jsx_usage_candidates_from_expr(
                parsed,
                &ts_instantiation.expr,
                state,
                candidates,
            );
        }
        SimpleAssignTarget::OptChain(opt_chain) => match &*opt_chain.base {
            swc_ecma_ast::OptChainBase::Member(member) => {
                collect_jsx_usage_candidates_from_expr(parsed, &member.obj, state, candidates);
                if let MemberProp::Computed(computed) = &member.prop {
                    collect_jsx_usage_candidates_from_expr(
                        parsed,
                        &computed.expr,
                        state,
                        candidates,
                    );
                }
            }
            swc_ecma_ast::OptChainBase::Call(call) => {
                collect_jsx_usage_candidates_from_expr(parsed, &call.callee, state, candidates);
                for arg in &call.args {
                    collect_jsx_usage_candidates_from_expr(parsed, &arg.expr, state, candidates);
                }
            }
        },
        SimpleAssignTarget::Ident(_) | SimpleAssignTarget::Invalid(_) => {}
    }
}

fn collect_jsx_usage_candidates_from_class(
    parsed: &ParsedReactModule,
    class: &Class,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
    class_name: Option<&str>,
) {
    let parent_frame = class_name
        .filter(|name| is_pascal_case(name))
        .filter(|_| class_returns_jsx(class))
        .map(|name| (name.to_owned(), class.span));
    if let Some(frame) = parent_frame.clone() {
        state.parent_stack.push(frame);
    }

    if let Some(super_class) = class.super_class.as_deref() {
        collect_jsx_usage_candidates_from_expr(parsed, super_class, state, candidates);
    }
    for member in &class.body {
        collect_jsx_usage_candidates_from_class_member(parsed, member, state, candidates);
    }

    if parent_frame.is_some() {
        state.parent_stack.pop();
    }
}

fn collect_jsx_usage_candidates_from_class_member(
    parsed: &ParsedReactModule,
    member: &ClassMember,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    match member {
        ClassMember::Constructor(constructor) => {
            collect_jsx_usage_candidates_from_prop_name(
                parsed,
                &constructor.key,
                state,
                candidates,
            );
            let mut constructor_bindings = BTreeSet::new();
            for param in &constructor.params {
                if let swc_ecma_ast::ParamOrTsParamProp::Param(param) = param {
                    collect_pat_bindings(&param.pat, &mut constructor_bindings);
                }
            }
            state.scopes.push(constructor_bindings);
            if let Some(body) = &constructor.body {
                collect_jsx_usage_candidates_from_block(parsed, body, state, candidates);
            }
            state.scopes.pop();
        }
        ClassMember::Method(method) => {
            collect_jsx_usage_candidates_from_prop_name(parsed, &method.key, state, candidates);
            collect_jsx_usage_candidates_from_function(
                parsed,
                &method.function,
                state,
                candidates,
                None,
            );
        }
        ClassMember::PrivateMethod(method) => {
            collect_jsx_usage_candidates_from_function(
                parsed,
                &method.function,
                state,
                candidates,
                None,
            );
        }
        ClassMember::ClassProp(class_prop) => {
            collect_jsx_usage_candidates_from_prop_name(parsed, &class_prop.key, state, candidates);
            if let Some(value) = class_prop.value.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, value, state, candidates);
            }
        }
        ClassMember::PrivateProp(private_prop) => {
            if let Some(value) = private_prop.value.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, value, state, candidates);
            }
        }
        ClassMember::StaticBlock(static_block) => {
            collect_jsx_usage_candidates_from_block(parsed, &static_block.body, state, candidates);
        }
        ClassMember::AutoAccessor(accessor) => {
            collect_jsx_usage_candidates_from_key(parsed, &accessor.key, state, candidates);
            if let Some(value) = accessor.value.as_deref() {
                collect_jsx_usage_candidates_from_expr(parsed, value, state, candidates);
            }
        }
        ClassMember::TsIndexSignature(_) | ClassMember::Empty(_) => {}
    }
}

fn collect_jsx_usage_candidates_from_key(
    parsed: &ParsedReactModule,
    key: &Key,
    state: &mut UsageWalkState,
    candidates: &mut Vec<JsxUsageCandidate>,
) {
    if let Key::Public(prop_name) = key {
        collect_jsx_usage_candidates_from_prop_name(parsed, prop_name, state, candidates);
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
        registry_symbol_for_candidate(parsed, module_graph, config, registry, candidate)
    {
        let registry_package = registry
            .component_packages
            .get(&registry_symbol)
            .and_then(|package| package.as_deref());
        if registry_package.is_none() {
            return Some((registry_symbol, MatchStatus::Resolved));
        }
        return resolve_import_aware_match(registry_package, import_package.as_deref())
            .map(|match_status| (registry_symbol, match_status));
    }

    let registry_symbol =
        lookup_registry_symbol_by_name(parsed, module_graph, registry, candidate)?;
    let registry_package = registry
        .component_packages
        .get(&registry_symbol)
        .and_then(|package| package.as_deref());

    resolve_import_aware_match(registry_package, import_package.as_deref())
        .map(|match_status| (registry_symbol, match_status))
}

fn registry_symbol_for_candidate(
    parsed: &ParsedReactModule,
    module_graph: &ReactModuleGraph,
    config: &ReactScanConfig,
    registry: &ReactRegistryIndex,
    candidate: &JsxUsageCandidate,
) -> Option<String> {
    resolve_usage_registry_symbol(parsed, module_graph, config, registry, candidate)
        .or_else(|| lookup_namespace_registry_symbol(parsed, module_graph, registry, candidate))
}

fn lookup_namespace_registry_symbol(
    parsed: &ParsedReactModule,
    module_graph: &ReactModuleGraph,
    registry: &ReactRegistryIndex,
    candidate: &JsxUsageCandidate,
) -> Option<String> {
    let import = module_graph.import_binding(&parsed.file, &candidate.binding_name)?;
    if !matches!(import.imported_symbol, ImportedSymbol::Namespace) {
        return None;
    }

    let member_symbol = namespace_member_symbol(candidate)?;
    if let Some(source_module) = import.source_module.as_ref()
        && let Some(member_resolved) = module_graph.resolve_export(source_module, &member_symbol)
        && let Some(registry_symbol) = registry.resolve_targets.get(&member_resolved.symbol)
    {
        return Some(registry_symbol.clone());
    }

    registry.resolve_targets.get(&member_symbol).cloned()
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

fn binding_is_shadowed(binding_name: &str, scopes: &[BTreeSet<String>]) -> bool {
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
            id: local_definition_id(&module_identity_for_file(&file), &symbol),
            symbol: symbol.clone(),
            qualified_symbol: Some(qualified_component_symbol(
                &module_identity_for_file(&file),
                &symbol,
            )),
            identity_basis: Some("module_path_and_symbol".to_owned()),
            identity_stability: Some(IdentityStability::PathSensitive),
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
    use wax_contract::MatchStatus;

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
            vec![
                "react:component:src/zzz#Alpha",
                "react:component:src/aaa#Zeta",
            ]
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

        assert_eq!(extraction.usage_sites.len(), 3);
        assert_eq!(
            extraction
                .usage_sites
                .iter()
                .filter(|site| site.match_status == MatchStatus::Unresolved)
                .count(),
            2
        );
        assert!(
            extraction.usage_sites.iter().any(|site| {
                site.symbol == "LocalCard" && site.match_status == MatchStatus::Local
            })
        );
        assert!(extraction.diagnostics.is_empty());
        assert!(
            extraction
                .usage_sites
                .iter()
                .any(|site| site.symbol == "Button")
        );
        assert!(
            extraction
                .usage_sites
                .iter()
                .any(|site| site.symbol == "PrimaryButton")
        );
        assert!(
            extraction
                .usage_sites
                .iter()
                .all(|site| site.symbol != "External")
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

        assert_eq!(extraction.usage_sites.len(), 1);
        assert_eq!(extraction.usage_sites[0].match_status, MatchStatus::Local);
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
        fixture.write(
            "src/LocalButton.tsx",
            "export const Button = () => <button />;",
        );

        let extraction = fixture.extract_usage(
            vec!["src/App.tsx", "src/LocalButton.tsx"],
            base_config(),
            registry_with_aliases(&[("Button", &[])]),
        );

        assert_eq!(extraction.usage_sites.len(), 1);
        assert_eq!(extraction.usage_sites[0].match_status, MatchStatus::Local);
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
            design_system_tokens: Vec::new(),
            token_index: Default::default(),
        }
    }

    fn registry_with_package(symbol: &str, package: &str) -> ReactRegistryIndex {
        ReactRegistryIndex {
            design_system_components: Vec::new(),
            resolve_targets: BTreeMap::from([(symbol.to_owned(), symbol.to_owned())]),
            component_packages: BTreeMap::from([(symbol.to_owned(), Some(package.to_owned()))]),
            design_system_tokens: Vec::new(),
            token_index: Default::default(),
        }
    }

    fn registry_with_primary_token() -> ReactRegistryIndex {
        let tokens = vec![wax_contract::DesignSystemToken {
            id: "color.primary".to_owned(),
            key: "theme.colors.primary".to_owned(),
            category: wax_contract::TokenCategory::Color,
            aliases: vec!["tokens.color.primary".to_owned()],
        }];
        let token_index = wax_lang_api::token_index(&tokens).expect("token index");
        ReactRegistryIndex {
            design_system_components: Vec::new(),
            resolve_targets: BTreeMap::new(),
            component_packages: BTreeMap::new(),
            design_system_tokens: tokens,
            token_index,
        }
    }

    #[test]
    fn token_parent_uses_smallest_containing_component_span() {
        let fixture = Fixture::new();
        fixture.write(
            "src/Screen.tsx",
            r#"
            export const Outer = () => {
                function Inner() {
                    const color = theme.colors.primary;
                    return <div style={{ padding: 8 }}>{color}</div>;
                }
                return <Inner />;
            };
            const after = theme.colors.primary;
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/Screen.tsx"],
            base_config(),
            registry_with_primary_token(),
        );

        let inner_token = extraction
            .token_sites
            .iter()
            .find(|site| {
                site.parent
                    .as_ref()
                    .is_some_and(|parent| parent.symbol == "Inner")
            })
            .unwrap_or_else(|| {
                panic!(
                    "expected Inner token parent, got {:?}",
                    extraction.token_sites
                )
            });
        assert_eq!(
            inner_token
                .parent
                .as_ref()
                .map(|parent| parent.symbol.as_str()),
            Some("Inner")
        );
        let style = extraction
            .hardcoded_style_sites
            .iter()
            .find(|site| site.category == wax_contract::TokenCategory::Spacing)
            .expect("padding style");
        assert_eq!(
            style.parent.as_ref().map(|parent| parent.symbol.as_str()),
            Some("Inner")
        );
        let module_token = extraction
            .token_sites
            .iter()
            .find(|site| site.parent.is_none())
            .unwrap_or_else(|| {
                panic!(
                    "expected module-level token without parent, got {:?}",
                    extraction.token_sites
                )
            });
        assert!(module_token.parent.is_none());
        assert_eq!(extraction.token_sites.len(), 2);
    }

    #[test]
    fn arrow_intrinsic_style_gets_component_parent() {
        let fixture = Fixture::new();
        fixture.write(
            "src/Card.tsx",
            r##"
            export const Card = () => <div style={{ color: "#336699" }} />;
            "##,
        );

        let extraction = fixture.extract_usage(
            vec!["src/Card.tsx"],
            base_config(),
            registry_with_primary_token(),
        );

        let color = extraction
            .hardcoded_style_sites
            .iter()
            .find(|site| site.category == wax_contract::TokenCategory::Color)
            .expect("color style");
        assert_eq!(
            color.parent.as_ref().map(|parent| parent.symbol.as_str()),
            Some("Card")
        );
    }

    #[test]
    fn token_extraction_ignores_comments_and_partial_identifiers() {
        let fixture = Fixture::new();
        fixture.write(
            "src/Tokens.tsx",
            r#"
            // theme.colors.primary should not count
            export const Screen = () => {
                const action = theme.colors.primaryAction;
                const color = theme.colors.primary;
                return <span>{action}{color}</span>;
            };
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/Tokens.tsx"],
            base_config(),
            registry_with_primary_token(),
        );

        assert_eq!(extraction.token_sites.len(), 1);
        assert_eq!(extraction.token_sites[0].key, "theme.colors.primary");
        assert!(extraction.token_sites[0].parent.is_some());
    }

    #[test]
    fn ts_wrapped_and_quoted_style_keys_emit_candidates() {
        let fixture = Fixture::new();
        fixture.write(
            "src/Styled.tsx",
            r##"
            export const Styled = () => (
                <div
                    style={{ padding: 8 } as React.CSSProperties}
                />
            );
            export const Quoted = () => (
                <div style={{ "color": "#fff" }} />
            );
            "##,
        );

        let extraction = fixture.extract_usage(
            vec!["src/Styled.tsx"],
            base_config(),
            registry_with_primary_token(),
        );

        assert!(
            extraction
                .hardcoded_style_sites
                .iter()
                .any(|site| site.category == wax_contract::TokenCategory::Spacing
                    && site.value == "8")
        );
        assert!(
            extraction
                .hardcoded_style_sites
                .iter()
                .any(|site| site.category == wax_contract::TokenCategory::Color
                    && site.value == "\"#fff\"")
        );
    }

    #[test]
    fn non_ds_import_is_not_counted_when_registry_package_is_set() {
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

        let extraction = fixture.extract_usage(
            vec!["src/Screen.tsx"],
            base_config(),
            registry_with_package("Button", "@acme/design-system"),
        );

        assert!(extraction.usage_sites.is_empty());
    }

    #[test]
    fn namespace_non_ds_import_is_not_counted_when_registry_package_is_set() {
        let fixture = Fixture::new();
        fixture.write(
            "src/Screen.tsx",
            r#"
            import * as Foundation from "@foundation/ui";

            export function Screen() {
                return <Foundation.Button />;
            }
            "#,
        );

        let extraction = fixture.extract_usage(
            vec!["src/Screen.tsx"],
            base_config(),
            registry_with_package("Button", "@acme/design-system"),
        );

        assert!(extraction.usage_sites.is_empty());
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
            let local_components = discover_local_components(&parsed_modules);
            collect_usage_sites(
                &parsed_modules,
                &graph_build.graph,
                &config,
                &registry,
                &local_components,
            )
        }
    }
}
