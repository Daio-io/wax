//! Literal custom-property references in CSS declarations.

use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::{Node, Parser, Point};
use wax_contract::{Diagnostic, DiagnosticSeverity, SourceLocation, TokenSite};
use wax_lang_api::{TokenMatch, normalize_repo_relative_path};

use crate::diagnostics::PARSE_FAILED;
use crate::swc_parse::ReactParseError;

pub(crate) fn css_parser() -> Result<Parser, ReactParseError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_css::LANGUAGE.into())
        .map_err(ReactParseError::CssLanguage)?;
    Ok(parser)
}

pub(crate) fn collect_css_token_sites(
    parser: &mut Parser,
    repo_root: &Path,
    relative_path: &Path,
    matches: &BTreeMap<String, TokenMatch>,
    sites: &mut Vec<TokenSite>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), ReactParseError> {
    let file_path = repo_root.join(relative_path);
    let source = std::fs::read_to_string(&file_path).map_err(|source| ReactParseError::Io {
        context: format!("read CSS source file {}", file_path.display()),
        source,
    })?;
    let tree = parser
        .parse(&source, None)
        .ok_or(ReactParseError::CssNoTree)?;
    let file = normalize_repo_relative_path(relative_path);
    if tree.root_node().has_error() {
        let point = first_error(tree.root_node()).start_position();
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: PARSE_FAILED.to_owned(),
            message: "failed to parse CSS file cleanly; affected declarations skipped".to_owned(),
            location: Some(source_location(&file, point)),
        });
    }
    let mut nodes = vec![(tree.root_node(), false)];
    while let Some((node, in_declaration)) = nodes.pop() {
        if node.is_error()
            || node.is_missing()
            || (node.kind() == "declaration" && node.has_error())
        {
            continue;
        }
        if in_declaration
            && node.kind() == "call_expression"
            && let Some(name) = referenced_property(node, &source)
            && let Some(matched) = matches.get(name)
        {
            let location = source_location(&file, node.start_position());
            let column = node.start_position().column.saturating_add(1);
            sites.push(TokenSite {
                id: format!(
                    "token.react:{file}:{}:{column}:{}",
                    location.line, matched.token_id
                ),
                location,
                token_id: matched.token_id.clone(),
                key: matched.key.clone(),
                category: matched.category,
                parent: None,
            });
        }
        let mut cursor = node.walk();
        nodes.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, in_declaration || node.kind() == "declaration")),
        );
    }
    Ok(())
}

fn referenced_property<'a>(call: Node<'_>, source: &'a str) -> Option<&'a str> {
    let function = call.named_child(0)?;
    if !source[function.byte_range()].eq_ignore_ascii_case("var") {
        return None;
    }
    let args = call.named_child(1)?;
    let mut cursor = args.walk();
    let mut first_argument = true;
    let mut name = None;
    for child in args.children(&mut cursor) {
        match child.kind() {
            ";" => return None,
            "," | ")" => first_argument = false,
            "(" | "comment" => {}
            _ if !first_argument => {}
            "plain_value" if name.is_none() => name = Some(&source[child.byte_range()]),
            _ => return None,
        }
    }
    name
}

fn source_location(file: &str, point: Point) -> SourceLocation {
    SourceLocation {
        file: file.to_owned(),
        line: u32::try_from(point.row.saturating_add(1)).unwrap_or(u32::MAX),
        column: Some(u32::try_from(point.column.saturating_add(1)).unwrap_or(u32::MAX)),
        root_group: None,
    }
}

/// Finds the first ERROR or MISSING node in document order.
fn first_error(root: Node<'_>) -> Node<'_> {
    let mut node = root;
    'descend: loop {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_error() || child.is_missing() {
                return child;
            }
            if child.has_error() {
                node = child;
                continue 'descend;
            }
        }
        return node;
    }
}
