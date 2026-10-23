//! Source-backed Swift registry enrichment.

use std::path::Path;

use serde_json::Map;
use wax_contract::{
    RegistryAttachment, RegistryDeprecation, RegistryDocument, RegistryExtracted,
    RegistryExtractionStatus, RegistryFieldGap, RegistryParameter, RegistrySignature,
    RegistrySourceEvidence, RegistrySourceKind,
};
use wax_lang_api::{
    DiscoverRegistryRequest, DiscoverRegistryResult, swift_module_from_source_path,
};

use crate::component_detect::collect_component_declarations;
use crate::discover::SwiftDiscoverError;
use crate::swift_ast::{
    ParseSwiftFileError, collect_swift_files, new_parser, parse_swift_file_permissive,
    partial_tree_parse_diagnostic, tree_has_syntax_errors, unparseable_file_diagnostic,
};

#[derive(Debug)]
struct ComponentDeclaration {
    symbol: String,
    package: Option<String>,
    extracted: RegistryExtracted,
}

#[derive(Debug, Default)]
struct SwiftDocumentation {
    documentation: Option<String>,
    deprecation: Option<RegistryDeprecation>,
    example: Option<String>,
}

/// Enriches an existing registry document with facts proven by Swift declarations.
///
/// Existing identities, creator metadata, and canonical token values are preserved. Source
/// syntax is parsed directly; discovering details never builds the Swift project.
///
/// # Errors
///
/// Returns [`SwiftDiscoverError::InvalidLanguageId`] for non-Swift requests,
/// [`SwiftDiscoverError::MissingRoot`] for nonexistent roots,
/// [`SwiftDiscoverError::InvalidRegistry`] for contract-invalid registry data, or parser and
/// I/O errors reported by Swift source discovery.
pub fn discover_registry_details(
    request: &DiscoverRegistryRequest,
) -> Result<DiscoverRegistryResult, SwiftDiscoverError> {
    if request.language_id.as_str() != "swift" {
        return Err(SwiftDiscoverError::InvalidLanguageId(
            request.language_id.to_string(),
        ));
    }

    let repo_root = Path::new(&request.repo_root);
    let roots = request
        .roots
        .iter()
        .map(|root| repo_root.join(root))
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    for root in &roots {
        if !root.exists() {
            return Err(SwiftDiscoverError::MissingRoot(root.clone()));
        }
        collect_swift_files(root, &mut files).map_err(|source| SwiftDiscoverError::Io {
            context: format!("read Swift root {}", root.display()),
            source,
        })?;
    }
    files.sort();

    let mut parser =
        new_parser().map_err(|error| SwiftDiscoverError::ParserInitFailed(error.to_string()))?;
    let mut declarations = Vec::new();
    let mut damaged_paths = Vec::new();
    let mut diagnostics = Vec::new();
    for file in files {
        let relative_path = repo_relative_path(repo_root, &file);
        match parse_swift_file_permissive(&mut parser, &file) {
            Ok(parsed) => {
                let damaged = tree_has_syntax_errors(&parsed.tree);
                declarations.extend(component_declarations(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &relative_path,
                    swift_module_from_source_path(&file),
                    damaged,
                ));
                if damaged {
                    damaged_paths.push(relative_path.clone());
                    diagnostics.push(partial_tree_parse_diagnostic(
                        parsed.tree.root_node(),
                        &relative_path,
                    ));
                }
            }
            Err(ParseSwiftFileError::ParseFailed(_)) => {
                damaged_paths.push(relative_path.clone());
                diagnostics.push(unparseable_file_diagnostic(&relative_path));
            }
            Err(ParseSwiftFileError::Io { context, source }) => {
                return Err(SwiftDiscoverError::Io { context, source });
            }
        }
    }

    let mut document = request.registry.clone();
    enrich_components(&mut document, &declarations, &damaged_paths);
    enrich_explicit_tokens(&mut document, repo_root, &damaged_paths);
    document
        .validate()
        .map_err(SwiftDiscoverError::InvalidRegistry)?;

    Ok(DiscoverRegistryResult {
        document,
        diagnostics,
    })
}

fn component_declarations(
    root: tree_sitter::Node<'_>,
    source: &str,
    path: &str,
    package: Option<String>,
    damaged: bool,
) -> Vec<ComponentDeclaration> {
    collect_component_declarations(root, source.as_bytes(), true)
        .into_iter()
        .filter_map(|component| {
            let node = find_component_node(root, component.start_byte)?;
            Some(ComponentDeclaration {
                symbol: component.symbol,
                package: package.clone(),
                extracted: extracted_component(node, source, path, damaged),
            })
        })
        .collect()
}

fn find_component_node<'tree>(
    root: tree_sitter::Node<'tree>,
    start_byte: usize,
) -> Option<tree_sitter::Node<'tree>> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "class_declaration" | "function_declaration")
            && node.start_byte() == start_byte
        {
            return Some(node);
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
    None
}

fn extracted_component(
    node: tree_sitter::Node<'_>,
    source: &str,
    path: &str,
    damaged: bool,
) -> RegistryExtracted {
    let source_bytes = source.as_bytes();
    let signature_node = if node.kind() == "function_declaration" {
        Some(node)
    } else {
        find_direct_initializer(node, source_bytes)
    };
    let signature = signature_node.and_then(|signature_node| {
        let text = signature_text(signature_node, source_bytes)?;
        Some(RegistrySignature {
            parameters: parameters_from_initializer(signature_node, source_bytes),
            type_text: Some(text),
            extra: Map::new(),
        })
    });
    let attached = attached_source_prefix(source, node.start_position().row);
    let attached_attributes = nearby_available_prefix(
        source,
        signature_node.map_or(node.start_position().row, |node| node.start_position().row),
    );
    let documentation = parse_documentation(&attached);
    let mut field_gaps = Vec::new();
    if signature.is_none() {
        field_gaps.push(RegistryFieldGap {
            field: "signature.parameters".to_owned(),
            reason: "no direct initializer was observed; synthesized or inherited initializer facts require compiler resolution".to_owned(),
            extra: Map::new(),
        });
    }
    if let Some(signature) = &signature {
        field_gaps.extend(signature.parameters.iter().filter_map(|parameter| {
            parameter
                .type_text
                .as_deref()
                .filter(|type_text| type_text.contains('.'))
                .map(|_| RegistryFieldGap {
                    field: format!("signature.parameters.{}.type_details", parameter.name),
                    reason: "imported type metadata requires compiler resolution".to_owned(),
                    extra: Map::new(),
                })
        }));
    }
    if damaged {
        field_gaps.push(RegistryFieldGap {
            field: "source".to_owned(),
            reason: "the source file contains syntax damage, so extracted facts may be incomplete"
                .to_owned(),
            extra: Map::new(),
        });
    }
    let status = if field_gaps.is_empty() {
        RegistryExtractionStatus::Complete
    } else {
        RegistryExtractionStatus::Partial
    };
    let attachments = documentation
        .example
        .map(|content| {
            vec![RegistryAttachment {
                kind: RegistrySourceKind::Example,
                path: path.to_owned(),
                content,
                language: Some("swift".to_owned()),
                extra: Map::new(),
            }]
        })
        .unwrap_or_default();

    RegistryExtracted {
        status,
        source_evidence: Some(RegistrySourceEvidence {
            path: path.to_owned(),
            start_line: Some(u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX)),
            end_line: Some(u32::try_from(node.end_position().row + 1).unwrap_or(u32::MAX)),
            extra: Map::new(),
        }),
        documentation: documentation.documentation,
        signature,
        deprecation: deprecation_from_available(&attached_attributes).or(documentation.deprecation),
        attachments,
        field_gaps,
        extra: Map::new(),
    }
}

fn find_direct_initializer<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &[u8],
) -> Option<tree_sitter::Node<'tree>> {
    let body = node.child_by_field_name("body")?;
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .find(|child| child.kind() == "init_declaration" && is_public_declaration(*child, source))
}

fn is_public_declaration(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let modifiers = (0..node.child_count())
        .filter_map(|index| node.child(index))
        .find(|child| child.kind() == "modifiers");
    let Some(modifiers) = modifiers else {
        return false;
    };
    let mut cursor = modifiers.walk();
    modifiers.named_children(&mut cursor).any(|modifier| {
        modifier.kind() == "visibility_modifier"
            && modifier.utf8_text(source).ok() == Some("public")
    })
}

fn signature_text(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let text = node.utf8_text(source).ok()?.trim();
    let body_start = find_top_level_body(text).unwrap_or(text.len());
    let signature = text[..body_start].trim();
    (!signature.is_empty()).then(|| signature.to_owned())
}

fn find_top_level_body(text: &str) -> Option<usize> {
    let mut parens = 0_u32;
    for (index, character) in text.char_indices() {
        match character {
            '(' => parens += 1,
            ')' => parens = parens.saturating_sub(1),
            '{' if parens == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn parameters_from_initializer(
    initializer: tree_sitter::Node<'_>,
    source: &[u8],
) -> Vec<RegistryParameter> {
    let mut parameters = Vec::new();
    let mut cursor = initializer.walk();
    let children = initializer.named_children(&mut cursor).collect::<Vec<_>>();
    for (index, child) in children.iter().enumerate() {
        if child.kind() != "parameter" {
            continue;
        }
        let text = child.utf8_text(source).ok().unwrap_or_default();
        let end = children
            .iter()
            .skip(index + 1)
            .find(|next| next.kind() == "parameter")
            .map_or(initializer.end_byte(), |next| next.start_byte());
        let default = source
            .get(child.end_byte()..end)
            .and_then(|text| std::str::from_utf8(text).ok())
            .and_then(|text| text.split_once('=').map(|(_, value)| value))
            .map(str::trim)
            .map(|value| {
                value
                    .trim_end_matches(['}', '{'])
                    .trim()
                    .trim_end_matches(')')
                    .trim_end_matches(',')
                    .trim()
            })
            .and_then(non_empty);
        let Some(mut parameter) = parameter_from_text(text) else {
            continue;
        };
        parameter.default = default;
        parameter.required = Some(parameter.default.is_none());
        parameters.push(parameter);
    }
    parameters
}

fn parameter_from_text(text: &str) -> Option<RegistryParameter> {
    let (labels, value) = text.split_once(':')?;
    let label_parts = labels.split_whitespace().collect::<Vec<_>>();
    let name = match label_parts.as_slice() {
        [name] => *name,
        ["_", internal, ..] => *internal,
        [external, ..] => *external,
        [] => return None,
    };
    let (type_text, default) = value
        .split_once('=')
        .map_or((value.trim(), None), |(type_text, default)| {
            (type_text.trim(), non_empty(default.trim()))
        });
    Some(RegistryParameter {
        name: name.to_owned(),
        type_text: non_empty(type_text),
        required: Some(default.is_none()),
        default,
        extra: Map::new(),
    })
}

fn attached_source_prefix(source: &str, declaration_row: usize) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    let mut start = declaration_row.min(lines.len());
    while start > 0 {
        let previous = lines[start - 1].trim();
        if previous.starts_with("///")
            || previous.starts_with("/**")
            || previous.starts_with('*')
            || previous.starts_with("*/")
            || previous.starts_with('@')
        {
            start -= 1;
        } else {
            break;
        }
    }
    lines[start..declaration_row.min(lines.len())].join("\n")
}

fn nearby_available_prefix(source: &str, declaration_row: usize) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    let end = declaration_row.min(lines.len().saturating_sub(1));
    let start = end.saturating_sub(8);
    let mut attributes = Vec::new();
    for line in lines[start..=end].iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if trimmed.starts_with('@') {
            attributes.push(*line);
        } else if !attributes.is_empty() {
            break;
        }
    }
    attributes.reverse();
    attributes.join("\n")
}

fn parse_documentation(attached: &str) -> SwiftDocumentation {
    let lines = attached
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("///")
                .or_else(|| line.strip_prefix("/**"))
                .or_else(|| line.strip_prefix("*/"))
                .or_else(|| line.strip_prefix('*'))
                .map(str::trim)
        })
        .collect::<Vec<_>>();
    let mut description = Vec::new();
    let mut example = Vec::new();
    let mut in_example = false;
    let mut in_fence = false;
    let mut deprecation = None;
    for line in lines {
        if let Some(message) = line.strip_prefix("- Deprecated:") {
            deprecation = Some(RegistryDeprecation {
                message: non_empty(message.trim()),
                since: None,
                replaced_by: None,
                extra: Map::new(),
            });
        } else if line.eq_ignore_ascii_case("Example:") || line == "- Example:" {
            in_example = true;
        } else if in_example && line.starts_with("```") {
            in_fence = !in_fence;
        } else if in_example && (in_fence || !line.is_empty()) {
            example.push(line);
        } else if !line.is_empty() {
            description.push(line);
        }
    }
    SwiftDocumentation {
        documentation: non_empty(description.join("\n").trim()),
        deprecation,
        example: non_empty(example.join("\n").trim()),
    }
}

fn deprecation_from_available(attached: &str) -> Option<RegistryDeprecation> {
    let available = attached
        .lines()
        .find(|line| line.contains("@available") && line.contains("deprecated"))?;
    let message = available
        .split("message:")
        .nth(1)
        .and_then(|text| text.split('"').nth(1))
        .and_then(non_empty);
    Some(RegistryDeprecation {
        message,
        since: None,
        replaced_by: None,
        extra: Map::new(),
    })
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
            // Preserve prior facts when a damaged file still produces a partial declaration.
            if damaged_paths.contains(
                &declaration
                    .extracted
                    .source_evidence
                    .as_ref()
                    .map(|evidence| evidence.path.clone())
                    .unwrap_or_default(),
            ) && component.extracted.is_some()
            {
                if let Some(extracted) = &mut component.extracted {
                    extracted.status = RegistryExtractionStatus::Stale;
                }
                continue;
            }
            component.extracted = Some(declaration.extracted.clone());
            continue;
        }
        let evidence_damaged = component
            .extracted
            .as_ref()
            .and_then(|extracted| extracted.source_evidence.as_ref())
            .is_some_and(|evidence| damaged_paths.contains(&evidence.path));
        if evidence_damaged {
            if let Some(extracted) = &mut component.extracted {
                extracted.status = RegistryExtractionStatus::Stale;
            }
            continue;
        }
        if component.extracted.is_some() && !damaged_paths.is_empty() {
            continue;
        }
        component.extracted = Some(unavailable(
            "source_evidence",
            "no matching public Swift View or function declaration was observed",
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
                source.kind == RegistrySourceKind::Declaration
                    && source
                        .language
                        .as_deref()
                        .is_none_or(|language| language == "swift")
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

fn repo_relative_path(repo_root: &Path, file: &Path) -> String {
    file.strip_prefix(repo_root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}
