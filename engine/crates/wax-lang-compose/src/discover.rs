//! Compose registry symbol discovery.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wax_contract::{Diagnostic, DiagnosticSeverity};
use wax_lang_api::DiscoveredRegistrySymbol;

use crate::kotlin_ast::{
    ParseKotlinFileError, collect_kotlin_files, function_name_from_decl, has_composable_annotation,
    new_parser, package_name_from_source, parse_kotlin_file_permissive,
    partial_tree_parse_diagnostic, tree_has_syntax_errors, unparseable_file_diagnostic,
};

/// Result of discovering Compose registry symbols from Kotlin source roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverRegistryResult {
    /// Discovered design-system symbols with optional package identity.
    pub components: Vec<DiscoveredRegistrySymbol>,
    /// Structured diagnostics emitted while discovering symbols.
    pub diagnostics: Vec<Diagnostic>,
}

impl DiscoverRegistryResult {
    /// Returns discovered symbol names in stable order.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        DiscoveredRegistrySymbol::symbol_names(&self.components)
    }
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
/// Files that tree-sitter cannot parse at all are skipped and reported as diagnostics.
/// Recoverable syntax errors still allow symbol extraction from the partial tree.
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

    let mut components = BTreeMap::<String, Option<String>>::new();
    let mut diagnostics = Vec::new();
    for file_path in kotlin_files {
        match parse_kotlin_file_permissive(&mut parser, &file_path) {
            Ok(parsed) => {
                let package =
                    package_name_from_source(parsed.tree.root_node(), parsed.source.as_bytes());
                collect_symbols(
                    parsed.tree.root_node(),
                    parsed.source.as_bytes(),
                    package,
                    &mut components,
                    &mut diagnostics,
                );
                if tree_has_syntax_errors(&parsed.tree) {
                    let relative_file = repo_relative_path(parse_root, &file_path);
                    diagnostics.push(partial_tree_parse_diagnostic(
                        parsed.tree.root_node(),
                        &relative_file,
                    ));
                }
            }
            Err(ParseKotlinFileError::ParseFailed(_)) => {
                let relative_file = repo_relative_path(parse_root, &file_path);
                diagnostics.push(unparseable_file_diagnostic(&relative_file));
            }
            Err(ParseKotlinFileError::Io { context, source }) => {
                return Err(ComposeDiscoverError::Io { context, source });
            }
        }
    }

    Ok(DiscoverRegistryResult {
        components: components
            .into_iter()
            .map(|(symbol, package)| DiscoveredRegistrySymbol::new(symbol, package))
            .collect(),
        diagnostics,
    })
}

fn collect_symbols(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    package: Option<String>,
    components: &mut BTreeMap<String, Option<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_declaration"
            && is_top_level_declaration(node)
            && has_composable_annotation(node, source)
            && is_public(node, source)
            && let Some((name, _)) = function_name_from_decl(node, source)
            && name.starts_with(|c: char| c.is_ascii_uppercase())
        {
            insert_discovered_symbol(components, diagnostics, name, package.clone());
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
}

fn insert_discovered_symbol(
    components: &mut BTreeMap<String, Option<String>>,
    diagnostics: &mut Vec<Diagnostic>,
    symbol: String,
    package: Option<String>,
) {
    if let Some(existing) = components.get(&symbol) {
        if existing.as_ref() != package.as_ref()
            && existing.is_some()
            && package.is_some()
        {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "discover_package_conflict".to_owned(),
                message: format!(
                    "symbol '{symbol}' was discovered in multiple packages; omitting package metadata"
                ),
                location: None,
            });
            components.insert(symbol, None);
        }
        return;
    }

    components.insert(symbol, package);
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
        .map(|relative| relative.to_string_lossy().replace("\\", "/"))
        .unwrap_or_else(|_| {
            file_path
                .file_name()
                .map(|name| name.to_string_lossy().replace("\\", "/"))
                .unwrap_or_else(|| file_path.to_string_lossy().replace("\\", "/"))
        })
}
