//! Source-backed React registry enrichment.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Map;
use swc_common::{Span, Spanned, comments::CommentKind};
use swc_ecma_ast::{
    Decl, DefaultDecl, Expr, ImportSpecifier, ModuleDecl, ModuleItem, ObjectPat, ObjectPatProp,
    Pat, Stmt, TsInterfaceDecl, TsKeywordTypeKind, TsLit, TsType, TsTypeAliasDecl, TsTypeElement,
    TsUnionOrIntersectionType,
};
use wax_contract::{
    RegistryAttachment, RegistryBestPractice, RegistryDeprecation, RegistryDetails,
    RegistryDocument, RegistryExtractionStatus, RegistryFieldGap, RegistryParameter,
    RegistryParameterTypeKind, RegistrySignature, RegistrySourceEvidence,
};
use wax_lang_api::{DiscoverRegistryRequest, DiscoverRegistryResult};

use crate::component_detect::simple_binding_ident;
use crate::discover::{
    ReactDiscoverError, ReactWrapperCallees, callee_matches, collect_and_parse_react_sources,
    react_wrapper_callees,
};
use crate::swc_parse::ParsedReactModule;

#[derive(Debug)]
struct PropsDefinition {
    parameters: Vec<RegistryParameter>,
    inherited: bool,
}

#[derive(Debug)]
struct ComponentDeclaration {
    symbol: String,
    package: Option<String>,
    details: RegistryDetails,
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
    let (parsed_files, damaged_paths) =
        collect_and_parse_react_sources(repo_root, &absolute_roots, &mut diagnostics)?;

    let mut declarations = Vec::new();
    for file in parsed_files {
        declarations.extend(component_declarations(&file.parsed, file.package));
    }

    let mut document = request.registry.clone();
    enrich_components(&mut document, &declarations, &damaged_paths);
    enrich_explicit_tokens(&mut document, repo_root, &damaged_paths);
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
    signature_span: Span,
    parameter: Option<&'a Pat>,
}

fn component_declarations(
    parsed: &ParsedReactModule,
    package: Option<String>,
) -> Vec<ComponentDeclaration> {
    let imports = collect_import_packages(parsed);
    let props = collect_props_definitions(parsed, &imports);
    let wrapper_callees = react_wrapper_callees(&parsed.module.body);
    let locals = local_component_impls(parsed);
    let mut declarations = Vec::new();

    for item in &parsed.module.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => match &export.decl {
                Decl::Fn(function) => {
                    let parameter = function.function.params.first().map(|param| &param.pat);
                    declarations.push(ComponentDeclaration {
                        symbol: function.ident.sym.to_string(),
                        package: package.clone(),
                        details: component_details(
                            parsed,
                            export.span,
                            signature_span_for_function(export.span, &function.function),
                            parameter,
                            &props,
                            &imports,
                        ),
                    });
                }
                Decl::Var(variable) => {
                    for declarator in &variable.decls {
                        let Some((symbol, _)) = simple_binding_ident(declarator) else {
                            continue;
                        };
                        let Some(init) = declarator.init.as_deref() else {
                            continue;
                        };
                        if let Some(implementation) =
                            component_impl_from_expr(init, export.span, &locals, &wrapper_callees)
                        {
                            declarations.push(ComponentDeclaration {
                                symbol,
                                package: package.clone(),
                                details: component_details(
                                    parsed,
                                    export.span,
                                    implementation.signature_span,
                                    implementation.parameter,
                                    &props,
                                    &imports,
                                ),
                            });
                        }
                    }
                }
                _ => {}
            },
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(default_decl)) => {
                if let DefaultDecl::Fn(function) = &default_decl.decl
                    && let Some(ident) = &function.ident
                {
                    let parameter = function.function.params.first().map(|param| &param.pat);
                    declarations.push(ComponentDeclaration {
                        symbol: ident.sym.to_string(),
                        package: package.clone(),
                        details: component_details(
                            parsed,
                            default_decl.span,
                            signature_span_for_function(default_decl.span, &function.function),
                            parameter,
                            &props,
                            &imports,
                        ),
                    });
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(default_expr)) => {
                match &*default_expr.expr {
                    Expr::Ident(ident) => {
                        if let Some(implementation) = locals.get(ident.sym.as_ref()) {
                            declarations.push(ComponentDeclaration {
                                symbol: ident.sym.to_string(),
                                package: package.clone(),
                                details: component_details(
                                    parsed,
                                    default_expr.span,
                                    implementation.signature_span,
                                    implementation.parameter,
                                    &props,
                                    &imports,
                                ),
                            });
                        }
                    }
                    expr => {
                        if let Some((Some(symbol), implementation)) = react_wrapper_component(
                            expr,
                            default_expr.span,
                            &locals,
                            &wrapper_callees,
                        ) {
                            declarations.push(ComponentDeclaration {
                                symbol,
                                package: package.clone(),
                                details: component_details(
                                    parsed,
                                    default_expr.span,
                                    implementation.signature_span,
                                    implementation.parameter,
                                    &props,
                                    &imports,
                                ),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    declarations
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
                        signature_span: signature_span_for_function(
                            function.function.span,
                            &function.function,
                        ),
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
    wrapper_callees: &ReactWrapperCallees,
) -> Option<ComponentImpl<'a>> {
    if let Some(implementation) = direct_function_impl(expr) {
        return Some(ComponentImpl {
            signature_span: Span::new(export_span.lo, implementation.signature_span.hi),
            parameter: implementation.parameter,
        });
    }
    react_wrapper_component(expr, export_span, locals, wrapper_callees)
        .map(|(_, implementation)| implementation)
}

/// Unwraps `memo` / `forwardRef` (including nested wrappers) to the inner component.
///
/// When the inner value is a named local or named function expression, the first
/// tuple element is `Some(name)`. Anonymous inline functions/arrows return `None`.
fn react_wrapper_component<'a>(
    expr: &'a Expr,
    export_span: Span,
    locals: &BTreeMap<String, ComponentImpl<'a>>,
    wrapper_callees: &ReactWrapperCallees,
) -> Option<(Option<String>, ComponentImpl<'a>)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    let arg = &*call.args[0].expr;

    if callee_matches(&call.callee, "memo", wrapper_callees) {
        return match arg {
            Expr::Ident(ident) => locals
                .get(ident.sym.as_ref())
                .copied()
                .map(|implementation| {
                    (
                        Some(ident.sym.to_string()),
                        ComponentImpl {
                            signature_span: implementation.signature_span,
                            parameter: implementation.parameter,
                        },
                    )
                }),
            Expr::Call(_) => react_wrapper_component(arg, export_span, locals, wrapper_callees),
            other => match named_inline_function_component(other, export_span) {
                named @ Some(_) => named,
                None => inline_function_component(other, export_span)
                    .map(|implementation| (None, implementation)),
            },
        };
    }

    if callee_matches(&call.callee, "forwardRef", wrapper_callees) {
        return match named_inline_function_component(arg, export_span) {
            named @ Some(_) => named,
            None => inline_function_component(arg, export_span)
                .map(|implementation| (None, implementation)),
        };
    }

    None
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
            signature_span: signature_span_for_function(export_span, &function.function),
            parameter,
        },
    ))
}

fn inline_function_component<'a>(expr: &'a Expr, export_span: Span) -> Option<ComponentImpl<'a>> {
    match expr {
        Expr::Fn(function) => {
            let parameter = function.function.params.first().map(|param| &param.pat);
            Some(ComponentImpl {
                signature_span: signature_span_for_function(export_span, &function.function),
                parameter,
            })
        }
        Expr::Arrow(arrow) => Some(ComponentImpl {
            signature_span: Span::new(export_span.lo, arrow.body.span().lo),
            parameter: arrow.params.first(),
        }),
        _ => None,
    }
}

fn direct_function_impl(expr: &Expr) -> Option<ComponentImpl<'_>> {
    match expr {
        Expr::Arrow(arrow) => Some(ComponentImpl {
            signature_span: Span::new(arrow.span().lo, arrow.body.span().lo),
            parameter: arrow.params.first(),
        }),
        Expr::Fn(function) => {
            let parameter = function.function.params.first().map(|param| &param.pat);
            Some(ComponentImpl {
                signature_span: signature_span_for_function(
                    function.function.span,
                    &function.function,
                ),
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

fn collect_import_packages(parsed: &ParsedReactModule) -> BTreeMap<String, String> {
    let mut imports = BTreeMap::new();
    for item in &parsed.module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
            continue;
        };
        let package = import.src.value.to_string_lossy().into_owned();
        for specifier in &import.specifiers {
            let local = match specifier {
                ImportSpecifier::Default(specifier) => &specifier.local,
                ImportSpecifier::Namespace(specifier) => &specifier.local,
                ImportSpecifier::Named(specifier) => &specifier.local,
            };
            imports.insert(local.sym.to_string(), package.clone());
        }
    }
    imports
}

fn collect_props_definitions(
    parsed: &ParsedReactModule,
    imports: &BTreeMap<String, String>,
) -> BTreeMap<String, PropsDefinition> {
    let mut definitions = BTreeMap::new();
    for item in &parsed.module.body {
        let decl = match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => Some(&export.decl),
            ModuleItem::Stmt(Stmt::Decl(decl)) => Some(decl),
            _ => None,
        };
        match decl {
            Some(Decl::TsInterface(interface)) => {
                definitions.insert(
                    interface.id.sym.to_string(),
                    props_from_interface(parsed, interface, imports),
                );
            }
            Some(Decl::TsTypeAlias(alias)) => {
                if let Some(definition) = props_from_alias(parsed, alias, imports) {
                    definitions.insert(alias.id.sym.to_string(), definition);
                }
            }
            _ => {}
        }
    }
    definitions
}

fn props_from_interface(
    parsed: &ParsedReactModule,
    interface: &TsInterfaceDecl,
    imports: &BTreeMap<String, String>,
) -> PropsDefinition {
    PropsDefinition {
        parameters: parameters_from_members(parsed, &interface.body.body, imports),
        inherited: !interface.extends.is_empty(),
    }
}

fn props_from_alias(
    parsed: &ParsedReactModule,
    alias: &TsTypeAliasDecl,
    imports: &BTreeMap<String, String>,
) -> Option<PropsDefinition> {
    let TsType::TsTypeLit(literal) = &*alias.type_ann else {
        return None;
    };
    Some(PropsDefinition {
        parameters: parameters_from_members(parsed, &literal.members, imports),
        inherited: false,
    })
}

fn parameters_from_members(
    parsed: &ParsedReactModule,
    members: &[TsTypeElement],
    imports: &BTreeMap<String, String>,
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
                |annotation| parameter_type_facts(parsed, &annotation.type_ann),
            );
            let package = property
                .type_ann
                .as_ref()
                .and_then(|annotation| imported_type_name(&annotation.type_ann))
                .and_then(|name| imports.get(name))
                .cloned();
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
        TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(union)) => {
            let allowed_values = union
                .types
                .iter()
                .map(|member| match &**member {
                    TsType::TsLitType(literal) => match &literal.lit {
                        TsLit::Tpl(template) if !template.types.is_empty() => None,
                        _ => parsed
                            .source_slice_from_span(literal.span)
                            .map(|value| value.trim().trim_matches('"').to_owned()),
                    },
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            return allowed_values.map_or_else(
                || (None, Vec::new()),
                |values| {
                    let values = values.into_iter().fold(Vec::new(), |mut unique, value| {
                        if !unique.contains(&value) {
                            unique.push(value);
                        }
                        unique
                    });
                    (Some(RegistryParameterTypeKind::Enum), values)
                },
            );
        }
        _ => None,
    };
    (kind, Vec::new())
}

fn component_details(
    parsed: &ParsedReactModule,
    span: Span,
    signature_span: Span,
    parameter: Option<&Pat>,
    props: &BTreeMap<String, PropsDefinition>,
    imports: &BTreeMap<String, String>,
) -> RegistryDetails {
    let (props_type_text, mut parameters, mut field_gaps) =
        signature_facts(parsed, parameter, props, imports);
    let type_text = parsed
        .source_slice_from_span(signature_span)
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
    props: &BTreeMap<String, PropsDefinition>,
    imports: &BTreeMap<String, String>,
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
    let type_name = match &*annotation.type_ann {
        TsType::TsTypeLit(literal) => {
            return (
                type_text,
                parameters_from_members(parsed, &literal.members, imports),
                Vec::new(),
            );
        }
        TsType::TsTypeRef(reference) => match &reference.type_name {
            swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.as_ref(),
            swc_ecma_ast::TsEntityName::TsQualifiedName(qualified) => {
                let package = match &qualified.left {
                    swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
                    swc_ecma_ast::TsEntityName::TsQualifiedName(_) => String::new(),
                };
                return imported_props_parameters(
                    parameter,
                    type_text,
                    (!package.is_empty()).then_some(package),
                );
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
    let Some(definition) = props.get(type_name) else {
        if let Some(package) = imports.get(type_name) {
            return imported_props_parameters(parameter, type_text, Some(package.clone()));
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
        Pat::Object(object) => object
            .props
            .iter()
            .filter_map(|property| match property {
                ObjectPatProp::Assign(assign) => Some(assign.key.sym.to_string()),
                ObjectPatProp::KeyValue(key_value) => match &key_value.key {
                    swc_ecma_ast::PropName::Ident(ident) => Some(ident.sym.to_string()),
                    swc_ecma_ast::PropName::Str(value) => {
                        Some(value.value.to_string_lossy().into_owned())
                    }
                    _ => None,
                },
                ObjectPatProp::Rest(_) => None,
            })
            .map(|name| RegistryParameter {
                name,
                type_text: None,
                package: package.clone(),
                type_kind: None,
                allowed_values: Vec::new(),
                description: None,
                default: None,
                required: Some(true),
                extra: Map::new(),
            })
            .collect(),
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

fn imported_type_name(ty: &TsType) -> Option<&str> {
    let TsType::TsTypeRef(reference) = ty else {
        return None;
    };
    match &reference.type_name {
        swc_ecma_ast::TsEntityName::Ident(ident) => Some(ident.sym.as_ref()),
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
    let lines = text
        .lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .collect::<Vec<_>>();
    let mut description = Vec::new();
    let mut deprecation = None;
    let mut example = Vec::new();
    let mut best_practices = Vec::new();
    let mut in_example = false;
    for line in lines {
        if let Some(message) = line.strip_prefix("@deprecated") {
            in_example = false;
            deprecation = Some(RegistryDeprecation {
                message: non_empty(message.trim()),
                since: None,
                replaced_by: None,
                extra: Map::new(),
            });
        } else if let Some(first_line) = line.strip_prefix("@example") {
            in_example = true;
            if !first_line.trim().is_empty() {
                example.push(first_line.trim());
            }
        } else if let Some(guidance) = line.strip_prefix("@guidance") {
            in_example = false;
            let guidance = guidance.trim();
            let guidance = guidance.strip_prefix('-').unwrap_or(guidance).trim();
            if !guidance.is_empty() {
                best_practices.push(RegistryBestPractice {
                    description: guidance.to_owned(),
                    extra: Map::new(),
                });
            }
        } else if line.starts_with('@') {
            in_example = false;
        } else if in_example {
            if !line.is_empty() {
                example.push(line);
            }
        } else if !line.is_empty() {
            description.push(line);
        }
    }
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
    declarations: &[ComponentDeclaration],
    damaged_paths: &[String],
) {
    for component in &mut document.components {
        let declaration = declarations.iter().find(|declaration| {
            (declaration.symbol == component.symbol
                || component.aliases.contains(&declaration.symbol))
                && component
                    .package
                    .as_ref()
                    .is_none_or(|package| declaration.package.as_ref() == Some(package))
        });
        if let Some(declaration) = declaration {
            component.details = Some(declaration.details.clone());
            continue;
        }

        let evidence_damaged = component
            .details
            .as_ref()
            .and_then(|details| details.source_evidence.as_ref())
            .is_some_and(|evidence| damaged_paths.contains(&evidence.path));
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
            "no matching directly exported React declaration was observed",
        ));
    }
}

fn enrich_explicit_tokens(
    document: &mut RegistryDocument,
    repo_root: &Path,
    damaged_paths: &[String],
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
        if damaged_paths.contains(&source.path) {
            if let Some(details) = &mut token.details {
                details.status = RegistryExtractionStatus::Stale;
            }
            continue;
        }
        if !repo_root.join(&source.path).is_file() {
            continue;
        }
        token.details = Some(RegistryDetails {
            status: RegistryExtractionStatus::Complete,
            source_evidence: Some(RegistrySourceEvidence {
                path: source.path.clone(),
                start_line: source.start_line,
                end_line: source.end_line,
                extra: Map::new(),
            }),
            documentation: None,
            signature: None,
            deprecation: None,
            attachments: Vec::new(),
            field_gaps: Vec::new(),
            best_practices: Vec::new(),
            extra: Map::new(),
        });
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
