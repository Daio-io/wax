//! Source-backed React registry enrichment.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Map;
use swc_common::{Span, Spanned, comments::CommentKind};
use swc_ecma_ast::{
    Decl, Expr, ModuleDecl, ModuleItem, ObjectPat, ObjectPatProp, Pat, Stmt, TsInterfaceDecl,
    TsType, TsTypeAliasDecl, TsTypeElement,
};
use wax_contract::{
    RegistryAttachment, RegistryDeprecation, RegistryDocument, RegistryExtracted,
    RegistryExtractionStatus, RegistryFieldGap, RegistryParameter, RegistrySignature,
    RegistrySourceEvidence,
};
use wax_lang_api::{DiscoverRegistryRequest, DiscoverRegistryResult, npm_package_name_for_path};

use crate::discover::{ReactDiscoverError, collect_react_files, repo_relative_path};
use crate::swc_parse::{ParsedReactModule, ReactParseOutcome, parse_react_source_file};

#[derive(Debug)]
struct PropsDefinition {
    parameters: Vec<RegistryParameter>,
    inherited: bool,
}

#[derive(Debug)]
struct ComponentDeclaration {
    symbol: String,
    package: Option<String>,
    extracted: RegistryExtracted,
}

#[derive(Debug, Default)]
struct JsDoc {
    documentation: Option<String>,
    deprecation: Option<RegistryDeprecation>,
    example: Option<String>,
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
    let mut source_files = Vec::new();
    for root in &request.roots {
        let root = repo_root.join(root);
        if !root.exists() {
            return Err(ReactDiscoverError::MissingRoot(root));
        }
        collect_react_files(&root, &mut source_files).map_err(|source| ReactDiscoverError::Io {
            context: format!("read React root {}", root.display()),
            source,
        })?;
    }
    source_files.sort();

    let mut declarations = Vec::new();
    let mut diagnostics = Vec::new();
    let mut damaged_paths = Vec::new();
    for file_path in source_files {
        let relative_path = repo_relative_path(repo_root, &file_path);
        match parse_react_source_file(repo_root, Path::new(&relative_path)).map_err(
            |err| match err {
                crate::ReactParseError::Io { source, .. } => ReactDiscoverError::Io {
                    context: format!("read React source {relative_path}"),
                    source,
                },
            },
        )? {
            ReactParseOutcome::Parsed(parsed) => {
                declarations.extend(component_declarations(
                    &parsed,
                    npm_package_name_for_path(repo_root, &file_path),
                ));
            }
            ReactParseOutcome::Failed(diagnostic) => {
                damaged_paths.push(relative_path);
                diagnostics.push(diagnostic);
            }
        }
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

fn component_declarations(
    parsed: &ParsedReactModule,
    package: Option<String>,
) -> Vec<ComponentDeclaration> {
    let props = collect_props_definitions(parsed);
    let mut declarations = Vec::new();
    for item in &parsed.module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) = item else {
            continue;
        };
        match &export.decl {
            Decl::Fn(function) => declarations.push(ComponentDeclaration {
                symbol: function.ident.sym.to_string(),
                package: package.clone(),
                extracted: extracted_component(
                    parsed,
                    export.span,
                    Span::new(
                        export.span.lo,
                        function
                            .function
                            .body
                            .as_ref()
                            .map_or(export.span.hi, |body| body.span.lo),
                    ),
                    function.function.params.first().map(|param| &param.pat),
                    &props,
                ),
            }),
            Decl::Var(variable) => {
                for declarator in &variable.decls {
                    let Pat::Ident(binding) = &declarator.name else {
                        continue;
                    };
                    let Some(Expr::Arrow(arrow)) = declarator.init.as_deref() else {
                        continue;
                    };
                    declarations.push(ComponentDeclaration {
                        symbol: binding.id.sym.to_string(),
                        package: package.clone(),
                        extracted: extracted_component(
                            parsed,
                            export.span,
                            Span::new(export.span.lo, arrow.body.span().lo),
                            arrow.params.first(),
                            &props,
                        ),
                    });
                }
            }
            _ => {}
        }
    }
    declarations
}

fn collect_props_definitions(parsed: &ParsedReactModule) -> BTreeMap<String, PropsDefinition> {
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
                    props_from_interface(parsed, interface),
                );
            }
            Some(Decl::TsTypeAlias(alias)) => {
                if let Some(definition) = props_from_alias(parsed, alias) {
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
) -> PropsDefinition {
    PropsDefinition {
        parameters: parameters_from_members(parsed, &interface.body.body),
        inherited: !interface.extends.is_empty(),
    }
}

fn props_from_alias(
    parsed: &ParsedReactModule,
    alias: &TsTypeAliasDecl,
) -> Option<PropsDefinition> {
    let TsType::TsTypeLit(literal) = &*alias.type_ann else {
        return None;
    };
    Some(PropsDefinition {
        parameters: parameters_from_members(parsed, &literal.members),
        inherited: false,
    })
}

fn parameters_from_members(
    parsed: &ParsedReactModule,
    members: &[TsTypeElement],
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
            Some(RegistryParameter {
                name,
                type_text,
                default: None,
                required: Some(!property.optional),
                extra: Map::new(),
            })
        })
        .collect()
}

fn extracted_component(
    parsed: &ParsedReactModule,
    span: Span,
    signature_span: Span,
    parameter: Option<&Pat>,
    props: &BTreeMap<String, PropsDefinition>,
) -> RegistryExtracted {
    let (props_type_text, mut parameters, mut field_gaps) =
        signature_facts(parsed, parameter, props);
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

    RegistryExtracted {
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
        extra: Map::new(),
    }
}

fn signature_facts(
    parsed: &ParsedReactModule,
    parameter: Option<&Pat>,
    props: &BTreeMap<String, PropsDefinition>,
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
    let Some(type_name) = type_annotation.and_then(|annotation| match &*annotation.type_ann {
        TsType::TsTypeRef(reference) => match &reference.type_name {
            swc_ecma_ast::TsEntityName::Ident(ident) => Some(ident.sym.as_ref()),
            swc_ecma_ast::TsEntityName::TsQualifiedName(_) => None,
        },
        _ => None,
    }) else {
        return (type_text, Vec::new(), Vec::new());
    };
    let Some(definition) = props.get(type_name) else {
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
    let gaps = if definition.inherited {
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
    (type_text, definition.parameters.clone(), gaps)
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
            component.extracted = Some(declaration.extracted.clone());
        } else if !damaged_paths.is_empty() && component.extracted.is_some() {
            if let Some(extracted) = &mut component.extracted {
                extracted.status = RegistryExtractionStatus::Stale;
            }
        } else {
            component.extracted = Some(unavailable(
                "source_evidence",
                "no matching directly exported React declaration was observed",
            ));
        }
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
            if let Some(extracted) = &mut token.extracted {
                extracted.status = RegistryExtractionStatus::Stale;
            }
            continue;
        }
        if !repo_root.join(&source.path).is_file() {
            continue;
        }
        token.extracted = Some(RegistryExtracted {
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
            extra: Map::new(),
        });
    }
}

fn unavailable(field: &str, reason: &str) -> RegistryExtracted {
    RegistryExtracted {
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
        extra: Map::new(),
    }
}
