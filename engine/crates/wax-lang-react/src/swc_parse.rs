//! SWC-backed parsing helpers for React source files.

use std::path::{Path, PathBuf};

/// SWC parser crate version from `workspace.dependencies.swc_ecma_parser` in `engine/Cargo.toml`.
pub const SWC_PARSER_VERSION: &str = env!("SWC_PARSER_VERSION");

use swc_common::{FileName, SourceMap, Span, Spanned, sync::Lrc};
use swc_ecma_ast::{EsVersion, Module};
use swc_ecma_parser::{
    EsSyntax, Parser, StringInput, Syntax, TsSyntax, error::SyntaxError, lexer::Lexer,
};
use wax_contract::{Diagnostic, DiagnosticSeverity, SourceLocation};

use crate::diagnostics::PARSE_FAILED;

/// Parse output for one React source file.
#[derive(Debug)]
pub enum ReactParseOutcome {
    /// Parsing succeeded and produced a module AST.
    Parsed(ParsedReactModule),
    /// Parsing failed for this file and produced a recoverable diagnostic.
    Failed(Diagnostic),
}

/// Parsed module and metadata for one source file.
pub struct ParsedReactModule {
    /// Repo-relative file path for the parsed module.
    pub file: PathBuf,
    /// SWC module AST for the source file.
    pub module: Module,
    source_map: Lrc<SourceMap>,
}

impl ParsedReactModule {
    /// Maps an SWC span in this module to a repo-relative [`SourceLocation`].
    #[must_use]
    pub fn source_location_from_span(&self, span: Span) -> Option<SourceLocation> {
        source_location_from_span(&self.source_map, span, &self.file)
    }
}

impl std::fmt::Debug for ParsedReactModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedReactModule")
            .field("file", &self.file)
            .field("module", &self.module)
            .finish_non_exhaustive()
    }
}

/// Fatal errors returned while preparing source input for SWC parsing.
#[derive(Debug)]
pub enum ReactParseError {
    /// Reading a source file from disk failed.
    Io {
        /// Human-readable context for the failed operation.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl ReactParseError {
    /// Stable reason string suitable for wire-error mapping by callers.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::Io { .. } => "io_error",
        }
    }
}

impl std::fmt::Display for ReactParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReactParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Parses one repo-relative React source file with SWC.
///
/// Supported extensions are `.js`, `.jsx`, `.ts`, and `.tsx`.
/// Parse failures are returned as [`ReactParseOutcome::Failed`] diagnostics so
/// callers can continue scanning remaining files.
pub fn parse_react_source_file(
    repo_root: &Path,
    relative_path: &Path,
) -> Result<ReactParseOutcome, ReactParseError> {
    let file_path = repo_root.join(relative_path);
    let source = std::fs::read_to_string(&file_path).map_err(|source| ReactParseError::Io {
        context: format!("read react source file {}", file_path.display()),
        source,
    })?;

    let syntax = syntax_for_path(relative_path);
    let source_map: Lrc<SourceMap> = Default::default();
    let source_file = source_map.new_source_file(
        FileName::Custom(normalize_repo_relative_path(relative_path)).into(),
        source,
    );

    let lexer = Lexer::new(
        syntax,
        EsVersion::Es2022,
        StringInput::from(&*source_file),
        None,
    );
    let mut parser = Parser::new_from(lexer);

    match parser.parse_module() {
        Ok(module) => {
            if let Some(error) = parser.take_errors().into_iter().next() {
                return Ok(ReactParseOutcome::Failed(parse_failed_diagnostic(
                    syntax_error_message(error.kind()),
                    source_location_from_span(&source_map, error.span(), relative_path),
                )));
            }

            Ok(ReactParseOutcome::Parsed(ParsedReactModule {
                file: relative_path.to_path_buf(),
                module,
                source_map,
            }))
        }
        Err(error) => Ok(ReactParseOutcome::Failed(parse_failed_diagnostic(
            syntax_error_message(error.kind()),
            source_location_from_span(&source_map, error.span(), relative_path),
        ))),
    }
}

fn parse_failed_diagnostic(message: String, location: Option<SourceLocation>) -> Diagnostic {
    Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: PARSE_FAILED.to_owned(),
        message,
        location,
    }
}

fn syntax_error_message(kind: &SyntaxError) -> String {
    format!("failed to parse source file ({}); file skipped", kind.msg())
}

fn syntax_for_path(path: &Path) -> Syntax {
    match extension(path) {
        "ts" => Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            ..Default::default()
        }),
        "tsx" => Syntax::Typescript(TsSyntax {
            tsx: true,
            decorators: true,
            ..Default::default()
        }),
        "jsx" | "js" => Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        // Unknown extension: attempt ES+JSX parsing.
        _ => Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
    }
}

fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
}

fn source_location_from_span(
    source_map: &Lrc<SourceMap>,
    span: Span,
    relative_path: &Path,
) -> Option<SourceLocation> {
    if span.is_dummy() {
        return None;
    }

    let lookup = source_map.lookup_char_pos(span.lo());
    let line = u32::try_from(lookup.line).ok()?;
    let column = u32::try_from(lookup.col_display + 1).ok()?;

    Some(SourceLocation {
        file: normalize_repo_relative_path(relative_path),
        line,
        column: Some(column),
    })
}

fn normalize_repo_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{ReactParseOutcome, parse_react_source_file};
    use std::fs;
    use std::path::Path;
    use swc_common::Spanned;
    use wax_contract::DiagnosticSeverity;

    #[test]
    fn swc_parse_parses_supported_extensions() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let cases = [
            ("src/App.js", "export const App = () => <div />;"),
            (
                "src/Card.jsx",
                "export function Card() { return <section />; }",
            ),
            ("src/types.ts", "export const value: number = 1;"),
            (
                "src/Screen.tsx",
                "type Props = { title: string }; export const Screen = (_: Props) => <h1 />;",
            ),
        ];

        for (file, source) in cases {
            let path = tempdir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, source).unwrap();

            let outcome = parse_react_source_file(tempdir.path(), Path::new(file))
                .expect("parse should not return fatal error");
            match outcome {
                ReactParseOutcome::Parsed(parsed) => {
                    assert_eq!(parsed.file, Path::new(file));
                    assert!(!parsed.module.body.is_empty());
                    let first_span = parsed.module.body[0].span();
                    let location = parsed
                        .source_location_from_span(first_span)
                        .expect("span should map to location");
                    assert_eq!(location.file, file);
                    assert!(location.line > 0);
                }
                ReactParseOutcome::Failed(diagnostic) => panic!(
                    "expected parse success for {file}, got {}: {}",
                    diagnostic.code, diagnostic.message
                ),
            }
        }
    }

    #[test]
    fn swc_parse_reports_parse_failed_diagnostic_with_location() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let relative = Path::new("src/Broken.tsx");
        let path = tempdir.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "export function Broken() { return <button><span></button>; }",
        )
        .unwrap();

        let outcome = parse_react_source_file(tempdir.path(), relative).expect("io should succeed");
        match outcome {
            ReactParseOutcome::Failed(diagnostic) => {
                assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
                assert_eq!(diagnostic.code, "parse_failed");
                assert!(
                    diagnostic
                        .message
                        .starts_with("failed to parse source file ("),
                    "unexpected message: {}",
                    diagnostic.message
                );
                assert!(
                    diagnostic.message.ends_with("); file skipped"),
                    "unexpected message: {}",
                    diagnostic.message
                );
                let location = diagnostic.location.expect("location should be present");
                assert_eq!(location.file, "src/Broken.tsx");
                assert_eq!(location.line, 1);
                assert!(location.column.unwrap_or(0) > 0);
            }
            ReactParseOutcome::Parsed(_) => panic!("expected parse failure"),
        }
    }

    #[test]
    fn swc_parse_returns_io_error_for_missing_file() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let err = parse_react_source_file(tempdir.path(), Path::new("src/Missing.tsx"))
            .expect_err("missing file should fail with io error");

        assert_eq!(err.reason(), "io_error");
    }
}
