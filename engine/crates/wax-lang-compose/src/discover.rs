//! Compose registry symbol discovery.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wax_contract::{Diagnostic, DiagnosticSeverity, SourceLocation};

use crate::kotlin_ast::{
    ParseKotlinFileError, collect_kotlin_files, function_name_from_decl, has_composable_annotation,
    new_parser, parse_kotlin_file_strict,
};

/// Result of discovering Compose registry symbols from Kotlin source roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverRegistryResult {
    /// Discovered design-system symbol names.
    pub symbols: Vec<String>,
    /// Structured diagnostics emitted while discovering symbols.
    pub diagnostics: Vec<Diagnostic>,
}

/// Errors produced while discovering Compose registry symbols.
#[derive(Debug)]
pub enum ComposeDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
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
            Self::InvalidLanguageId(id) => write!(f, "invalid compose language id: {id}"),
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
            Self::InvalidLanguageId(_) | Self::MissingRoot(_) | Self::ParserInitFailed(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Discovers likely public top-level Compose component symbols from Kotlin source roots.
///
/// Files that fail to parse are skipped and reported as diagnostics so discovery can
/// continue with the remaining Kotlin sources.
pub fn discover_registry_symbols(
    parse_root: &Path,
    roots: &[PathBuf],
) -> Result<DiscoverRegistryResult, ComposeDiscoverError> {
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
    let mut diagnostics = Vec::new();
    for file_path in kotlin_files {
        match parse_kotlin_file_strict(&mut parser, &file_path) {
            Ok(parsed) => collect_symbols(
                parsed.tree.root_node(),
                parsed.source.as_bytes(),
                &mut symbols,
            ),
            Err(ParseKotlinFileError::ParseFailed(_)) => {
                let relative_file = repo_relative_path(parse_root, &file_path);
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    code: "parse_failed".to_owned(),
                    message: format!("tree-sitter failed to parse {relative_file}; file skipped"),
                    location: Some(SourceLocation {
                        file: relative_file,
                        line: 1,
                        column: None,
                    }),
                });
            }
            Err(ParseKotlinFileError::Io { context, source }) => {
                return Err(ComposeDiscoverError::Io { context, source });
            }
        }
    }

    Ok(DiscoverRegistryResult {
        symbols: symbols.into_iter().collect(),
        diagnostics,
    })
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

fn repo_relative_path(parse_root: &Path, file_path: &Path) -> String {
    file_path
        .strip_prefix(parse_root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| {
            file_path
                .file_name()
                .map(|name| name.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| file_path.to_string_lossy().replace('\\', "/"))
        })
}
