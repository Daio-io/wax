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
        .set_language(&tree_sitter_kotlin::language())
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

pub(crate) fn parse_kotlin_file(
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
    if tree.root_node().has_error() {
        return Err(ParseKotlinFileError::ParseFailed(path.to_path_buf()));
    }

    Ok(ParsedKotlinFile { source, tree })
}

pub(crate) fn annotation_type_name(
    annotation: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<String> {
    let mut cursor = annotation.walk();
    for child in annotation.named_children(&mut cursor) {
        if child.kind() == "user_type" {
            let mut type_cursor = child.walk();
            let mut last_type_identifier = None;
            for type_child in child.named_children(&mut type_cursor) {
                if type_child.kind() == "type_identifier" {
                    last_type_identifier = type_child.utf8_text(source).ok().map(str::to_owned);
                }
            }
            return last_type_identifier;
        }
    }
    None
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
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "simple_identifier" {
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
    let first = node.named_children(&mut cursor).next()?;
    if first.kind() == "simple_identifier" {
        let name = first.utf8_text(source).ok()?.to_owned();
        Some((name, first.start_position()))
    } else {
        None
    }
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
    fn parse_kotlin_file_reports_syntax_errors() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let broken_file = tempdir.path().join("Broken.kt");
        fs::write(&broken_file, "@Composable\nfun Broken(").expect("write broken source");

        let mut parser = new_parser().expect("parser");
        let err = parse_kotlin_file(&mut parser, &broken_file).expect_err("parse should fail");

        assert!(matches!(err, ParseKotlinFileError::ParseFailed(path) if path == broken_file));
    }
}
