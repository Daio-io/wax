//! Compose registry symbol discovery.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::kotlin_ast::{
    ParseKotlinFileError, collect_kotlin_files, function_name_from_decl, has_composable_annotation,
    new_parser, parse_kotlin_file_strict,
};

/// Errors produced while discovering Compose registry symbols.
#[derive(Debug)]
pub enum ComposeDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// A configured discovery root does not exist.
    MissingRoot(PathBuf),
    /// A Kotlin file could not be parsed successfully.
    ParseFailed(PathBuf),
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
            Self::InvalidLanguageId(id) => write!(f, "invalid compose language id: {id}"),
            Self::MissingRoot(path) => {
                write!(f, "discovery root does not exist: {}", path.display())
            }
            Self::ParseFailed(path) => {
                write!(f, "failed to parse Kotlin source {}", path.display())
            }
            Self::ParserInitFailed(reason) => write!(f, "parser init failed: {reason}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ComposeDiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_)
            | Self::MissingRoot(_)
            | Self::ParseFailed(_)
            | Self::ParserInitFailed(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Discovers likely public top-level Compose component symbols from Kotlin source roots.
pub fn discover_registry_symbols(roots: &[PathBuf]) -> Result<Vec<String>, ComposeDiscoverError> {
    let mut parser = new_parser().map_err(ComposeDiscoverError::ParserInitFailed)?;

    let mut kotlin_files = Vec::new();
    for root in roots {
        if !root.exists() {
            return Err(ComposeDiscoverError::MissingRoot(root.clone()));
        }
        collect_kotlin_files(root, &mut kotlin_files).map_err(|source| {
            ComposeDiscoverError::Io {
                context: format!("read Kotlin root {}", root.display()),
                source,
            }
        })?;
    }
    kotlin_files.sort();

    let mut symbols = BTreeSet::new();
    for file_path in kotlin_files {
        let parsed = parse_kotlin_file_strict(&mut parser, &file_path).map_err(map_parse_error)?;
        collect_symbols(
            parsed.tree.root_node(),
            parsed.source.as_bytes(),
            &mut symbols,
        );
    }

    Ok(symbols.into_iter().collect())
}

fn collect_symbols(root: tree_sitter::Node<'_>, source: &[u8], symbols: &mut BTreeSet<String>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_declaration"
            && is_top_level_declaration(node)
            && has_composable_annotation(node, source)
            && is_public(node, source)
            && let Some((name, _)) = function_name_from_decl(node, source)
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

fn map_parse_error(err: ParseKotlinFileError) -> ComposeDiscoverError {
    match err {
        ParseKotlinFileError::Io { context, source } => {
            ComposeDiscoverError::Io { context, source }
        }
        ParseKotlinFileError::ParseFailed(path) => ComposeDiscoverError::ParseFailed(path),
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
