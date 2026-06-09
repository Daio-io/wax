//! Recoverable scan-gap diagnostic codes shared by emitters and status derivation.

/// SWC parse failed for a single file; scanning continues for other files.
pub const PARSE_FAILED: &str = "parse_failed";
/// Configured literal root path does not exist under the repo root.
pub const ROOT_NOT_FOUND: &str = "root_not_found";
/// Configured wildcard root pattern matched no directories.
pub const ROOT_GLOB_NOT_FOUND: &str = "root_glob_not_found";
/// Design-system default import could not be resolved in the module graph.
pub const DS_IMPORT_UNRESOLVED: &str = "ds_import_unresolved";
/// Design-system named export could not be resolved in the module graph.
pub const DS_EXPORT_UNRESOLVED: &str = "ds_export_unresolved";
/// Configured package entrypoint target file is missing.
pub const PACKAGE_ENTRYPOINT_UNRESOLVED: &str = "package_entrypoint_unresolved";
/// JSX usage could not be resolved to a registry symbol.
pub const DS_USAGE_UNRESOLVED: &str = "ds_usage_unresolved";

const GAP_DIAGNOSTIC_CODES: &[&str] = &[
    PARSE_FAILED,
    ROOT_NOT_FOUND,
    ROOT_GLOB_NOT_FOUND,
    DS_IMPORT_UNRESOLVED,
    DS_EXPORT_UNRESOLVED,
    PACKAGE_ENTRYPOINT_UNRESOLVED,
    DS_USAGE_UNRESOLVED,
];

/// Returns whether a diagnostic code indicates a recoverable scan gap.
#[must_use]
pub fn is_gap_diagnostic(code: &str) -> bool {
    GAP_DIAGNOSTIC_CODES.contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_diagnostic_codes_are_recognized() {
        for code in GAP_DIAGNOSTIC_CODES {
            assert!(is_gap_diagnostic(code), "expected gap code: {code}");
        }
    }

    #[test]
    fn non_gap_diagnostic_codes_are_not_recognized() {
        assert!(!is_gap_diagnostic("react_scaffold"));
        assert!(!is_gap_diagnostic("unknown_code"));
    }
}
