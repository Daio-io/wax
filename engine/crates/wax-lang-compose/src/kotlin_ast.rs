//! Shared Kotlin tree-sitter helpers for Compose extraction.

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
    let tree = parser
        .parse(source.as_bytes(), None)
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

pub(crate) fn has_composable_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut modifiers_cursor = child.walk();
            for modifier in child.named_children(&mut modifiers_cursor) {
                if modifier.kind() == "annotation"
                    && annotation_type_name(modifier, source).as_deref() == Some("Composable")
                {
                    return true;
                }
            }
        }
    }
    false
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
}
