//! Shared CLI formatting for language-pack diagnostics.

use wax_contract::{Diagnostic, SourceLocation};

/// Formats one diagnostic for CLI output, including structured location when present.
#[must_use]
pub fn format_diagnostic_line(diagnostic: &Diagnostic) -> String {
    match diagnostic.location.as_ref() {
        Some(location) => format!(
            "{} ({}): {}",
            diagnostic.code,
            format_source_location(location),
            diagnostic.message
        ),
        None => format!("{}: {}", diagnostic.code, diagnostic.message),
    }
}

fn format_source_location(location: &SourceLocation) -> String {
    match location.column {
        Some(column) => format!("{}:{}:{}", location.file, location.line, column),
        None if location.line > 0 => format!("{}:{}", location.file, location.line),
        None => location.file.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_diagnostic_line;
    use wax_contract::{Diagnostic, DiagnosticSeverity, SourceLocation};

    #[test]
    fn formats_diagnostic_with_file_line_and_column() {
        let line = format_diagnostic_line(&Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: "parse_failed".to_owned(),
            message: "failed to parse source file (Expression expected); file skipped".to_owned(),
            location: Some(SourceLocation {
                file: "src/components/Broken.tsx".to_owned(),
                line: 1,
                column: Some(24),
            }),
        });

        assert_eq!(
            line,
            "parse_failed (src/components/Broken.tsx:1:24): failed to parse source file (Expression expected); file skipped"
        );
    }

    #[test]
    fn formats_diagnostic_without_location() {
        let line = format_diagnostic_line(&Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: "parse_failed".to_owned(),
            message: "tree-sitter failed to parse Broken.kt; file skipped".to_owned(),
            location: None,
        });

        assert_eq!(
            line,
            "parse_failed: tree-sitter failed to parse Broken.kt; file skipped"
        );
    }
}
