//! SwiftUI registry symbol discovery.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::component_detect::collect_component_declarations;
use crate::swift_ast::{
    ParseSwiftFileError, collect_swift_files, new_parser, parse_swift_file_strict,
};

/// Errors produced while discovering SwiftUI registry symbols.
#[derive(Debug)]
pub enum SwiftDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// A configured discovery root does not exist.
    MissingRoot(PathBuf),
    /// A Swift file could not be parsed successfully.
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

impl std::fmt::Display for SwiftDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
            Self::MissingRoot(path) => {
                write!(f, "discovery root does not exist: {}", path.display())
            }
            Self::ParseFailed(path) => {
                write!(f, "failed to parse Swift source {}", path.display())
            }
            Self::ParserInitFailed(reason) => write!(f, "parser init failed: {reason}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for SwiftDiscoverError {
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

/// Discovers likely public top-level SwiftUI component symbols from source roots.
pub fn discover_registry_symbols(roots: &[PathBuf]) -> Result<Vec<String>, SwiftDiscoverError> {
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
    for file_path in swift_files {
        let parsed = parse_swift_file_strict(&mut parser, &file_path).map_err(map_parse_error)?;
        for component in
            collect_component_declarations(parsed.tree.root_node(), parsed.source.as_bytes(), true)
        {
            symbols.insert(component.symbol);
        }
    }

    Ok(symbols.into_iter().collect())
}

fn map_parse_error(err: ParseSwiftFileError) -> SwiftDiscoverError {
    match err {
        ParseSwiftFileError::Io { context, source } => SwiftDiscoverError::Io { context, source },
        ParseSwiftFileError::ParseFailed(path) => SwiftDiscoverError::ParseFailed(path),
    }
}
