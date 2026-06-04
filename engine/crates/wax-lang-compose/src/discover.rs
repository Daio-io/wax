//! Compose registry symbol discovery.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Errors produced while discovering Compose registry symbols.
#[derive(Debug)]
pub enum ComposeDiscoverError {
    /// A configured discovery root does not exist.
    MissingRoot(PathBuf),
    /// Tree-sitter parser failed to initialize.
    ParserInitFailed(String),
    /// A filesystem operation failed.
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for ComposeDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRoot(path) => {
                write!(f, "discovery root does not exist: {}", path.display())
            }
            Self::ParserInitFailed(reason) => write!(f, "parser init failed: {reason}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ComposeDiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingRoot(_) | Self::ParserInitFailed(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Discovers likely public top-level Compose component symbols from Kotlin source roots.
pub fn discover_registry_symbols(roots: &[PathBuf]) -> Result<Vec<String>, ComposeDiscoverError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin::language())
        .map_err(|err| ComposeDiscoverError::ParserInitFailed(err.to_string()))?;

    let mut kotlin_files = Vec::new();
    for root in roots {
        if !root.exists() {
            return Err(ComposeDiscoverError::MissingRoot(root.clone()));
        }
        collect_kotlin_files(root, &mut kotlin_files)?;
    }
    kotlin_files.sort();

    let mut symbols = BTreeSet::new();
    for file_path in kotlin_files {
        let source = fs::read_to_string(&file_path).map_err(|source| ComposeDiscoverError::Io {
            context: format!("read Kotlin source {}", file_path.display()),
            source,
        })?;

        if let Some(tree) = parser.parse(source.as_bytes(), None) {
            collect_symbols(tree.root_node(), source.as_bytes(), &mut symbols);
        }
    }

    Ok(symbols.into_iter().collect())
}

fn collect_kotlin_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ComposeDiscoverError> {
    let entries = fs::read_dir(dir).map_err(|source| ComposeDiscoverError::Io {
        context: format!("read Kotlin root {}", dir.display()),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| ComposeDiscoverError::Io {
            context: format!("read Kotlin root entry {}", dir.display()),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_kotlin_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "kt") {
            files.push(path);
        }
    }

    Ok(())
}

fn collect_symbols(root: tree_sitter::Node<'_>, source: &[u8], symbols: &mut BTreeSet<String>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_declaration"
            && is_top_level_declaration(node)
            && has_composable_annotation(node, source)
            && is_public(node, source)
            && let Some(name) = function_name(node, source)
            && name.starts_with(|c: char| c.is_ascii_uppercase())
        {
            symbols.insert(name);
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
}

fn is_top_level_declaration(node: tree_sitter::Node<'_>) -> bool {
    let mut parent = node.parent();
    while let Some(current) = parent {
        match current.kind() {
            "source_file" => return true,
            "class_declaration" | "object_declaration" | "function_declaration" => return false,
            _ => parent = current.parent(),
        }
    }
    false
}

fn has_composable_annotation(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
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

fn annotation_type_name(annotation: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
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

fn is_public(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "modifiers" {
            let mut modifiers_cursor = child.walk();
            for modifier in child.named_children(&mut modifiers_cursor) {
                if modifier.kind() == "visibility_modifier"
                    && let Ok(visibility) = modifier.utf8_text(source)
                    && matches!(visibility, "private" | "internal")
                {
                    return false;
                }
            }
        }
    }
    true
}

fn function_name(node: tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "simple_identifier" {
            return child.utf8_text(source).ok().map(str::to_owned);
        }
    }
    None
}
