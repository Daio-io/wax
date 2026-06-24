//! Shared Kotlin tree-sitter helpers for Compose extraction.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{Diagnostic, DiagnosticSeverity, SourceLocation};

/// Parsed Kotlin source and syntax tree.
#[derive(Debug)]
pub(crate) struct ParsedKotlinFile {
    pub(crate) source: String,
    pub(crate) tree: tree_sitter::Tree,
}

/// Errors produced while reading or parsing Kotlin source.
#[derive(Debug)]
pub(crate) enum ParseKotlinFileError {
    Io {
        context: String,
        source: std::io::Error,
    },
    ParseFailed(PathBuf),
}

impl std::fmt::Display for ParseKotlinFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::ParseFailed(path) => {
                write!(f, "failed to parse Kotlin source {}", path.display())
            }
        }
    }
}

impl std::error::Error for ParseKotlinFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ParseFailed(_) => None,
        }
    }
}

pub(crate) fn new_parser() -> Result<tree_sitter::Parser, String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .map_err(|err| err.to_string())?;
    Ok(parser)
}

/// Returns the Kotlin `package` declaration from a parsed source file, when present.
pub(crate) fn package_name_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<String> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "package_header" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() == "qualified_identifier" {
                    return child.utf8_text(source).ok().map(str::to_owned);
                }
            }
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }

    None
}

pub(crate) fn collect_kotlin_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_kotlin_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "kt") {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn parse_kotlin_file_permissive(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedKotlinFile, ParseKotlinFileError> {
    let source = fs::read_to_string(path).map_err(|source| ParseKotlinFileError::Io {
        context: format!("read Kotlin source {}", path.display()),
        source,
    })?;
    let normalized = normalize_annotated_parenthesized_function_types_for_parse(&source);
    let tree = parser
        .parse(normalized.as_bytes(), None)
        .ok_or_else(|| ParseKotlinFileError::ParseFailed(path.to_path_buf()))?;

    Ok(ParsedKotlinFile { source, tree })
}

#[allow(dead_code)]
pub(crate) fn parse_kotlin_file_strict(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedKotlinFile, ParseKotlinFileError> {
    let parsed = parse_kotlin_file_permissive(parser, path)?;
    if tree_has_syntax_errors(&parsed.tree) {
        return Err(ParseKotlinFileError::ParseFailed(path.to_path_buf()));
    }

    Ok(parsed)
}

/// Returns whether tree-sitter reported recoverable syntax errors in a parsed tree.
pub(crate) fn tree_has_syntax_errors(tree: &tree_sitter::Tree) -> bool {
    tree.root_node().has_error()
}

fn normalize_annotated_parenthesized_function_types_for_parse(source: &str) -> Cow<'_, str> {
    let bytes = source.as_bytes();
    let mut normalized = None;
    let mut index = 0;
    let mut paren_stack = Vec::new();

    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_block_comment(bytes, index + 2);
            }
            b'"' if bytes.get(index + 1) == Some(&b'"') && bytes.get(index + 2) == Some(&b'"') => {
                index = skip_triple_quoted_string(bytes, index + 3);
            }
            b'"' => {
                index = skip_quoted_literal(bytes, index + 1, b'"');
            }
            b'\'' => {
                index = skip_quoted_literal(bytes, index + 1, b'\'');
            }
            b'(' => {
                paren_stack.push(index);
                index += 1;
            }
            b')' => {
                paren_stack.pop();
                index += 1;
            }
            b':' => {
                let Some((outer_open, outer_close)) =
                    parameter_annotation_function_type_parens(bytes, index, paren_stack.last())
                else {
                    index += 1;
                    continue;
                };

                let normalized_bytes = normalized.get_or_insert_with(|| bytes.to_vec());
                normalized_bytes[outer_open] = b' ';
                normalized_bytes[outer_close] = b' ';
                index = outer_close + 1;
            }
            _ => index += 1,
        }
    }

    match normalized {
        Some(bytes) => String::from_utf8(bytes)
            .map(Cow::Owned)
            .unwrap_or_else(|_| Cow::Borrowed(source)),
        None => Cow::Borrowed(source),
    }
}

fn parameter_annotation_function_type_parens(
    bytes: &[u8],
    colon_index: usize,
    parameter_list_open: Option<&usize>,
) -> Option<(usize, usize)> {
    let parameter_list_open = *parameter_list_open?;
    if !is_plain_parameter_type_annotation(bytes, parameter_list_open, colon_index) {
        return None;
    }

    let mut index = skip_ascii_whitespace(bytes, colon_index + 1);
    let mut saw_annotation = false;

    while bytes.get(index) == Some(&b'@') {
        let annotation_end = annotation_token_end(bytes, index)?;
        saw_annotation = true;
        index = skip_ascii_whitespace(bytes, annotation_end);
    }

    if !saw_annotation || bytes.get(index) != Some(&b'(') || bytes.get(index + 1) != Some(&b'(') {
        return None;
    }

    let outer_close = matching_paren_index(bytes, index)?;
    (outer_close > index + 2 && contains_function_arrow(bytes, index + 1, outer_close))
        .then_some((index, outer_close))
}

fn is_plain_parameter_type_annotation(
    bytes: &[u8],
    parameter_list_open: usize,
    colon_index: usize,
) -> bool {
    let segment_start = bytes[parameter_list_open + 1..colon_index]
        .iter()
        .rposition(|byte| *byte == b',')
        .map_or(parameter_list_open + 1, |offset| {
            parameter_list_open + 1 + offset + 1
        });
    let start = skip_ascii_whitespace(bytes, segment_start);
    if starts_with_keyword(bytes, start, b"val") || starts_with_keyword(bytes, start, b"var") {
        return false;
    }

    let Some(name_start) = previous_identifier_start(bytes, colon_index) else {
        return false;
    };
    name_start >= start
}

fn starts_with_keyword(bytes: &[u8], start: usize, keyword: &[u8]) -> bool {
    bytes
        .get(start..start + keyword.len())
        .is_some_and(|found| found == keyword)
        && bytes
            .get(start + keyword.len())
            .is_none_or(|byte| !is_identifier_byte(*byte))
}

fn previous_identifier_start(bytes: &[u8], index: usize) -> Option<usize> {
    let mut cursor = index.checked_sub(1)?;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        cursor = cursor.checked_sub(1)?;
    }
    if !bytes
        .get(cursor)
        .is_some_and(|byte| is_identifier_byte(*byte))
    {
        return None;
    }
    while cursor > 0
        && bytes
            .get(cursor - 1)
            .is_some_and(|byte| is_identifier_byte(*byte))
    {
        cursor -= 1;
    }
    Some(cursor)
}

fn annotation_token_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start.checked_add(1)?;
    while index < bytes.len() && is_annotation_token_byte(bytes[index]) {
        index += 1;
    }

    (index > start + 1).then_some(index)
}

fn is_annotation_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':')
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn skip_line_comment(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

fn skip_block_comment(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 1_u32;
    while index < bytes.len() {
        match (bytes.get(index), bytes.get(index + 1)) {
            (Some(b'/'), Some(b'*')) => {
                depth = depth.saturating_add(1);
                index += 2;
            }
            (Some(b'*'), Some(b'/')) => {
                depth = depth.saturating_sub(1);
                index += 2;
                if depth == 0 {
                    break;
                }
            }
            _ => index += 1,
        }
    }
    index
}

fn skip_triple_quoted_string(bytes: &[u8], mut index: usize) -> usize {
    while index + 2 < bytes.len() {
        if bytes[index] == b'"' && bytes[index + 1] == b'"' && bytes[index + 2] == b'"' {
            return index + 3;
        }
        index += 1;
    }
    bytes.len()
}

fn skip_quoted_literal(bytes: &[u8], mut index: usize, delimiter: u8) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            byte if byte == delimiter => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn matching_paren_index(bytes: &[u8], open_index: usize) -> Option<usize> {
    let mut depth = 0_u32;
    for (index, byte) in bytes.iter().enumerate().skip(open_index) {
        match byte {
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn contains_function_arrow(bytes: &[u8], start: usize, end: usize) -> bool {
    if end <= start + 1 {
        return false;
    }

    let mut index = start;
    while index + 1 < end {
        if bytes[index] == b'-' && bytes[index + 1] == b'>' {
            return true;
        }
        index += 1;
    }

    false
}

/// Diagnostic emitted when tree-sitter returns no syntax tree for a source file.
pub(crate) fn unparseable_file_diagnostic(relative_file: &str) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: "parse_failed".to_owned(),
        message: format!("tree-sitter failed to parse {relative_file}; file skipped"),
        location: Some(SourceLocation {
            file: relative_file.to_owned(),
            line: 1,
            column: None,
        }),
    }
}

/// Diagnostic emitted when tree-sitter recovers a partial tree with syntax errors.
pub(crate) fn partial_tree_parse_diagnostic(
    root: tree_sitter::Node<'_>,
    relative_file: &str,
) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: "parse_failed".to_owned(),
        message: format!(
            "tree-sitter reported syntax errors in {relative_file}; file scanned with gaps"
        ),
        location: first_syntax_error_location(root, relative_file),
    }
}

fn first_syntax_error_location(
    root: tree_sitter::Node<'_>,
    relative_file: &str,
) -> Option<SourceLocation> {
    let node = first_error_node(root)?;
    let start = node.start_position();
    Some(SourceLocation {
        file: relative_file.to_owned(),
        line: u32::try_from(start.row.saturating_add(1)).unwrap_or(u32::MAX),
        column: Some(u32::try_from(start.column.saturating_add(1)).unwrap_or(u32::MAX)),
    })
}

fn first_error_node(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_error_node(child) {
            return Some(found);
        }
    }

    None
}

pub(crate) fn annotation_type_name(
    annotation: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<String> {
    let mut cursor = annotation.walk();
    for child in annotation.named_children(&mut cursor) {
        match child.kind() {
            "user_type" => return last_type_name_segment(child, source),
            "type" => {
                let mut type_cursor = child.walk();
                for type_child in child.named_children(&mut type_cursor) {
                    if type_child.kind() == "user_type" {
                        return last_type_name_segment(type_child, source);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn last_type_name_segment(user_type: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = user_type.walk();
    let mut last_type_identifier = None;
    for type_child in user_type.named_children(&mut cursor) {
        if matches!(type_child.kind(), "identifier" | "type_identifier") {
            last_type_identifier = type_child.utf8_text(source).ok().map(str::to_owned);
        }
    }
    last_type_identifier
}

fn has_annotation_named(node: tree_sitter::Node<'_>, source: &[u8], expected: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut modifiers_cursor = child.walk();
            for modifier in child.named_children(&mut modifiers_cursor) {
                if modifier.kind() == "annotation"
                    && annotation_type_name(modifier, source).as_deref() == Some(expected)
                {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn has_composable_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    has_annotation_named(node, source, "Composable")
}

pub(crate) fn has_preview_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    has_annotation_named(node, source, "Preview")
}

pub(crate) fn function_name_from_decl(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = name_node.utf8_text(source).ok()?.to_owned();
        return Some((name, name_node.start_position()));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "simple_identifier" | "identifier") {
            let name = child.utf8_text(source).ok()?.to_owned();
            return Some((name, child.start_position()));
        }
    }
    None
}

/// Import bindings collected from Kotlin `import` declarations in one source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ImportBindings {
    /// Maps local symbol names to the package prefix they were imported from.
    pub symbol_packages: BTreeMap<String, String>,
    /// Package prefixes imported with a wildcard (`import com.example.*`).
    pub wildcard_packages: Vec<String>,
}

impl ImportBindings {
    /// Returns the package prefix for a symbol used at a call site, when known.
    pub(crate) fn package_for_symbol(&self, symbol: &str) -> Option<String> {
        if let Some(package) = self.symbol_packages.get(symbol) {
            return Some(package.clone());
        }

        match self.wildcard_packages.len() {
            0 => None,
            1 => Some(self.wildcard_packages[0].clone()),
            _ => None,
        }
    }
}

/// Collects import bindings from the top level of a Kotlin source file.
pub(crate) fn collect_import_bindings(
    root: tree_sitter::Node<'_>,
    source: &[u8],
) -> ImportBindings {
    let mut bindings = ImportBindings::default();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "import" {
            if let Some(import) = parse_import_directive(node, source) {
                match import {
                    ParsedImport::Named {
                        local_name,
                        package,
                    } => {
                        bindings.symbol_packages.insert(local_name, package);
                    }
                    ParsedImport::Wildcard { package } => {
                        bindings.wildcard_packages.push(package);
                    }
                }
            }
            continue;
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }

    bindings.wildcard_packages.sort();
    bindings
}

enum ParsedImport {
    Named { local_name: String, package: String },
    Wildcard { package: String },
}

fn parse_import_directive(
    import_node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<ParsedImport> {
    let qualified = import_node
        .named_children(&mut import_node.walk())
        .find(|child| child.kind() == "qualified_identifier")?;

    let qualified_name = qualified.utf8_text(source).ok()?.to_owned();
    let mut alias = None;
    let mut is_wildcard = false;

    let mut cursor = import_node.walk();
    for child in import_node.children(&mut cursor) {
        if child.kind() == "as"
            && let Some(next) = child.next_sibling()
            && (next.kind() == "identifier" || next.kind() == "simple_identifier")
        {
            alias = next.utf8_text(source).ok().map(str::to_owned);
        }
        if child.kind() == "*" || child.utf8_text(source).ok().is_some_and(|text| text == "*") {
            is_wildcard = true;
        }
    }

    if is_wildcard {
        return Some(ParsedImport::Wildcard {
            package: qualified_name,
        });
    }

    let package = package_prefix_from_qualified(&qualified_name)?;
    let local_name = alias.unwrap_or_else(|| symbol_from_qualified(&qualified_name));
    Some(ParsedImport::Named {
        local_name,
        package,
    })
}

fn package_prefix_from_qualified(qualified: &str) -> Option<String> {
    let (package, _) = qualified.rsplit_once('.')?;
    if package.is_empty() {
        None
    } else {
        Some(package.to_owned())
    }
}

fn symbol_from_qualified(qualified: &str) -> String {
    qualified
        .rsplit_once('.')
        .map_or(qualified, |(_, symbol)| symbol)
        .to_owned()
}

pub(crate) fn nearest_enclosing_composable(
    mut node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    while let Some(parent) = node.parent() {
        if parent.kind() == "function_declaration"
            && has_composable_annotation(parent, source)
            && let Some((name, pos)) = function_name_from_decl(parent, source)
            && name.starts_with(|c: char| c.is_ascii_uppercase())
        {
            return Some((name, pos));
        }
        node = parent;
    }
    None
}

pub(crate) fn is_within_preview_composable(mut node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "function_declaration"
            && has_composable_annotation(parent, source)
            && has_preview_annotation(parent, source)
        {
            return true;
        }
        node = parent;
    }

    false
}

pub(crate) fn is_pascal_case_composable_symbol(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

pub(crate) fn is_non_ui_scaffolding_composable_symbol(symbol: &str) -> bool {
    // Compose provider/effect naming convention marks dependency wiring or side effects, not UI.
    symbol.starts_with("Provide") || symbol.ends_with("Effect")
}

pub(crate) fn call_simple_callee(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "simple_identifier" | "identifier") {
            let name = child.utf8_text(source).ok()?.to_owned();
            return Some((name, child.start_position()));
        }
        if child.kind() == "expression"
            && let Some(found) = simple_identifier_from_expression(child, source)
        {
            return Some(found);
        }
    }
    None
}

fn simple_identifier_from_expression(
    node: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<(String, tree_sitter::Point)> {
    if matches!(node.kind(), "simple_identifier" | "identifier") {
        let name = node.utf8_text(source).ok()?.to_owned();
        return Some((name, node.start_position()));
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = simple_identifier_from_expression(child, source) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_import_bindings_maps_named_and_wildcard_imports() {
        let mut parser = new_parser().expect("parser");
        let source = r#"
import com.acme.designsystem.Button
import com.foundation.ui.Icon
import com.example.widgets.*
import com.example.widgets.Widget as CustomWidget

@Composable
fun Screen() {}
"#;
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let bindings = collect_import_bindings(tree.root_node(), source.as_bytes());

        assert_eq!(
            bindings.symbol_packages.get("Button"),
            Some(&"com.acme.designsystem".to_owned())
        );
        assert_eq!(
            bindings.symbol_packages.get("Icon"),
            Some(&"com.foundation.ui".to_owned())
        );
        assert_eq!(
            bindings.symbol_packages.get("CustomWidget"),
            Some(&"com.example.widgets".to_owned())
        );
        assert_eq!(
            bindings.wildcard_packages,
            vec!["com.example.widgets".to_owned()]
        );
        assert_eq!(
            bindings.package_for_symbol("Button"),
            Some("com.acme.designsystem".to_owned())
        );
        assert_eq!(
            bindings.package_for_symbol("Icon"),
            Some("com.foundation.ui".to_owned())
        );
    }

    #[test]
    fn annotation_type_name_returns_last_segment_for_qualified_names() {
        let mut parser = new_parser().expect("parser");
        let source = "@androidx.compose.runtime.Composable\nfun QualifiedCard() {}";
        let tree = parser.parse(source.as_bytes(), None).expect("tree");
        let root = tree.root_node();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "annotation" {
                assert_eq!(
                    annotation_type_name(node, source.as_bytes()).as_deref(),
                    Some("Composable")
                );
                return;
            }
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    stack.push(child);
                }
            }
        }

        panic!("annotation not found");
    }

    #[test]
    fn strict_parse_reports_syntax_errors() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let broken_file = tempdir.path().join("Broken.kt");
        fs::write(&broken_file, "@Composable\nfun Broken(").expect("write broken source");

        let mut parser = new_parser().expect("parser");
        let err = parse_kotlin_file_strict(&mut parser, &broken_file)
            .expect_err("strict parse should fail");

        assert!(matches!(err, ParseKotlinFileError::ParseFailed(path) if path == broken_file));
    }

    #[test]
    fn permissive_parse_keeps_partial_trees() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let broken_file = tempdir.path().join("Broken.kt");
        fs::write(
            &broken_file,
            "@Composable\nfun PrimaryButton() {}\nfun Broken(\n@Composable\nfun SecondaryButton() {}",
        )
        .expect("write broken source");

        let mut parser = new_parser().expect("parser");
        let parsed = parse_kotlin_file_permissive(&mut parser, &broken_file)
            .expect("permissive parse should keep partial trees");

        assert!(parsed.tree.root_node().has_error());
    }

    #[test]
    fn normalization_only_rewrites_parameter_type_annotations() {
        let source = r#"
val label = "@Composable ((T) -> Unit)"
// @Composable ((T) -> Unit)
fun Screen(
    content: @Composable ((T) -> Unit),
) {}
"#;

        let normalized = normalize_annotated_parenthesized_function_types_for_parse(source);

        assert_eq!(
            normalized.as_ref(),
            r#"
val label = "@Composable ((T) -> Unit)"
// @Composable ((T) -> Unit)
fun Screen(
    content: @Composable  (T) -> Unit ,
) {}
"#
        );
    }

    #[test]
    fn normalization_does_not_rewrite_property_or_return_type_annotations() {
        let source = r#"
val handler: @Composable ((T) -> Unit) = {}

fun factory(): @Composable ((T) -> Unit) = {}

class Screen(
    val content: @Composable ((T) -> Unit),
)
"#;

        let normalized = normalize_annotated_parenthesized_function_types_for_parse(source);

        assert_eq!(
            normalized.as_ref(),
            r#"
val handler: @Composable ((T) -> Unit) = {}

fun factory(): @Composable ((T) -> Unit) = {}

class Screen(
    val content: @Composable ((T) -> Unit),
)
"#
        );
    }
}
