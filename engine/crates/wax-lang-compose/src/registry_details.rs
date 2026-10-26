//! Source-backed Compose registry enrichment.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Map;
use wax_contract::{
    RegistryAttachment, RegistryBestPractice, RegistryDeprecation, RegistryDetails,
    RegistryDocument, RegistryExtractionStatus, RegistryFieldGap, RegistryParameter,
    RegistryParameterTypeKind, RegistrySignature, RegistrySourceEvidence, RegistrySourceKind,
};
use wax_lang_api::{DiscoverRegistryRequest, DiscoverRegistryResult};

use crate::discover::{
    ComposeDiscoverError, ParsedKotlinSourceFile, collect_and_parse_kotlin_sources,
    discoverable_component_name,
};
use crate::kotlin_ast::{ImportBindings, annotation_with_name, collect_import_bindings};
use crate::kotlin_recovery::ByteRange;

#[derive(Debug)]
struct ComponentDeclaration {
    symbol: String,
    package: Option<String>,
    details: RegistryDetails,
}

struct ParameterTypeFacts {
    local_types: BTreeSet<String>,
    imports: ImportBindings,
    local_enums: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
struct KDoc {
    documentation: Option<String>,
    parameter_descriptions: BTreeMap<String, String>,
    best_practices: Vec<RegistryBestPractice>,
    deprecation: Option<RegistryDeprecation>,
    example: Option<String>,
}

/// Enriches an existing registry document with facts proven by Compose source declarations.
///
/// Creator-owned fields, component and token identities, and canonical token values are
/// preserved. Syntax failures are returned as diagnostics and never cause generated facts
/// from the input document to be removed.
///
/// # Errors
///
/// Returns [`ComposeDiscoverError::InvalidLanguageId`] for non-Compose requests,
/// [`ComposeDiscoverError::MissingRoot`] for nonexistent roots,
/// [`ComposeDiscoverError::InvalidRegistry`] for contract-invalid registry data,
/// [`ComposeDiscoverError::ParserInitFailed`] when tree-sitter cannot initialize, or
/// [`ComposeDiscoverError::Io`] when source discovery or reading fails.
pub fn discover_registry_details(
    request: &DiscoverRegistryRequest,
) -> Result<DiscoverRegistryResult, ComposeDiscoverError> {
    if request.language_id.as_str() != "compose" {
        return Err(ComposeDiscoverError::InvalidLanguageId(
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
        collect_and_parse_kotlin_sources(repo_root, &absolute_roots, &mut diagnostics)?;

    let mut declarations = Vec::new();
    for file in &parsed_files {
        declarations.extend(component_declarations(file));
    }

    let mut document = request.registry.clone();
    enrich_components(&mut document, &declarations, &damaged_paths);
    enrich_explicit_tokens(&mut document, repo_root, &damaged_paths);
    document
        .validate()
        .map_err(ComposeDiscoverError::InvalidRegistry)?;

    Ok(DiscoverRegistryResult {
        document,
        diagnostics,
    })
}

fn component_declarations(file: &ParsedKotlinSourceFile) -> Vec<ComponentDeclaration> {
    let source = file.parsed.source.as_bytes();
    let primary_root = file.parsed.primary_tree().root_node();
    let imports = collect_import_bindings(primary_root, source);
    let type_facts = ParameterTypeFacts {
        local_types: local_type_names(primary_root, source),
        imports,
        local_enums: local_enum_values(primary_root, source),
    };

    let mut declarations = Vec::new();
    for pass in file.parsed.passes() {
        collect_component_declarations(
            pass.tree.root_node(),
            source,
            &pass.clean,
            file,
            &type_facts,
            &mut declarations,
        );
    }
    declarations
}

fn collect_component_declarations(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    clean: &[ByteRange],
    file: &ParsedKotlinSourceFile,
    type_facts: &ParameterTypeFacts,
    declarations: &mut Vec<ComponentDeclaration>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if let Some(name) = discoverable_component_name(node, source, clean)
            && !declarations.iter().any(|existing| existing.symbol == name)
        {
            declarations.push(ComponentDeclaration {
                symbol: name,
                package: file.package.clone(),
                details: component_details(node, source, file, type_facts),
            });
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
}

fn component_details(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    file: &ParsedKotlinSourceFile,
    type_facts: &ParameterTypeFacts,
) -> RegistryDetails {
    let kdoc = kdoc_for_declaration(node, source);
    let (parameters, mut field_gaps) =
        parameters_from_function(node, source, type_facts, &kdoc.parameter_descriptions);
    let type_text = signature_type_text(node, source);
    let deprecation = deprecated_annotation(node, source).or(kdoc.deprecation);
    let attachments = kdoc
        .example
        .map(|content| {
            vec![RegistryAttachment {
                kind: RegistrySourceKind::Example,
                path: file.relative_path.clone(),
                content,
                language: Some("kotlin".to_owned()),
                extra: Map::new(),
            }]
        })
        .unwrap_or_default();

    if type_text.is_none() {
        field_gaps.push(RegistryFieldGap {
            field: "signature.type_text".to_owned(),
            reason: "function signature text was not available in source".to_owned(),
            extra: Map::new(),
        });
    }

    let source_evidence = Some(RegistrySourceEvidence {
        path: file.relative_path.clone(),
        start_line: Some(declaration_start_line(node)),
        end_line: Some(u32::try_from(node.end_position().row + 1).unwrap_or(u32::MAX)),
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
        documentation: kdoc.documentation,
        signature: Some(RegistrySignature {
            type_text,
            parameters,
            extra: Map::new(),
        }),
        deprecation,
        attachments,
        field_gaps,
        best_practices: kdoc.best_practices,
        extra: Map::new(),
    }
}

fn signature_type_text(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let fun_node = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "fun")?;
    let params = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "function_value_parameters")?;
    let text = source.get(fun_node.start_byte()..params.end_byte())?;
    std::str::from_utf8(text).ok().map(|value| value.to_owned())
}

fn parameters_from_function(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    type_facts: &ParameterTypeFacts,
    descriptions: &BTreeMap<String, String>,
) -> (Vec<RegistryParameter>, Vec<RegistryFieldGap>) {
    let Some(params_node) = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "function_value_parameters")
    else {
        return (Vec::new(), Vec::new());
    };

    let mut parameters = Vec::new();
    let mut cursor = params_node.walk();
    let children = params_node.children(&mut cursor).collect::<Vec<_>>();
    let mut index = 0;
    while index < children.len() {
        let child = children[index];
        if child.kind() != "parameter" {
            index += 1;
            continue;
        }

        let Some(name) = parameter_name(child, source) else {
            index += 1;
            continue;
        };
        let type_text = parameter_type_text(child, source);
        let mut default = None;
        if let Some(next) = children.get(index + 1)
            && next.kind() == "="
            && let Some(value) = children.get(index + 2)
        {
            default = value
                .utf8_text(source)
                .ok()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned);
            index += 2;
        }

        let imported_type = type_text
            .as_deref()
            .and_then(simple_type_name)
            .filter(|type_name| {
                !type_facts.local_types.contains(*type_name)
                    && !is_builtin_kotlin_type(type_name)
                    && type_facts.imports.package_for_symbol(type_name).is_some()
            });
        let package = imported_type
            .and_then(|type_name| type_facts.imports.package_for_symbol(type_name))
            .map(str::to_owned);

        let (type_kind, allowed_values) = if imported_type.is_some() {
            (None, Vec::new())
        } else {
            classify_parameter_type(type_text.as_deref(), &type_facts.local_enums)
        };

        parameters.push(RegistryParameter {
            description: descriptions.get(&name).cloned(),
            name,
            type_text,
            package,
            type_kind,
            allowed_values,
            default: default.clone(),
            required: Some(default.is_none()),
            extra: Map::new(),
        });
        index += 1;
    }

    (parameters, Vec::new())
}

fn classify_parameter_type(
    type_text: Option<&str>,
    local_enums: &BTreeMap<String, Vec<String>>,
) -> (Option<RegistryParameterTypeKind>, Vec<String>) {
    let Some(type_text) = type_text.map(str::trim).filter(|text| !text.is_empty()) else {
        return (None, Vec::new());
    };
    let direct_type = type_text.trim_end_matches('?').trim();
    if has_top_level_function_arrow(strip_outer_parentheses(direct_type)) {
        return (Some(RegistryParameterTypeKind::Function), Vec::new());
    }

    if direct_type.contains('.') {
        return (None, Vec::new());
    }
    if let Some(values) = local_enums.get(direct_type) {
        return if values.is_empty() {
            (None, Vec::new())
        } else {
            (Some(RegistryParameterTypeKind::Enum), values.clone())
        };
    }
    if direct_type == "Boolean" {
        return (Some(RegistryParameterTypeKind::Boolean), Vec::new());
    }
    if matches!(direct_type, "String" | "Char") {
        return (Some(RegistryParameterTypeKind::String), Vec::new());
    }
    if matches!(
        direct_type,
        "Byte"
            | "Short"
            | "Int"
            | "Long"
            | "Float"
            | "Double"
            | "Number"
            | "UByte"
            | "UShort"
            | "UInt"
            | "ULong"
    ) {
        return (Some(RegistryParameterTypeKind::Number), Vec::new());
    }
    if is_collection_type(direct_type) {
        return (Some(RegistryParameterTypeKind::Collection), Vec::new());
    }
    if !direct_type.contains('<') && simple_type_name(direct_type).is_some() {
        return (Some(RegistryParameterTypeKind::Object), Vec::new());
    }

    (None, Vec::new())
}

fn strip_outer_parentheses(mut type_text: &str) -> &str {
    loop {
        let Some(stripped) = type_text
            .strip_prefix('(')
            .and_then(|text| text.strip_suffix(')'))
        else {
            return type_text;
        };
        let mut depth = 0;
        let mut encloses_all = true;
        for (index, character) in type_text.char_indices() {
            match character {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 && index < type_text.len() - 1 {
                encloses_all = false;
                break;
            }
        }
        if !encloses_all {
            return type_text;
        }
        type_text = stripped.trim();
    }
}

fn has_top_level_function_arrow(type_text: &str) -> bool {
    let mut angle_depth: usize = 0;
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    let chars = type_text.chars().collect::<Vec<_>>();

    for (index, character) in chars.iter().copied().enumerate() {
        match character {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '-' if chars.get(index + 1) == Some(&'>')
                && angle_depth == 0
                && paren_depth == 0
                && bracket_depth == 0 =>
            {
                return true;
            }
            _ => {}
        }
    }

    false
}

fn is_collection_type(type_text: &str) -> bool {
    let name = type_text
        .split_once('<')
        .map_or(type_text, |(name, _)| name);
    matches!(
        name,
        "List"
            | "Map"
            | "Set"
            | "Collection"
            | "Iterable"
            | "Iterator"
            | "MutableList"
            | "MutableMap"
            | "MutableSet"
            | "MutableCollection"
            | "MutableIterable"
            | "MutableIterator"
            | "Array"
            | "Sequence"
            | "ByteArray"
            | "ShortArray"
            | "IntArray"
            | "LongArray"
            | "FloatArray"
            | "DoubleArray"
            | "BooleanArray"
            | "CharArray"
            | "UByteArray"
            | "UShortArray"
            | "UIntArray"
            | "ULongArray"
    )
}

fn parameter_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "simple_identifier" | "identifier") {
            return child.utf8_text(source).ok().map(str::to_owned);
        }
    }
    None
}

fn parameter_type_text(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    let mut saw_colon = false;
    for child in node.children(&mut cursor) {
        if child.kind() == ":" {
            saw_colon = true;
            continue;
        }
        if saw_colon {
            return child
                .utf8_text(source)
                .ok()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned);
        }
    }
    None
}

fn simple_type_name(type_text: &str) -> Option<&str> {
    let trimmed = type_text.trim();
    if trimmed.contains("->") || trimmed.contains('<') || trimmed.contains('.') {
        return None;
    }
    let name = trimmed
        .trim_end_matches('?')
        .split_whitespace()
        .next()
        .unwrap_or_default();
    if name.is_empty() { None } else { Some(name) }
}

fn is_builtin_kotlin_type(name: &str) -> bool {
    matches!(
        name,
        "Any"
            | "Boolean"
            | "Byte"
            | "Char"
            | "Double"
            | "Float"
            | "Int"
            | "Long"
            | "Number"
            | "Nothing"
            | "Short"
            | "String"
            | "UByte"
            | "UInt"
            | "ULong"
            | "UShort"
            | "Unit"
            | "List"
            | "Map"
            | "Set"
            | "MutableList"
            | "MutableMap"
            | "MutableSet"
            | "Array"
            | "Sequence"
            | "Pair"
            | "Triple"
    )
}

fn local_type_names(root: tree_sitter::Node<'_>, source: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "class_declaration"
                | "object_declaration"
                | "type_alias"
                | "enum_class_body"
                | "interface_declaration"
        ) {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source).ok())
            {
                names.insert(name.to_owned());
            } else {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if matches!(child.kind(), "simple_identifier" | "identifier")
                        && let Ok(name) = child.utf8_text(source)
                    {
                        names.insert(name.to_owned());
                        break;
                    }
                }
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
    names
}

fn local_enum_values(root: tree_sitter::Node<'_>, source: &[u8]) -> BTreeMap<String, Vec<String>> {
    let mut enums = BTreeMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "class_declaration"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "source_file")
            && is_enum_class(node, source)
            && let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source).ok())
        {
            let values = node
                .children(&mut node.walk())
                .find(|child| child.kind() == "enum_class_body")
                .map(|body| {
                    let mut body_cursor = body.walk();
                    body.named_children(&mut body_cursor)
                        .filter(|child| child.kind() == "enum_entry")
                        .filter_map(|entry| {
                            let mut entry_cursor = entry.walk();
                            entry
                                .named_children(&mut entry_cursor)
                                .find(|child| {
                                    matches!(child.kind(), "identifier" | "simple_identifier")
                                })
                                .and_then(|identifier| identifier.utf8_text(source).ok())
                                .map(str::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            enums.insert(name.to_owned(), values);
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
    enums
}

fn is_enum_class(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "enum_class_body" {
            return true;
        }
        if child.kind() != "modifiers" {
            continue;
        }
        let mut modifier_cursor = child.walk();
        if child.named_children(&mut modifier_cursor).any(|modifier| {
            modifier.kind() == "class_modifier"
                && modifier.utf8_text(source).is_ok_and(|text| text == "enum")
        }) {
            return true;
        }
    }
    false
}

fn kdoc_for_declaration(node: tree_sitter::Node<'_>, source: &[u8]) -> KDoc {
    let Some(comment) = previous_block_comment(node, source) else {
        return KDoc::default();
    };
    parse_kdoc(comment)
}

/// Start line for source evidence, including leading annotations that tree-sitter
/// sometimes emits as siblings (`annotated_expression`) instead of `modifiers`.
fn declaration_start_line(node: tree_sitter::Node<'_>) -> u32 {
    let mut start_row = node.start_position().row;
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        if is_detachable_leading_annotation(sibling) {
            start_row = sibling.start_position().row;
            previous = sibling.prev_sibling();
            continue;
        }
        break;
    }
    u32::try_from(start_row + 1).unwrap_or(u32::MAX)
}

fn previous_block_comment<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "block_comment" => {
                let text = sibling.utf8_text(source).ok()?;
                if text.starts_with("/**") {
                    return Some(text);
                }
                return None;
            }
            // Skip line comments and leading annotations that the grammar detached
            // from the declaration's modifiers list (e.g. `@OptIn` / `@Deprecated`
            // before `@Composable` in overload or extension shapes).
            "line_comment" => {
                previous = sibling.prev_sibling();
            }
            kind if is_detachable_leading_annotation_kind(kind) => {
                previous = sibling.prev_sibling();
            }
            _ => return None,
        }
    }
    None
}

fn is_detachable_leading_annotation(node: tree_sitter::Node<'_>) -> bool {
    is_detachable_leading_annotation_kind(node.kind())
}

fn is_detachable_leading_annotation_kind(kind: &str) -> bool {
    matches!(kind, "annotated_expression" | "annotation")
}

fn parse_kdoc(text: &str) -> KDoc {
    let lines = text
        .trim()
        .trim_start_matches("/**")
        .trim_end_matches("*/")
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix('*')
                .map(str::trim)
                .unwrap_or(trimmed)
                .to_owned()
        })
        .collect::<Vec<_>>();

    let mut description = Vec::new();
    let mut deprecation = None;
    let mut fenced = Vec::new();
    let mut in_fence = false;
    let mut parameter_descriptions: BTreeMap<String, String> = BTreeMap::new();
    let mut active_parameter = None;
    let mut ignoring_parameter_continuation = false;
    let mut best_practices = Vec::new();

    for line in lines {
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            if !line.is_empty() {
                fenced.push(line);
            }
            continue;
        }
        if let Some(message) = kdoc_tag_value(&line, "@deprecated") {
            active_parameter = None;
            ignoring_parameter_continuation = false;
            deprecation = Some(RegistryDeprecation {
                message: non_empty(message.trim()),
                since: None,
                replaced_by: None,
                extra: Map::new(),
            });
        } else if let Some(parameter) = kdoc_tag_value(&line, "@param") {
            let parameter = parameter.trim();
            let (name, text) = parameter
                .split_once(char::is_whitespace)
                .map_or((parameter, ""), |(name, text)| (name, text.trim()));
            let already_documented = parameter_descriptions
                .get(name)
                .is_some_and(|description| !description.is_empty());
            active_parameter = (!name.is_empty() && !already_documented).then(|| name.to_owned());
            ignoring_parameter_continuation = already_documented;
            if !text.is_empty() && !already_documented {
                parameter_descriptions.insert(name.to_owned(), text.to_owned());
            }
        } else if let Some(guidance) = kdoc_tag_value(&line, "@guidance") {
            active_parameter = None;
            ignoring_parameter_continuation = false;
            let guidance = guidance
                .trim()
                .strip_prefix('-')
                .map_or_else(|| guidance.trim(), |without_dash| without_dash.trim());
            if !guidance.is_empty() {
                best_practices.push(RegistryBestPractice {
                    description: guidance.to_owned(),
                    extra: Map::new(),
                });
            }
        } else if line.starts_with('@') {
            active_parameter = None;
            ignoring_parameter_continuation = false;
            // Ignore KDoc tags for description text. `@sample` is a file reference
            // resolved in a later phase, not embedded example body.
        } else if !line.is_empty() {
            if let Some(name) = &active_parameter {
                parameter_descriptions
                    .entry(name.clone())
                    .and_modify(|description| {
                        description.push(' ');
                        description.push_str(&line);
                    })
                    .or_insert(line);
            } else if !ignoring_parameter_continuation {
                description.push(line);
            }
        }
    }

    KDoc {
        documentation: non_empty(description.join("\n").trim()),
        parameter_descriptions,
        best_practices,
        deprecation,
        example: non_empty(fenced.join("\n").trim()),
    }
}

fn kdoc_tag_value<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let value = line.strip_prefix(tag)?;
    (value.is_empty() || value.chars().next().is_some_and(char::is_whitespace)).then_some(value)
}

fn deprecated_annotation(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<RegistryDeprecation> {
    let annotation = annotation_with_name(node, source, "Deprecated")?;
    let message = annotation_string_argument(annotation, source, 0);
    let replaced_by = annotation_replace_with(annotation, source);
    Some(RegistryDeprecation {
        message,
        since: None,
        replaced_by,
        extra: Map::new(),
    })
}

fn annotation_string_argument(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    index: usize,
) -> Option<String> {
    let mut strings = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(current.kind(), "string_literal" | "line_string_literal") {
            if let Ok(text) = current.utf8_text(source) {
                let unquoted = text.trim().trim_matches('"').trim_matches('\'').to_owned();
                if !unquoted.is_empty() {
                    strings.push(unquoted);
                }
            }
            continue;
        }
        for child_index in (0..current.child_count()).rev() {
            if let Some(child) = current.child(child_index) {
                stack.push(child);
            }
        }
    }
    strings.get(index).cloned()
}

fn annotation_replace_with(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if (current.kind() == "call_expression" || current.kind() == "constructor_invocation")
            && let Ok(text) = current.utf8_text(source)
            && text.starts_with("ReplaceWith")
        {
            return annotation_string_argument(current, source, 0);
        }
        for child_index in (0..current.child_count()).rev() {
            if let Some(child) = current.child(child_index) {
                stack.push(child);
            }
        }
    }
    None
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
            let declaration_path_damaged = declaration
                .details
                .source_evidence
                .as_ref()
                .is_some_and(|evidence| damaged_paths.contains(&evidence.path));
            if declaration_path_damaged {
                // Clean-range hits in a damaged file are incomplete evidence: keep prior
                // facts and mark them stale instead of trusting a full replacement.
                mark_details_stale(&mut component.details);
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
            mark_details_stale(&mut component.details);
            continue;
        }

        // Unrelated parse damage is incomplete evidence: preserve prior facts instead of
        // generating removals. Only emit unavailable when the scan had no damage.
        if component.details.is_some() && !damaged_paths.is_empty() {
            continue;
        }

        component.details = Some(unavailable(
            "source_evidence",
            "no matching directly declared Compose declaration was observed",
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
                        .is_none_or(|language| matches!(language, "compose" | "kotlin"))
            })
        });
        let Some(source) = declaration else {
            continue;
        };
        if damaged_paths.contains(&source.path) {
            mark_details_stale(&mut token.details);
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

fn mark_details_stale(details: &mut Option<RegistryDetails>) {
    if let Some(details) = details {
        details.status = RegistryExtractionStatus::Stale;
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
