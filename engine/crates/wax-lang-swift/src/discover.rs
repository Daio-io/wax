//! SwiftUI registry symbol discovery.

use std::collections::BTreeSet;
use std::path::PathBuf;

use wax_contract::{Diagnostic, DiagnosticSeverity};

use crate::component_detect::collect_component_declarations;
use crate::swift_ast::{
    ParseSwiftFileError, collect_swift_files, new_parser, parse_swift_file_strict,
};

/// Result of discovering SwiftUI registry symbols from source roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverRegistryResult {
    /// Discovered design-system symbol names.
    pub symbols: Vec<String>,
    /// Structured diagnostics emitted while discovering symbols.
    pub diagnostics: Vec<Diagnostic>,
}

/// Errors produced while discovering SwiftUI registry symbols.
#[derive(Debug)]
pub enum SwiftDiscoverError {
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

impl std::fmt::Display for SwiftDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
            Self::MissingRoot(path) => {
                write!(f, "discovery root does not exist: {}", path.display())
            }
            Self::ParserInitFailed(reason) => write!(f, "parser init failed: {reason}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for SwiftDiscoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) | Self::MissingRoot(_) | Self::ParserInitFailed(_) => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Discovers likely public top-level SwiftUI component symbols from source roots.
///
/// Files that fail to parse are skipped and reported as diagnostics so discovery can
/// continue with the remaining Swift sources.
pub fn discover_registry_symbols(
    roots: &[PathBuf],
) -> Result<DiscoverRegistryResult, SwiftDiscoverError> {
    let mut parser = new_parser().map_err(SwiftDiscoverError::ParserInitFailed)?;
    let mut swift_files = Vec::new();
    for root in roots {
        if !root.exists() {
            return Err(SwiftDiscoverError::MissingRoot(root.clone()));
        }
        collect_swift_files(root, &mut swift_files).map_err(|source| SwiftDiscoverError::Io {
            context: format!("read Swift root {}", root.display()),
            source,
        })?;
    }
    swift_files.sort();

    let mut symbols = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for file_path in swift_files {
        match parse_swift_file_strict(&mut parser, &file_path) {
            Ok(parsed) => {
                for component in collect_component_declarations(
                    parsed.tree.root_node(),
                    parsed.source.as_bytes(),
                    true,
                ) {
                    symbols.insert(component.symbol);
                }
            }
            Err(ParseSwiftFileError::ParseFailed(_)) => {
                diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::Error,
                    code: "parse_failed".to_owned(),
                    message: format!(
                        "tree-sitter failed to parse {}; file skipped",
                        file_path.display()
                    ),
                    location: None,
                });
            }
            Err(ParseSwiftFileError::Io { context, source }) => {
                return Err(SwiftDiscoverError::Io { context, source });
            }
        }
    }

    Ok(DiscoverRegistryResult {
        symbols: symbols.into_iter().collect(),
        diagnostics,
    })
}
