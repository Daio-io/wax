//! Source-backed React registry enrichment.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::Map;
use swc_common::{Span, Spanned, comments::CommentKind};
use swc_ecma_ast::{
    Decl, DefaultDecl, ExportSpecifier, Expr, Id, ImportSpecifier, ModuleDecl, ModuleExportName,
    ModuleItem, ObjectPat, ObjectPatProp, Pat, Stmt, TsEntityName, TsEnumMemberId,
    TsKeywordTypeKind, TsLit, TsType, TsTypeElement, TsUnionOrIntersectionType,
};
use wax_contract::{
    RegistryAttachment, RegistryBestPractice, RegistryDeprecation, RegistryDetails,
    RegistryDocument, RegistryExtractionStatus, RegistryFieldGap, RegistryParameter,
    RegistryParameterTypeKind, RegistrySignature, RegistrySourceEvidence,
};
use wax_lang_api::{DiscoverRegistryRequest, DiscoverRegistryResult};

use crate::component_detect::{LocalValue, module_export_name, simple_binding_ident};
use crate::discover::{
    DamagedReactSources, ParsedReactSourceFile, ReactDiscoverError, ReactWrapperCallees,
    collect_and_parse_react_sources, is_declaration_file, react_wrapper_callees,
};
use crate::module_graph::{normalize_relative_path, resolve_path_candidates};
use crate::palette_discovery::{apply_token_evidence, discover_palette_tokens};
use crate::public_exports::{PublicBinding, public_bindings, relative_module_path};
use crate::swc_parse::ParsedReactModule;

#[derive(Debug)]
struct PropsDefinition {
    parameters: Vec<RegistryParameter>,
    inherited: bool,
}

#[derive(Debug)]
struct ComponentDeclaration {
    fallback_symbol: Option<String>,
    details: RegistryDetails,
}

struct SourceDeclarations<'a> {
    package: Option<&'a str>,
    by_name: BTreeMap<String, ComponentDeclaration>,
}

#[derive(Debug, Default)]
struct JsDoc {
    documentation: Option<String>,
    deprecation: Option<RegistryDeprecation>,
    example: Option<String>,
    best_practices: Vec<RegistryBestPractice>,
}

/// Enriches an existing registry document with facts proven by React source declarations.
///
/// Creator-owned fields, component and token identities, and canonical token values are
/// preserved. Syntax failures are returned as diagnostics and never cause generated facts
/// from the input document to be removed.
///
/// # Errors
///
/// Returns [`ReactDiscoverError::InvalidLanguageId`] for non-React requests,
/// [`ReactDiscoverError::MissingRoot`] for nonexistent roots, or
/// [`ReactDiscoverError::InvalidRegistry`] for contract-invalid registry data, or
/// [`ReactDiscoverError::Io`] when source discovery or reading fails.
pub fn discover_registry_details(
    request: &DiscoverRegistryRequest,
) -> Result<DiscoverRegistryResult, ReactDiscoverError> {
    if request.language_id.as_str() != "react" {
        return Err(ReactDiscoverError::InvalidLanguageId(
            request.language_id.to_string(),
        ));
    }

    let repo_root = Path::new(&request.repo_root);
    let absolute_roots: Vec<_> = request
        .roots
        .iter()
        .map(|root| repo_root.join(root))
        .collect();
    let mut diagnostics = Vec::new();
    let (mut parsed_files, mut damaged_paths) =
        collect_and_parse_react_sources(repo_root, &absolute_roots, true, &mut diagnostics)?;
    let relative_roots: Vec<_> = absolute_roots
        .iter()
        .filter_map(|root| normalize_relative_path(root.strip_prefix(repo_root).ok()?))
        .collect();
    let damaged_modules: BTreeSet<_> = damaged_paths.keys().map(PathBuf::from).collect();
    let known_files = parsed_files
        .iter()
        .map(|file| file.parsed.file.clone())
        .chain(damaged_modules.iter().cloned())
        .filter(|path| relative_roots.iter().any(|root| path.starts_with(root)))
        .collect();
    let mut enums_by_file: BTreeMap<_, _> = parsed_files
        .iter()
        .map(|file| {
            (
                file.parsed.file.clone(),
                collect_local_enum_values(&file.parsed),
            )
        })
        .collect();
    let exported_enums = collect_exported_enum_values(&parsed_files, &enums_by_file, &known_files);
    // Declaration files supply prop types without participating in value discovery.
    parsed_files.retain(|file| !is_declaration_file(&file.parsed.file));
    damaged_paths.retain(|path, _| !is_declaration_file(Path::new(path)));

    let exports = public_bindings(
        repo_root,
        &absolute_roots,
        &parsed_files,
        &damaged_paths,
        &mut diagnostics,
    );
    let bindings = &exports.bindings;
    let mut wanted = BTreeMap::<_, BTreeSet<_>>::new();
    for binding in bindings
        .iter()
        .filter(|binding| binding.component_symbol.is_some())
    {
        wanted
            .entry(binding.declaration_path)
            .or_default()
            .insert(binding.declaration_name.as_str());
    }
    let declarations = parsed_files
        .iter()
        .map(|file| {
            let path = file.parsed.file.as_path();
            let mut enum_values = enums_by_file.remove(path).unwrap_or_default();
            let imports = collect_imports(
                &file.parsed,
                &relative_roots,
                &known_files,
                &damaged_modules,
                &exported_enums,
                &mut enum_values,
            );
            (
                path,
                SourceDeclarations {
                    package: file.package.as_ref().map(|package| package.name.as_str()),
                    by_name: component_declarations(
                        &file.parsed,
                        wanted.get(path),
                        &imports,
                        &enum_values,
                    ),
                },
            )
        })
        .collect();

    let mut document = request.registry.clone();
    enrich_components(&mut document, bindings, &declarations, &damaged_paths);
    enrich_explicit_tokens(&mut document, repo_root, &damaged_paths);
    discover_palette_tokens(
        &mut document,
        &request.registry.tokens,
        &exports,
        &parsed_files,
        &damaged_paths,
    )?;
    document
        .validate()
        .map_err(ReactDiscoverError::InvalidRegistry)?;

    Ok(DiscoverRegistryResult {
        document,
        diagnostics,
    })
}

#[derive(Debug, Clone, Copy)]
struct ComponentImpl<'a> {
    signature_span: Option<Span>,
    parameter: Option<&'a Pat>,
}

fn component_declarations(
    parsed: &ParsedReactModule,
    public_names: Option<&BTreeSet<&str>>,
    imports: &ImportPackages,
    enum_values: &EnumValues,
) -> BTreeMap<String, ComponentDeclaration> {
    let props = collect_props_definitions(parsed, imports, enum_values);
    let wrapper_callees = react_wrapper_callees(&parsed.module.body);
    let locals = local_component_impls(parsed);
    let named_exports = local_named_value_exports(&parsed.module.body);
    let mut declarations = BTreeMap::new();
    let mut record = |name: String, fallback_symbol, span, implementation: ComponentImpl<'_>| {
        declarations.insert(
            name,
            ComponentDeclaration {
                fallback_symbol,
                details: component_details(
                    parsed,
                    span,
                    implementation,
                    &props,
                    imports,
                    enum_values,
                ),
            },
        );
    };
    let is_public = |name: &str| public_names.is_some_and(|names| names.contains(name));

    for item in &parsed.module.body {
        let declaration = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => {
                Some((&export.decl, export.span, true))
            }
            ModuleItem::Stmt(Stmt::Decl(decl)) => Some((decl, decl.span(), false)),
            _ => None,
        };
        if let Some((decl, span, directly_exported)) = declaration {
            let is_direct = |symbol: &str| directly_exported || named_exports.contains(symbol);
            match decl {
                Decl::Fn(function)
                    if (directly_exported
                        || (!function.declare && function.function.body.is_some()))
                        && (is_direct(function.ident.sym.as_ref())
                            || is_public(function.ident.sym.as_ref())) =>
                {
                    let name = function.ident.sym.as_ref();
                    record(
                        name.to_owned(),
                        is_direct(name).then(|| name.to_owned()),
                        span,
                        ComponentImpl {
                            signature_span: Some(signature_span_for_function(
                                span,
                                &function.function,
                            )),
                            parameter: function.function.params.first().map(|param| &param.pat),
                        },
                    );
                }
                Decl::Class(class) if is_public(class.ident.sym.as_ref()) => {
                    record(
                        class.ident.sym.to_string(),
                        is_direct(class.ident.sym.as_ref()).then(|| class.ident.sym.to_string()),
                        span,
                        class_implementation(span, &class.class),
                    );
                }
                Decl::Var(variable) if directly_exported || !variable.declare => {
                    for declarator in &variable.decls {
                        let Some((name, _)) = simple_binding_ident(declarator) else {
                            continue;
                        };
                        if !is_direct(&name) && !is_public(&name) {
                            continue;
                        }
                        let Some(init) = declarator.init.as_deref() else {
                            continue;
                        };
                        let span = if !directly_exported && variable.decls.len() > 1 {
                            declarator.span
                        } else {
                            span
                        };
                        if let Some(implementation) =
                            component_impl_from_expr(init, span, &locals, &wrapper_callees)
                        {
                            let fallback = is_direct(&name).then(|| name.clone());
                            record(name, fallback, span, implementation);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
                let span = default_decl.span;
                match &default_decl.decl {
                    DefaultDecl::Fn(function) => {
                        let fallback = function.ident.as_ref().map(|ident| ident.sym.to_string());
                        let name = fallback.as_deref().unwrap_or("default");
                        if fallback.is_some() || is_public(name) {
                            record(
                                name.to_owned(),
                                fallback,
                                default_decl.span,
                                ComponentImpl {
                                    signature_span: Some(signature_span_for_function(
                                        default_decl.span,
                                        &function.function,
                                    )),
                                    parameter: function
                                        .function
                                        .params
                                        .first()
                                        .map(|param| &param.pat),
                                },
                            );
                        }
                    }
                    DefaultDecl::Class(class) => {
                        let fallback = class.ident.as_ref().map(|ident| ident.sym.to_string());
                        let name = fallback.as_deref().unwrap_or("default");
                        if is_public(name) {
                            record(
                                name.to_owned(),
                                fallback,
                                span,
                                class_implementation(span, &class.class),
                            );
                        }
                    }
                    DefaultDecl::TsInterfaceDecl(_) => {}
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr))
                if !matches!(&*default_expr.expr, Expr::Ident(_)) =>
            {
                let span = default_expr.span;
                if let Some((fallback, implementation)) =
                    react_wrapper_component(&default_expr.expr, span, &locals, &wrapper_callees)
                {
                    if fallback.is_some() || is_public("default") {
                        record("default".to_owned(), fallback, span, implementation);
                    }
                } else if is_public("default")
                    && let Some(implementation) =
                        inline_function_component(&default_expr.expr, span)
                {
                    record("default".to_owned(), None, span, implementation);
                }
            }
            _ => {}
        }
    }
    declarations
}

fn local_named_value_exports(items: &[ModuleItem]) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    for item in items {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export))
                if export.src.is_none() && !export.type_only =>
            {
                for specifier in &export.specifiers {
                    if let ExportSpecifier::Named(named) = specifier
                        && !named.is_type_only
                        && named.exported.is_none()
                        && let ModuleExportName::Ident(ident) = &named.orig
                    {
                        names.insert(ident.sym.as_ref());
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(export)) => {
                if let Expr::Ident(ident) = &*export.expr {
                    names.insert(ident.sym.as_ref());
                }
            }
            _ => {}
        }
    }
    names
}

fn local_component_impls(parsed: &ParsedReactModule) -> BTreeMap<String, ComponentImpl<'_>> {
    let mut locals = BTreeMap::new();
    for item in &parsed.module.body {
        let decl = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => Some(&export.decl),
            ModuleItem::Stmt(Stmt::Decl(decl)) => Some(decl),
            _ => None,
        };
        match decl {
            Some(Decl::Fn(function)) => {
                let parameter = function.function.params.first().map(|param| &param.pat);
                locals.insert(
                    function.ident.sym.to_string(),
                    ComponentImpl {
                        signature_span: Some(signature_span_for_function(
                            function.function.span,
                            &function.function,
                        )),
                        parameter,
                    },
                );
            }
            Some(Decl::Var(variable)) => {
                for declarator in &variable.decls {
                    let Some((name, _)) = simple_binding_ident(declarator) else {
                        continue;
                    };
                    let Some(init) = declarator.init.as_deref() else {
                        continue;
                    };
                    if let Some(implementation) = direct_function_impl(init) {
                        locals.insert(name, implementation);
                    }
                }
            }
            _ => {}
        }
    }
    locals
}

fn component_impl_from_expr<'a>(
    expr: &'a Expr,
    export_span: Span,
    locals: &BTreeMap<String, ComponentImpl<'a>>,
    wrapper_callees: &ReactWrapperCallees<'a>,
) -> Option<ComponentImpl<'a>> {
    if let Some(implementation) = direct_function_impl(expr) {
        return Some(ComponentImpl {
            signature_span: implementation
                .signature_span
                .map(|span| Span::new(export_span.lo, span.hi)),
            parameter: implementation.parameter,
        });
    }
    if let Some((_, implementation)) =
        react_wrapper_component(expr, export_span, locals, wrapper_callees)
    {
        return Some(implementation);
    }
    let parameter = match wrapper_callees.factory_component(expr)? {
        LocalValue::Function(function) => function.params.first().map(|param| &param.pat),
        LocalValue::Constant(callback) => direct_function_impl(callback)?.parameter,
        LocalValue::Class(_) => None,
    };
    Some(ComponentImpl {
        signature_span: None,
        parameter,
    })
}

/// Unwraps `memo` / `forwardRef` (including nested wrappers) to the inner component.
///
/// When the inner value is a named local or named function expression, the first
/// tuple element is `Some(name)`. Anonymous inline functions/arrows return `None`.
fn react_wrapper_component<'a>(
    expr: &'a Expr,
    export_span: Span,
    locals: &BTreeMap<String, ComponentImpl<'a>>,
    wrapper_callees: &ReactWrapperCallees<'a>,
) -> Option<(Option<String>, ComponentImpl<'a>)> {
    match wrapper_callees.wrapper_target(expr)? {
        Expr::Ident(ident) => locals
            .get(ident.sym.as_ref())
            .copied()
            .map(|implementation| (Some(ident.sym.to_string()), implementation)),
        callback => named_inline_function_component(callback, export_span).or_else(|| {
            inline_function_component(callback, export_span)
                .map(|implementation| (None, implementation))
        }),
    }
}

fn named_inline_function_component<'a>(
    expr: &'a Expr,
    export_span: Span,
) -> Option<(Option<String>, ComponentImpl<'a>)> {
    let Expr::Fn(function) = expr else {
        return None;
    };
    let symbol = function.ident.as_ref()?.sym.to_string();
    let parameter = function.function.params.first().map(|param| &param.pat);
    Some((
        Some(symbol),
        ComponentImpl {
            signature_span: Some(signature_span_for_function(export_span, &function.function)),
            parameter,
        },
    ))
}

fn inline_function_component<'a>(expr: &'a Expr, export_span: Span) -> Option<ComponentImpl<'a>> {
    match expr {
        Expr::Fn(function) => {
            let parameter = function.function.params.first().map(|param| &param.pat);
            Some(ComponentImpl {
                signature_span: Some(signature_span_for_function(export_span, &function.function)),
                parameter,
            })
        }
        Expr::Arrow(arrow) => Some(ComponentImpl {
            signature_span: Some(Span::new(export_span.lo, arrow.body.span().lo)),
            parameter: arrow.params.first(),
        }),
        _ => None,
    }
}

fn direct_function_impl(expr: &Expr) -> Option<ComponentImpl<'_>> {
    match expr {
        Expr::Arrow(arrow) => Some(ComponentImpl {
            signature_span: Some(Span::new(arrow.span().lo, arrow.body.span().lo)),
            parameter: arrow.params.first(),
        }),
        Expr::Fn(function) => {
            let parameter = function.function.params.first().map(|param| &param.pat);
            Some(ComponentImpl {
                signature_span: Some(signature_span_for_function(
                    function.function.span,
                    &function.function,
                )),
                parameter,
            })
        }
        _ => None,
    }
}

fn signature_span_for_function(span: Span, function: &swc_ecma_ast::Function) -> Span {
    Span::new(
        span.lo,
        function.body.as_ref().map_or(span.hi, |body| body.span.lo),
    )
}

fn class_implementation(span: Span, class: &swc_ecma_ast::Class) -> ComponentImpl<'_> {
    ComponentImpl {
        signature_span: Some(Span::new(
            span.lo,
            class
                .body
                .first()
                .map_or(span.hi, |member| member.span().lo),
        )),
        parameter: None,
    }
}

type ImportPackages = HashMap<Id, Option<String>>;

fn collect_imports(
    parsed: &ParsedReactModule,
    roots: &[PathBuf],
    known_files: &BTreeSet<PathBuf>,
    damaged_modules: &BTreeSet<PathBuf>,
    exported: &ExportedEnumValues,
    enum_values: &mut EnumValues,
) -> ImportPackages {
    let mut imports = HashMap::new();
    for item in &parsed.module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
            continue;
        };
        let source = import.src.value.to_string_lossy();
        let relative_path = relative_module_path(&parsed.file, &source);
        let within_roots = relative_path
            .as_ref()
            .is_some_and(|path| roots.iter().any(|root| path.starts_with(root)));
        let target = relative_path
            .as_deref()
            .and_then(|path| resolve_enum_module(path, known_files));
        let damaged = target
            .as_ref()
            .is_some_and(|path| damaged_modules.contains(path));
        // Preserve the existing opaque parameter when the target could not be parsed.
        let package = (!within_roots || damaged).then(|| source.into_owned());
        let target = target.map(|path| path.to_string_lossy().into_owned());
        for specifier in &import.specifiers {
            let local = match specifier {
                ImportSpecifier::Default(specifier) => &specifier.local,
                ImportSpecifier::Namespace(specifier) => &specifier.local,
                ImportSpecifier::Named(named) => {
                    if let Some(target) = &target {
                        let name = named
                            .imported
                            .as_ref()
                            .map_or_else(|| named.local.sym.to_string(), module_export_name);
                        if let Some(values) = exported.get(&(target.clone(), name)) {
                            enum_values.insert(named.local.to_id(), values.clone());
                        }
                    }
                    &named.local
                }
            };
            imports.insert(local.to_id(), package.clone());
        }
    }
    imports
}

fn module_declarations(parsed: &ParsedReactModule) -> impl Iterator<Item = &Decl> {
    parsed.module.body.iter().filter_map(|item| match item {
        ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => Some(&export.decl),
        ModuleItem::Stmt(Stmt::Decl(decl)) => Some(decl),
        _ => None,
    })
}

type EnumValues = BTreeMap<Id, Vec<String>>;
type ExportedEnumValues = BTreeMap<(String, String), Vec<String>>;

fn collect_local_enum_values(parsed: &ParsedReactModule) -> EnumValues {
    let mut enums = EnumValues::new();
    for decl in module_declarations(parsed) {
        let (id, values) = match decl {
            Decl::TsTypeAlias(alias) => {
                let Some(values) = literal_union_values(parsed, &alias.type_ann) else {
                    continue;
                };
                (alias.id.to_id(), values)
            }
            Decl::TsEnum(enumeration) => {
                let values = enumeration
                    .members
                    .iter()
                    .map(|member| match &member.id {
                        TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
                        TsEnumMemberId::Str(value) => value.value.to_string_lossy().into_owned(),
                    })
                    .collect();
                (enumeration.id.to_id(), values)
            }
            _ => continue,
        };
        let collected = enums.entry(id).or_default();
        for value in values {
            if !collected.contains(&value) {
                collected.push(value);
            }
        }
    }
    enums
}

fn collect_exported_enum_values(
    files: &[ParsedReactSourceFile],
    enums_by_file: &BTreeMap<PathBuf, EnumValues>,
    known_files: &BTreeSet<PathBuf>,
) -> ExportedEnumValues {
    let mut exported = BTreeMap::new();
    for file in files {
        let parsed = &file.parsed;
        let Some(local) = enums_by_file
            .get(&parsed.file)
            .filter(|local| !local.is_empty())
        else {
            continue;
        };
        let path = parsed.file.to_string_lossy();
        for item in &parsed.module.body {
            match item {
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => {
                    let id = match &export.decl {
                        Decl::TsTypeAlias(alias) => alias.id.to_id(),
                        Decl::TsEnum(enumeration) => enumeration.id.to_id(),
                        _ => continue,
                    };
                    if let Some(values) = local.get(&id) {
                        exported.insert((path.to_string(), id.0.to_string()), values.clone());
                    }
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export)) if export.src.is_none() => {
                    for specifier in &export.specifiers {
                        let ExportSpecifier::Named(named) = specifier else {
                            continue;
                        };
                        let ModuleExportName::Ident(ident) = &named.orig else {
                            continue;
                        };
                        if let Some(values) = local.get(&ident.to_id()) {
                            let name =
                                module_export_name(named.exported.as_ref().unwrap_or(&named.orig));
                            exported.insert((path.to_string(), name), values.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut reexports = BTreeMap::new();
    for file in files {
        let parsed = &file.parsed;
        for item in &parsed.module.body {
            let ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export)) = item else {
                continue;
            };
            let Some(source) = &export.src else {
                continue;
            };
            let Some(target) = relative_module_path(&parsed.file, &source.value.to_string_lossy())
                .and_then(|path| resolve_enum_module(&path, known_files))
            else {
                continue;
            };
            let target = target.to_string_lossy().into_owned();
            for specifier in &export.specifiers {
                let ExportSpecifier::Named(named) = specifier else {
                    continue;
                };
                let original = module_export_name(&named.orig);
                if let Some(values) = exported.get(&(target.clone(), original)) {
                    let name = module_export_name(named.exported.as_ref().unwrap_or(&named.orig));
                    reexports.insert(
                        (parsed.file.to_string_lossy().into_owned(), name),
                        values.clone(),
                    );
                }
            }
        }
    }
    // Read re-exports from the direct index so a second forwarding module stays opaque.
    exported.extend(reexports);
    exported
}

fn resolve_enum_module(path: &Path, known_files: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        let substituted_extensions = match extension {
            "js" => Some(["ts", "tsx", "d.ts"].as_slice()),
            "jsx" => Some(["tsx", "ts", "d.ts"].as_slice()),
            _ => None,
        };
        if let Some(extensions) = substituted_extensions {
            for extension in extensions {
                let candidate = path.with_extension(extension);
                if known_files.contains(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    resolve_path_candidates(path, known_files, &[".d.ts", ".ts", ".tsx", ".js", ".jsx"])
}

fn collect_props_definitions(
    parsed: &ParsedReactModule,
    imports: &ImportPackages,
    enum_values: &EnumValues,
) -> HashMap<Id, PropsDefinition> {
    module_declarations(parsed)
        .filter_map(|decl| {
            let (id, members, inherited) = match decl {
                Decl::TsInterface(interface) => (
                    &interface.id,
                    &interface.body.body,
                    !interface.extends.is_empty(),
                ),
                Decl::TsTypeAlias(alias) => {
                    let TsType::TsTypeLit(literal) = &*alias.type_ann else {
                        return None;
                    };
                    (&alias.id, &literal.members, false)
                }
                _ => return None,
            };
            Some((
                id.to_id(),
                PropsDefinition {
                    parameters: parameters_from_members(parsed, members, imports, enum_values),
                    inherited,
                },
            ))
        })
        .collect()
}

fn parameters_from_members(
    parsed: &ParsedReactModule,
    members: &[TsTypeElement],
    imports: &ImportPackages,
    enum_values: &EnumValues,
) -> Vec<RegistryParameter> {
    members
        .iter()
        .filter_map(|member| {
            let TsTypeElement::TsPropertySignature(property) = member else {
                return None;
            };
            let name = match &*property.key {
                Expr::Ident(ident) => ident.sym.to_string(),
                Expr::Lit(swc_ecma_ast::Lit::Str(value)) => {
                    value.value.to_string_lossy().into_owned()
                }
                _ => return None,
            };
            let type_text = property
                .type_ann
                .as_ref()
                .and_then(|annotation| parsed.source_slice_from_span(annotation.type_ann.span()))
                .map(|text| text.trim().to_owned());
            let description = parameter_description(parsed, property.span);
            let (type_kind, allowed_values) = property.type_ann.as_ref().map_or_else(
                || (None, Vec::new()),
                |annotation| parameter_type_facts(parsed, &annotation.type_ann, enum_values),
            );
            let package = property
                .type_ann
                .as_ref()
                .and_then(|annotation| imported_type_id(&annotation.type_ann))
                .and_then(|id| imports.get(&id))
                .cloned()
                .flatten();
            Some(RegistryParameter {
                name,
                type_text,
                package,
                type_kind,
                allowed_values,
                description,
                default: None,
                required: Some(!property.optional),
                extra: Map::new(),
            })
        })
        .collect()
}

fn parameter_description(parsed: &ParsedReactModule, span: Span) -> Option<String> {
    jsdoc_for_span(parsed, span).documentation
}

fn parameter_type_facts(
    parsed: &ParsedReactModule,
    ty: &TsType,
    enum_values: &EnumValues,
) -> (Option<RegistryParameterTypeKind>, Vec<String>) {
    let kind = match ty {
        TsType::TsKeywordType(keyword) => match keyword.kind {
            TsKeywordTypeKind::TsBooleanKeyword => Some(RegistryParameterTypeKind::Boolean),
            TsKeywordTypeKind::TsStringKeyword => Some(RegistryParameterTypeKind::String),
            TsKeywordTypeKind::TsNumberKeyword => Some(RegistryParameterTypeKind::Number),
            TsKeywordTypeKind::TsObjectKeyword => Some(RegistryParameterTypeKind::Object),
            _ => None,
        },
        TsType::TsFnOrConstructorType(_) => Some(RegistryParameterTypeKind::Function),
        TsType::TsArrayType(_) | TsType::TsTupleType(_) => {
            Some(RegistryParameterTypeKind::Collection)
        }
        TsType::TsTypeLit(_) => Some(RegistryParameterTypeKind::Object),
        _ => None,
    };
    let allowed_values = match ty {
        TsType::TsTypeRef(reference) => match &reference.type_name {
            TsEntityName::Ident(ident) => enum_values.get(&ident.to_id()).cloned(),
            _ => None,
        },
        _ => literal_union_values(parsed, ty),
    };
    match allowed_values {
        // The registry requires at least one value and cannot represent empty strings.
        Some(values) if !values.is_empty() && values.iter().all(|value| !value.is_empty()) => {
            (Some(RegistryParameterTypeKind::Enum), values)
        }
        _ => (kind, Vec::new()),
    }
}

fn literal_union_values(parsed: &ParsedReactModule, ty: &TsType) -> Option<Vec<String>> {
    let TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(union)) = ty
    else {
        return None;
    };
    let mut values = Vec::new();
    for member in &union.types {
        let TsType::TsLitType(literal) = &**member else {
            return None;
        };
        let value = match &literal.lit {
            TsLit::Str(value) => value.value.to_string_lossy().into_owned(),
            TsLit::Tpl(template) if !template.types.is_empty() => return None,
            _ => parsed
                .source_slice_from_span(literal.span)?
                .trim()
                .to_owned(),
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
    Some(values)
}

fn component_details(
    parsed: &ParsedReactModule,
    span: Span,
    implementation: ComponentImpl<'_>,
    props: &HashMap<Id, PropsDefinition>,
    imports: &ImportPackages,
    enum_values: &EnumValues,
) -> RegistryDetails {
    let ComponentImpl {
        signature_span,
        parameter,
    } = implementation;
    let (props_type_text, mut parameters, mut field_gaps) =
        signature_facts(parsed, parameter, props, imports, enum_values);
    let type_text = signature_span
        .and_then(|span| parsed.source_slice_from_span(span))
        .map(|text| text.trim().to_owned());
    apply_destructured_defaults(parsed, parameter, &mut parameters);
    let jsdoc = jsdoc_for_span(parsed, span);
    let attachments = jsdoc
        .example
        .map(|content| {
            vec![RegistryAttachment {
                kind: wax_contract::RegistrySourceKind::Example,
                path: parsed.file.to_string_lossy().replace('\\', "/"),
                content,
                language: parsed
                    .file
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_owned),
                extra: Map::new(),
            }]
        })
        .unwrap_or_default();
    if signature_span.is_none() {
        field_gaps.push(RegistryFieldGap {
            field: "signature.type_text".to_owned(),
            reason: "the public signature of the factory result requires type resolution"
                .to_owned(),
            extra: Map::new(),
        });
    }
    if props_type_text.is_none() {
        field_gaps.push(RegistryFieldGap {
            field: "signature.parameters".to_owned(),
            reason: "the exported component has no direct props type annotation".to_owned(),
            extra: Map::new(),
        });
    }
    let source_evidence = parsed
        .source_lines_from_span(span)
        .map(|(start_line, end_line)| RegistrySourceEvidence {
            path: parsed.file.to_string_lossy().replace('\\', "/"),
            start_line: Some(start_line),
            end_line: Some(end_line),
            extra: Map::new(),
        });
    let status = if field_gaps.is_empty() {
        RegistryExtractionStatus::Complete
    } else {
        RegistryExtractionStatus::Partial
    };

    RegistryDetails {
        status,
        source_evidence,
        documentation: jsdoc.documentation,
        signature: Some(RegistrySignature {
            type_text,
            parameters,
            extra: Map::new(),
        }),
        deprecation: jsdoc.deprecation,
        attachments,
        field_gaps,
        best_practices: jsdoc.best_practices,
        extra: Map::new(),
    }
}

fn signature_facts(
    parsed: &ParsedReactModule,
    parameter: Option<&Pat>,
    props: &HashMap<Id, PropsDefinition>,
    imports: &ImportPackages,
    enum_values: &EnumValues,
) -> (
    Option<String>,
    Vec<RegistryParameter>,
    Vec<RegistryFieldGap>,
) {
    let Some(parameter) = parameter else {
        return (None, Vec::new(), Vec::new());
    };
    let type_annotation = pat_type_annotation(parameter);
    let type_text = type_annotation
        .and_then(|annotation| parsed.source_slice_from_span(annotation.type_ann.span()))
        .map(|text| text.trim().to_owned());
    let Some(annotation) = type_annotation else {
        return (type_text, Vec::new(), Vec::new());
    };
    let ident = match &*annotation.type_ann {
        TsType::TsTypeLit(literal) => {
            return (
                type_text,
                parameters_from_members(parsed, &literal.members, imports, enum_values),
                Vec::new(),
            );
        }
        TsType::TsTypeRef(reference) => match &reference.type_name {
            swc_ecma_ast::TsEntityName::Ident(ident) => ident,
            swc_ecma_ast::TsEntityName::TsQualifiedName(qualified) => {
                let package = match &qualified.left {
                    swc_ecma_ast::TsEntityName::Ident(ident) => {
                        imports.get(&ident.to_id()).cloned().flatten()
                    }
                    swc_ecma_ast::TsEntityName::TsQualifiedName(_) => None,
                };
                return imported_props_parameters(parameter, type_text, package);
            }
        },
        _ => {
            return (
                type_text,
                Vec::new(),
                vec![RegistryFieldGap {
                    field: "signature.parameters".to_owned(),
                    reason: "props type shape is not a local type reference or inline object type"
                        .to_owned(),
                    extra: Map::new(),
                }],
            );
        }
    };
    let type_name = &ident.sym;
    let id = ident.to_id();
    let Some(definition) = props.get(&id) else {
        if let Some(package) = imports.get(&id) {
            return imported_props_parameters(parameter, type_text, package.clone());
        }
        return (
            type_text,
            Vec::new(),
            vec![RegistryFieldGap {
                field: "signature.parameters".to_owned(),
                reason: format!("props type `{type_name}` is imported or unavailable locally"),
                extra: Map::new(),
            }],
        );
    };
    let mut parameters = definition.parameters.clone();
    let gaps = if definition.inherited {
        for parameter in &mut parameters {
            parameter.type_kind = None;
            parameter.allowed_values.clear();
        }
        vec![RegistryFieldGap {
            field: "signature.parameters".to_owned(),
            reason: format!(
                "props type `{type_name}` inherits fields that require type resolution"
            ),
            extra: Map::new(),
        }]
    } else {
        Vec::new()
    };
    (type_text, parameters, gaps)
}

fn imported_props_parameters(
    parameter: &Pat,
    type_text: Option<String>,
    package: Option<String>,
) -> (
    Option<String>,
    Vec<RegistryParameter>,
    Vec<RegistryFieldGap>,
) {
    let parameters = match parameter {
        Pat::Object(_) => vec![RegistryParameter {
            name: "props".to_owned(),
            type_text: type_text.clone(),
            package,
            type_kind: None,
            allowed_values: Vec::new(),
            description: None,
            default: None,
            required: Some(true),
            extra: Map::new(),
        }],
        _ => pat_name(parameter)
            .into_iter()
            .map(|name| RegistryParameter {
                name: name.to_owned(),
                type_text: type_text.clone(),
                package: package.clone(),
                type_kind: None,
                allowed_values: Vec::new(),
                description: None,
                default: None,
                required: Some(true),
                extra: Map::new(),
            })
            .collect(),
    };
    (type_text, parameters, Vec::new())
}

fn imported_type_id(ty: &TsType) -> Option<Id> {
    let TsType::TsTypeRef(reference) = ty else {
        return None;
    };
    match &reference.type_name {
        swc_ecma_ast::TsEntityName::Ident(ident) => Some(ident.to_id()),
        swc_ecma_ast::TsEntityName::TsQualifiedName(_) => None,
    }
}

fn pat_name(pattern: &Pat) -> Option<&str> {
    match pattern {
        Pat::Ident(ident) => Some(ident.id.sym.as_ref()),
        Pat::Assign(assign) => pat_name(&assign.left),
        _ => None,
    }
}

fn pat_type_annotation(pattern: &Pat) -> Option<&swc_ecma_ast::TsTypeAnn> {
    match pattern {
        Pat::Ident(ident) => ident.type_ann.as_deref(),
        Pat::Array(array) => array.type_ann.as_deref(),
        Pat::Object(object) => object.type_ann.as_deref(),
        Pat::Rest(rest) => rest.type_ann.as_deref(),
        Pat::Assign(assign) => pat_type_annotation(&assign.left),
        Pat::Invalid(_) | Pat::Expr(_) => None,
    }
}

fn apply_destructured_defaults(
    parsed: &ParsedReactModule,
    parameter: Option<&Pat>,
    parameters: &mut [RegistryParameter],
) {
    let Some(Pat::Object(object)) = parameter else {
        return;
    };
    for (name, span) in object_defaults(object) {
        let Some(default) = parsed.source_slice_from_span(span) else {
            continue;
        };
        if let Some(parameter) = parameters
            .iter_mut()
            .find(|parameter| parameter.name == name)
        {
            parameter.default = Some(default.trim().to_owned());
            parameter.required = Some(false);
        }
    }
}

fn object_defaults(object: &ObjectPat) -> Vec<(String, Span)> {
    object
        .props
        .iter()
        .filter_map(|property| match property {
            ObjectPatProp::Assign(assign) => assign
                .value
                .as_ref()
                .map(|value| (assign.key.id.sym.to_string(), value.span())),
            ObjectPatProp::KeyValue(key_value) => {
                let Pat::Assign(assign) = &*key_value.value else {
                    return None;
                };
                let Pat::Ident(ident) = &*assign.left else {
                    return None;
                };
                Some((ident.id.sym.to_string(), assign.right.span()))
            }
            ObjectPatProp::Rest(_) => None,
        })
        .collect()
}

fn jsdoc_for_span(parsed: &ParsedReactModule, span: Span) -> JsDoc {
    let Some(comment) = parsed
        .leading_comments(span)
        .into_iter()
        .rev()
        .find(|comment| comment.kind == CommentKind::Block && comment.text.starts_with('*'))
    else {
        return JsDoc::default();
    };
    parse_jsdoc(comment.text.as_ref())
}

fn parse_jsdoc(text: &str) -> JsDoc {
    enum Block {
        Body,
        BodyFence,
        Example,
        ExampleFence,
        Guidance,
        Ignored,
    }

    let lines = text
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim());
    let mut description = Vec::new();
    let mut deprecation = None;
    let mut example = Vec::new();
    let mut best_practices = Vec::new();
    let mut block = Block::Body;
    for line in lines {
        if line.starts_with("```") {
            block = match block {
                Block::Example => Block::ExampleFence,
                Block::ExampleFence => Block::Example,
                Block::BodyFence => {
                    description.push(line);
                    Block::Body
                }
                _ => {
                    description.push(line);
                    Block::BodyFence
                }
            };
        } else if matches!(block, Block::ExampleFence) {
            example.push(line);
        } else if matches!(block, Block::BodyFence) {
            if !line.is_empty() {
                description.push(line);
            }
        } else if let Some(message) = line.strip_prefix("@deprecated") {
            block = Block::Ignored;
            deprecation = Some(RegistryDeprecation {
                message: non_empty(message.trim()),
                since: None,
                replaced_by: None,
                extra: Map::new(),
            });
        } else if let Some(first_line) = line.strip_prefix("@example") {
            let first_line = first_line.trim();
            block = if first_line.starts_with("```") {
                Block::ExampleFence
            } else {
                if !first_line.is_empty() {
                    example.push(first_line);
                }
                Block::Example
            };
        } else if let Some(guidance) = line.strip_prefix("@guidance") {
            block = Block::Guidance;
            let guidance = guidance.trim();
            best_practices.push(RegistryBestPractice {
                description: guidance
                    .strip_prefix('-')
                    .unwrap_or(guidance)
                    .trim()
                    .to_owned(),
                extra: Map::new(),
            });
        } else if line.starts_with('@') {
            block = Block::Ignored;
        } else if line.is_empty() {
            if matches!(block, Block::Guidance | Block::Ignored) {
                block = Block::Body;
            }
        } else {
            match block {
                Block::Body => description.push(line),
                Block::Example => example.push(line),
                Block::Guidance => {
                    if let Some(rule) = best_practices.last_mut() {
                        if !rule.description.is_empty() {
                            rule.description.push(' ');
                        }
                        rule.description.push_str(line);
                    }
                }
                Block::Ignored | Block::BodyFence | Block::ExampleFence => {}
            }
        }
    }
    best_practices.retain(|rule| !rule.description.is_empty());
    JsDoc {
        documentation: non_empty(description.join("\n").trim()),
        deprecation,
        example: non_empty(example.join("\n").trim()),
        best_practices,
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn enrich_components(
    document: &mut RegistryDocument,
    bindings: &[PublicBinding<'_>],
    declarations: &BTreeMap<&Path, SourceDeclarations<'_>>,
    damaged_paths: &DamagedReactSources,
) {
    for component in &mut document.components {
        let matches_symbol = |symbol: &str| {
            component.symbol == symbol || component.aliases.iter().any(|alias| alias == symbol)
        };
        let matches_package = |package: Option<&str>| {
            component
                .package
                .as_deref()
                .is_none_or(|expected| package == Some(expected))
        };
        let public = bindings
            .iter()
            .filter(|binding| {
                binding
                    .component_symbol
                    .as_deref()
                    .is_some_and(matches_symbol)
                    && matches_package(binding.package)
            })
            .find_map(|binding| {
                declarations
                    .get(binding.declaration_path)?
                    .by_name
                    .get(&binding.declaration_name)
            });
        let declaration = public.or_else(|| {
            let mut candidates = declarations
                .values()
                .filter(|file| matches_package(file.package))
                .flat_map(|file| file.by_name.values())
                .filter(|declaration| {
                    declaration
                        .fallback_symbol
                        .as_deref()
                        .is_some_and(matches_symbol)
                });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        });
        if let Some(declaration) = declaration {
            component.details = Some(declaration.details.clone());
            continue;
        }

        let evidence_damaged = component
            .details
            .as_ref()
            .and_then(|details| details.source_evidence.as_ref())
            .is_some_and(|evidence| damaged_paths.contains_key(&evidence.path));
        if evidence_damaged {
            if let Some(details) = &mut component.details {
                details.status = RegistryExtractionStatus::Stale;
            }
            continue;
        }

        // Unrelated parse damage is incomplete evidence: preserve prior facts instead of
        // generating removals. Only emit unavailable when the scan had no damage.
        if component.details.is_some() && !damaged_paths.is_empty() {
            continue;
        }

        component.details = Some(unavailable(
            "source_evidence",
            "no matching exported React component declaration was observed",
        ));
    }
}

fn enrich_explicit_tokens(
    document: &mut RegistryDocument,
    repo_root: &Path,
    damaged_paths: &DamagedReactSources,
) {
    for token in &mut document.tokens {
        let declaration = token.metadata.as_ref().and_then(|metadata| {
            metadata.sources.iter().find(|source| {
                source.kind == wax_contract::RegistrySourceKind::Declaration
                    && source
                        .language
                        .as_deref()
                        .is_none_or(|language| language == "react")
            })
        });
        let Some(source) = declaration else {
            continue;
        };
        if damaged_paths.contains_key(&source.path) {
            if let Some(details) = &mut token.details {
                details.status = RegistryExtractionStatus::Stale;
            }
            continue;
        }
        if !repo_root.join(&source.path).is_file() {
            continue;
        }
        let evidence = RegistrySourceEvidence {
            path: source.path.clone(),
            start_line: source.start_line,
            end_line: source.end_line,
            extra: Map::new(),
        };
        apply_token_evidence(token, evidence);
    }
}

fn unavailable(field: &str, reason: &str) -> RegistryDetails {
    RegistryDetails {
        status: RegistryExtractionStatus::Unavailable,
        source_evidence: None,
        documentation: None,
        signature: None,
        deprecation: None,
        attachments: Vec::new(),
        field_gaps: vec![RegistryFieldGap {
            field: field.to_owned(),
            reason: reason.to_owned(),
            extra: Map::new(),
        }],
        best_practices: Vec::new(),
        extra: Map::new(),
    }
}
