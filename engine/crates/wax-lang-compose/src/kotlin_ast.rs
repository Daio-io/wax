//! Shared Kotlin tree-sitter helpers for Compose extraction.

use std::fs;
use std::path::{Path, PathBuf};

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

pub(crate) fn parse_kotlin_file_strict(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedKotlinFile, ParseKotlinFileError> {
    let parsed = parse_kotlin_file_permissive(parser, path)?;
    if parsed.tree.root_node().has_error() {
        return Err(ParseKotlinFileError::ParseFailed(path.to_path_buf()));
    }

    Ok(parsed)
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
