//! Shared repo-relative path normalization and exclude/include glob matching.

use std::path::Path;

use crate::root_patterns::RootPatternKind;

/// Diagnostic code when a literal configured root is missing.
pub const ROOT_NOT_FOUND: &str = "root_not_found";

/// Diagnostic code when a wildcard configured root matches no directories.
pub const ROOT_GLOB_NOT_FOUND: &str = "root_glob_not_found";

/// Normalizes a repo-relative path to forward-slash form for glob matching.
#[must_use]
pub fn normalize_repo_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Returns true when `path` matches any of the glob `patterns`.
#[must_use]
pub fn path_matches_any(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| path_matches_glob(path, pattern))
}

/// Returns true when `path` matches a single brace-expanded glob `pattern`.
#[must_use]
pub fn path_matches_glob(path: &str, pattern: &str) -> bool {
    expand_brace_groups(pattern)
        .iter()
        .any(|expanded| path_matches_glob_no_brace(path, expanded))
}

/// Diagnostic code for a missing or unmatched configured source root.
#[must_use]
pub fn root_not_found_code(kind: RootPatternKind) -> &'static str {
    match kind {
        RootPatternKind::Literal => ROOT_NOT_FOUND,
        RootPatternKind::Wildcard => ROOT_GLOB_NOT_FOUND,
    }
}

/// Human-readable diagnostic for a missing or unmatched configured source root.
#[must_use]
pub fn root_not_found_message(root: &Path, kind: RootPatternKind) -> String {
    match kind {
        RootPatternKind::Literal => format!(
            "configured root '{}' does not exist under repo root; no files scanned from it",
            root.display()
        ),
        RootPatternKind::Wildcard => format!(
            "configured root pattern '{}' matched no directories under repo root; no files scanned from it",
            root.display()
        ),
    }
}

fn expand_brace_groups(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(end_offset) = pattern[start..].find('}') else {
        return vec![pattern.to_owned()];
    };
    let end = start + end_offset;
    let prefix = &pattern[..start];
    let suffix = &pattern[end + 1..];
    let alternatives = pattern[start + 1..end].split(',');
    let mut expanded = Vec::new();
    for alternative in alternatives {
        expanded.extend(expand_brace_groups(&format!(
            "{prefix}{alternative}{suffix}"
        )));
    }
    expanded
}

fn path_matches_glob_no_brace(path: &str, pattern: &str) -> bool {
    let path_segments = split_path_segments(path);
    let pattern_segments = split_path_segments(pattern);
    segments_match(&path_segments, &pattern_segments)
}

fn split_path_segments(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn segments_match(path_segments: &[&str], pattern_segments: &[&str]) -> bool {
    let mut path_idx = 0;
    let mut pattern_idx = 0;

    while pattern_idx < pattern_segments.len() {
        if pattern_segments[pattern_idx] == "**" {
            if pattern_idx == pattern_segments.len() - 1 {
                return true;
            }
            for skip in 0..=(path_segments.len().saturating_sub(path_idx)) {
                if segments_match(
                    &path_segments[path_idx + skip..],
                    &pattern_segments[pattern_idx + 1..],
                ) {
                    return true;
                }
            }
            return false;
        }

        if path_idx >= path_segments.len()
            || !segment_matches(path_segments[path_idx], pattern_segments[pattern_idx])
        {
            return false;
        }

        path_idx += 1;
        pattern_idx += 1;
    }

    path_idx == path_segments.len()
}

fn segment_matches(segment: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    glob_segment_match(segment.as_bytes(), pattern.as_bytes())
}

fn glob_segment_match(segment: &[u8], pattern: &[u8]) -> bool {
    match (segment.first(), pattern.first()) {
        (None, None) => true,
        (Some(_), None) => false,
        (None, Some(b'*')) => glob_segment_match(segment, &pattern[1..]),
        (None, Some(_)) => false,
        (Some(_), Some(b'*')) => {
            glob_segment_match(&segment[1..], pattern) || glob_segment_match(segment, &pattern[1..])
        }
        (Some(segment_byte), Some(pattern_byte)) if segment_byte == pattern_byte => {
            glob_segment_match(&segment[1..], &pattern[1..])
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn normalize_repo_relative_path_uses_forward_slashes() {
        assert_eq!(
            normalize_repo_relative_path(Path::new("src\\main\\App.kt")),
            "src/main/App.kt"
        );
    }

    #[test]
    fn path_matches_any_honors_brace_globs() {
        assert!(path_matches_any(
            "app/Button.test.tsx",
            &["**/*.{spec,test}.{js,jsx,ts,tsx}".to_owned()]
        ));
        assert!(!path_matches_any(
            "app/Button.tsx",
            &["**/*.{spec,test}.{js,jsx,ts,tsx}".to_owned()]
        ));
    }

    #[test]
    fn root_not_found_codes_match_contract_strings() {
        assert_eq!(
            root_not_found_code(RootPatternKind::Literal),
            ROOT_NOT_FOUND
        );
        assert_eq!(
            root_not_found_code(RootPatternKind::Wildcard),
            ROOT_GLOB_NOT_FOUND
        );
    }
}
