//! Source-backed Swift registry enrichment.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Map;
use wax_contract::{
    RegistryAttachment, RegistryBestPractice, RegistryDeprecation, RegistryDetails,
    RegistryDocument, RegistryExtractionStatus, RegistryFieldGap, RegistryParameter,
    RegistryParameterTypeKind, RegistrySignature, RegistrySourceEvidence, RegistrySourceKind,
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
    details: RegistryDetails,
}

#[derive(Debug, Clone, Default)]
struct SwiftDocumentation {
    documentation: Option<String>,
    parameter_descriptions: BTreeMap<String, String>,
    best_practices: Vec<RegistryBestPractice>,
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
    let local_enums = local_enum_values(root, source.as_bytes());
    collect_component_declarations(root, source.as_bytes(), true)
        .into_iter()
        .filter_map(|component| {
            let node = find_component_node(root, component.start_byte)?;
            Some(ComponentDeclaration {
                symbol: component.symbol,
                package: package.clone(),
                details: component_details(node, source, path, damaged, &local_enums),
            })
        })
        .collect()
}

fn local_enum_values(root: tree_sitter::Node<'_>, source: &[u8]) -> BTreeMap<String, Vec<String>> {
    let mut enums = BTreeMap::new();
    let mut cursor = root.walk();
    for declaration in root.named_children(&mut cursor) {
        if declaration.kind() != "class_declaration"
            || declaration
                .child_by_field_name("declaration_kind")
                .and_then(|kind| kind.utf8_text(source).ok())
                != Some("enum")
        {
            continue;
        }
        let Some(name_node) = declaration.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(source) else {
            continue;
        };
        if !is_simple_identifier(name)
            || declaration
                .named_children(&mut declaration.walk())
                .any(|child| child.kind() == "type_parameters")
        {
            continue;
        }
        let values = declaration
            .child_by_field_name("body")
            .map(|body| {
                let mut values = Vec::new();
                let mut body_cursor = body.walk();
                for entry in body.named_children(&mut body_cursor) {
                    if entry.kind() != "enum_entry" {
                        continue;
                    }
                    for index in 0..entry.child_count() {
                        let Ok(field_index) = u32::try_from(index) else {
                            continue;
                        };
                        if entry.field_name_for_child(field_index) != Some("name") {
                            continue;
                        }
                        if let Some(value) = entry
                            .child(index)
                            .and_then(|child| child.utf8_text(source).ok())
                            .and_then(non_empty)
                        {
                            values.push(value);
                        }
                    }
                }
                values
            })
            .unwrap_or_default();
        enums.insert(name.to_owned(), values);
    }
    enums
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

fn component_details(
    node: tree_sitter::Node<'_>,
    source: &str,
    path: &str,
    damaged: bool,
    local_enums: &BTreeMap<String, Vec<String>>,
) -> RegistryDetails {
    let source_bytes = source.as_bytes();
    let signature_node = if node.kind() == "function_declaration" {
        Some(node)
    } else {
        find_direct_initializer(node, source_bytes)
    };
    let attached = attached_source_prefix(source, node.start_position().row);
    let documentation = parse_documentation(&attached);
    let mut parameter_descriptions = documentation.parameter_descriptions.clone();
    if let Some(signature_node) =
        signature_node.filter(|signature_node| signature_node.start_byte() != node.start_byte())
    {
        let initializer_documentation = parse_documentation(&attached_source_prefix(
            source,
            signature_node.start_position().row,
        ));
        for (name, description) in initializer_documentation.parameter_descriptions {
            parameter_descriptions.entry(name).or_insert(description);
        }
    }
    let signature = signature_node.and_then(|signature_node| {
        let text = signature_text(signature_node, source_bytes)?;
        Some(RegistrySignature {
            parameters: parameters_from_initializer(
                signature_node,
                source_bytes,
                &parameter_descriptions,
                local_enums,
            ),
            type_text: Some(text),
            extra: Map::new(),
        })
    });
    let attached_attributes = nearby_available_prefix(
        source,
        signature_node.map_or(node.start_position().row, |node| node.start_position().row),
    );
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
            let type_text = parameter.type_text.as_deref()?;
            let reason = if type_text.contains('.') {
                "imported type metadata requires compiler resolution"
            } else if parameter.type_kind.is_none()
                && type_text.contains('<')
                && type_text.contains('>')
            {
                "generic type metadata requires compiler resolution"
            } else {
                return None;
            };
            Some(RegistryFieldGap {
                field: format!("signature.parameters.{}.type_details", parameter.name),
                reason: reason.to_owned(),
                extra: Map::new(),
            })
        }));
    }
    if damaged {
        field_gaps.push(RegistryFieldGap {
            field: "source".to_owned(),
            reason: "the source file contains syntax damage, so details facts may be incomplete"
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

    RegistryDetails {
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
        best_practices: documentation.best_practices,
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
    descriptions: &BTreeMap<String, String>,
    local_enums: &BTreeMap<String, Vec<String>>,
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
        parameter.description = descriptions.get(&parameter.name).cloned();
        (parameter.type_kind, parameter.allowed_values) =
            classify_parameter_type(parameter.type_text.as_deref(), local_enums);
        parameters.push(parameter);
    }
    parameters
}

fn classify_parameter_type(
    type_text: Option<&str>,
    local_enums: &BTreeMap<String, Vec<String>>,
) -> (Option<RegistryParameterTypeKind>, Vec<String>) {
    let Some(type_text) = type_text.map(str::trim) else {
        return (None, Vec::new());
    };
    let type_text = unwrap_parenthesized_type(type_text.trim_end_matches('?').trim_end());
    if type_text.contains('.') || is_custom_generic_type(type_text) {
        return (None, Vec::new());
    }
    let kind = match type_text {
        "Bool" => Some(RegistryParameterTypeKind::Boolean),
        "String" | "Character" => Some(RegistryParameterTypeKind::String),
        "Float" | "Double" => Some(RegistryParameterTypeKind::Number),
        _ if is_swift_integer(type_text) => Some(RegistryParameterTypeKind::Number),
        _ if has_top_level_arrow(type_text) => Some(RegistryParameterTypeKind::Function),
        _ if is_collection_type(type_text) => Some(RegistryParameterTypeKind::Collection),
        _ => None,
    };
    if kind.is_some() {
        return (kind, Vec::new());
    }
    if let Some(values) = local_enums.get(type_text) {
        return if values.is_empty() {
            (None, Vec::new())
        } else {
            (Some(RegistryParameterTypeKind::Enum), values.clone())
        };
    }
    if is_simple_identifier(type_text) {
        return (Some(RegistryParameterTypeKind::Object), Vec::new());
    }
    (None, Vec::new())
}

fn unwrap_parenthesized_type(mut type_text: &str) -> &str {
    while type_text.starts_with('(')
        && type_text.ends_with(')')
        && matching_outer_parentheses(type_text)
    {
        let inner = type_text[1..type_text.len() - 1].trim();
        if inner.contains(',') && !has_top_level_arrow(inner) {
            break;
        }
        type_text = inner;
    }
    type_text
}

fn matching_outer_parentheses(type_text: &str) -> bool {
    let mut depth = 0_u32;
    for (index, byte) in type_text.as_bytes().iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index + 1 != type_text.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn is_swift_integer(type_text: &str) -> bool {
    ["Int", "UInt"].iter().any(|prefix| {
        type_text.strip_prefix(prefix).is_some_and(|suffix| {
            suffix.is_empty() || suffix.chars().all(|character| character.is_ascii_digit())
        })
    })
}

fn has_top_level_arrow(type_text: &str) -> bool {
    let mut angle_depth = 0_u32;
    let mut bracket_depth = 0_u32;
    let mut paren_depth = 0_u32;
    let bytes = type_text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'<' => angle_depth += 1,
            b'>' => angle_depth = angle_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'-' if angle_depth == 0
                && bracket_depth == 0
                && paren_depth == 0
                && bytes.get(index + 1) == Some(&b'>') =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn is_collection_type(type_text: &str) -> bool {
    (type_text.starts_with('[') && type_text.ends_with(']'))
        || is_tuple_type(type_text)
        || COLLECTION_GENERIC_PREFIXES
            .iter()
            .any(|prefix| type_text.starts_with(prefix) && type_text.ends_with('>'))
}

const COLLECTION_GENERIC_PREFIXES: [&str; 3] = ["Array<", "Set<", "Dictionary<"];

fn is_tuple_type(type_text: &str) -> bool {
    type_text.starts_with('(')
        && type_text.ends_with(')')
        && matching_outer_parentheses(type_text)
        && type_text[1..type_text.len() - 1].contains(',')
}

fn is_custom_generic_type(type_text: &str) -> bool {
    type_text.contains('<')
        && type_text.contains('>')
        && !COLLECTION_GENERIC_PREFIXES
            .iter()
            .any(|prefix| type_text.starts_with(prefix) && type_text.ends_with('>'))
}

fn is_simple_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn parameter_from_text(text: &str) -> Option<RegistryParameter> {
    let (labels, value) = text.split_once(':')?;
    let label_parts = labels.split_whitespace().collect::<Vec<_>>();
    let name = match label_parts.as_slice() {
        [name] => *name,
        [_, internal, ..] => *internal,
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
        type_kind: None,
        allowed_values: Vec::new(),
        description: None,
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
            let line = line.trim_start();
            let content = line
                .strip_prefix("///")
                .or_else(|| line.strip_prefix("/**"))
                .or_else(|| line.strip_prefix("*/"))
                .or_else(|| line.strip_prefix('*'))?;
            let content = content.trim_end();
            let text = content.trim_start();
            Some((content.len() - text.len(), text))
        })
        .collect::<Vec<_>>();
    let parameter_descriptions = parameter_descriptions(&lines);
    let mut description = Vec::new();
    let mut example = Vec::new();
    let mut best_practices = Vec::new();
    let mut in_example = false;
    let mut in_fence = false;
    let mut deprecation = None;
    let mut index = 0;
    while index < lines.len() {
        let (indent, line) = lines[index];
        if line == "- Parameters:" {
            in_example = false;
            index = skip_parameter_section(&lines, index, indent);
            continue;
        }
        if line.strip_prefix("- Parameter ").is_some() {
            in_example = false;
            index = skip_parameter_section(&lines, index, indent);
            continue;
        }
        if let Some(guidance) = line.strip_prefix("- Guidance:") {
            let (description, next) = continued_text(&lines, index + 1, indent, guidance);
            if !guidance.trim().is_empty()
                && let Some(description) = non_empty(&description)
            {
                best_practices.push(RegistryBestPractice {
                    description,
                    extra: Map::new(),
                });
            }
            in_example = false;
            index = next;
            continue;
        }
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
        index += 1;
    }
    SwiftDocumentation {
        documentation: non_empty(description.join("\n").trim()),
        parameter_descriptions,
        best_practices,
        deprecation,
        example: non_empty(example.join("\n").trim()),
    }
}

fn skip_parameter_section(lines: &[(usize, &str)], mut index: usize, indent: usize) -> usize {
    index += 1;
    while index < lines.len() && lines[index].0 > indent {
        index += 1;
    }
    index
}

fn parameter_descriptions(lines: &[(usize, &str)]) -> BTreeMap<String, String> {
    let mut descriptions = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        let (indent, line) = lines[index];
        if let Some(parameter) = line.strip_prefix("- Parameter ") {
            if let Some((name, initial)) = parameter.split_once(':') {
                let (description, next) = continued_text(lines, index + 1, indent, initial);
                insert_first_non_empty(&mut descriptions, name.trim(), description);
                index = next;
                continue;
            }
        } else if line == "- Parameters:" {
            index += 1;
            while index < lines.len() && lines[index].0 > indent {
                let (parameter_indent, parameter_line) = lines[index];
                if let Some((name, initial)) = parameter_line
                    .strip_prefix("- ")
                    .and_then(|parameter| parameter.split_once(':'))
                {
                    let (description, next) =
                        continued_text(lines, index + 1, parameter_indent, initial);
                    insert_first_non_empty(&mut descriptions, name.trim(), description);
                    index = next;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        index += 1;
    }
    descriptions
}

fn continued_text(
    lines: &[(usize, &str)],
    mut index: usize,
    marker_indent: usize,
    initial: &str,
) -> (String, usize) {
    let mut parts = Vec::new();
    if !initial.trim().is_empty() {
        parts.push(initial.trim());
    }
    while index < lines.len() && lines[index].0 > marker_indent {
        let continuation = lines[index].1.trim();
        if !continuation.is_empty() {
            parts.push(continuation);
        }
        index += 1;
    }
    (parts.join(" "), index)
}

fn insert_first_non_empty(
    descriptions: &mut BTreeMap<String, String>,
    name: &str,
    description: String,
) {
    if !name.is_empty() && !description.is_empty() {
        descriptions.entry(name.to_owned()).or_insert(description);
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
                    .details
                    .source_evidence
                    .as_ref()
                    .map(|evidence| evidence.path.clone())
                    .unwrap_or_default(),
            ) && component.details.is_some()
            {
                if let Some(details) = &mut component.details {
                    details.status = RegistryExtractionStatus::Stale;
                }
                continue;
            }
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
        if component.details.is_some() && !damaged_paths.is_empty() {
            continue;
        }
        component.details = Some(unavailable(
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

fn repo_relative_path(repo_root: &Path, file: &Path) -> String {
    file.strip_prefix(repo_root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}
