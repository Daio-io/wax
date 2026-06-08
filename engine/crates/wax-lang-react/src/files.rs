//! React source file collection from configured roots.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use wax_contract::{Diagnostic, DiagnosticSeverity};
use wax_lang_api::{RootPatternKind, RootResolutionError, resolve_source_roots};

/// Default ignore glob patterns applied before configured `ignore` entries.
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    "**/node_modules/**",
    "**/generated/**",
    "**/__generated__/**",
    "**/*.d.ts",
    "**/*.stories.{js,jsx,ts,tsx}",
    "**/*.{spec,test}.{js,jsx,ts,tsx}",
];

/// Collected React source files and root-resolution diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactSourceFileCollection {
    /// Repo-relative paths to supported source files, sorted and deduplicated.
    pub files: Vec<PathBuf>,
    /// Diagnostics emitted while resolving configured roots.
    pub root_diagnostics: Vec<Diagnostic>,
}

/// Errors produced while collecting React source files.
#[derive(Debug)]
pub enum ReactFileCollectionError {
    /// A filesystem operation failed.
    Io {
        /// Human-readable context for the failed operation.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for ReactFileCollectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ReactFileCollectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Collects supported React source files from configured repo-relative roots.
///
/// Supported extensions are `.js`, `.jsx`, `.ts`, and `.tsx`. Declaration files
/// ending in `.d.ts` are always excluded. Documented default ignore patterns are
/// applied first, then any configured `ignore` patterns.
pub fn collect_react_source_files(
    repo_root: &Path,
    roots: &[PathBuf],
    ignore: &[String],
) -> Result<ReactSourceFileCollection, ReactFileCollectionError> {
    let ignore_patterns = combined_ignore_patterns(ignore);
    let mut collected = BTreeSet::new();
    let mut root_diagnostics = Vec::new();

    for root in roots {
        let resolved = resolve_source_roots(repo_root, root).map_err(map_root_resolution_error)?;
        if resolved.roots.is_empty() {
            root_diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: root_not_found_code(resolved.kind),
                message: root_not_found_message(root, resolved.kind),
                location: None,
            });
            continue;
        }

        for source_root in resolved.roots {
            walk_source_root(&source_root, repo_root, &ignore_patterns, &mut collected)?;
        }
    }

    Ok(ReactSourceFileCollection {
        files: collected.into_iter().collect(),
        root_diagnostics,
    })
}

fn combined_ignore_patterns(configured: &[String]) -> Vec<String> {
    let mut patterns = DEFAULT_IGNORE_PATTERNS
        .iter()
        .map(|pattern| (*pattern).to_owned())
        .collect::<Vec<_>>();
    patterns.extend(configured.iter().cloned());
    patterns
}

fn walk_source_root(
    dir: &Path,
    repo_root: &Path,
    ignore_patterns: &[String],
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), ReactFileCollectionError> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|source| ReactFileCollectionError::Io {
        context: format!("read source root {}", dir.display()),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ReactFileCollectionError::Io {
            context: format!("read source root entry {}", dir.display()),
            source,
        })?;
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)
            .map_err(|source| ReactFileCollectionError::Io {
                context: format!("read metadata for {}", path.display()),
                source,
            })?
            .file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let relative = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
            let relative_text = normalize_repo_relative_path(&relative);
            if path_matches_any(&relative_text, ignore_patterns) {
                continue;
            }
            walk_source_root(&path, repo_root, ignore_patterns, files)?;
        } else if is_supported_react_source(&path) {
            let relative = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
            let relative_text = normalize_repo_relative_path(&relative);
            if !path_matches_any(&relative_text, ignore_patterns) {
                files.insert(relative);
            }
        }
    }
    Ok(())
}

fn is_supported_react_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.ends_with(".d.ts") {
        return false;
    }
    name.ends_with(".jsx")
        || name.ends_with(".tsx")
        || name.ends_with(".js")
        || name.ends_with(".ts")
}

fn normalize_repo_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_matches_any(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| path_matches_glob(path, pattern))
}

fn path_matches_glob(path: &str, pattern: &str) -> bool {
    expand_brace_groups(pattern)
        .iter()
        .any(|expanded| path_matches_glob_no_brace(path, expanded))
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

// Root diagnostic helpers mirror wax-lang-compose and wax-lang-basic until shared in wax-lang-api.
fn map_root_resolution_error(err: RootResolutionError) -> ReactFileCollectionError {
    match err {
        RootResolutionError::Io { context, source } => {
            ReactFileCollectionError::Io { context, source }
        }
    }
}

fn root_not_found_code(kind: RootPatternKind) -> String {
    match kind {
        RootPatternKind::Literal => "root_not_found".to_owned(),
        RootPatternKind::Wildcard => "root_glob_not_found".to_owned(),
    }
}

fn root_not_found_message(root: &Path, kind: RootPatternKind) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn files_collects_supported_extensions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("src");
        fs::create_dir_all(&root).unwrap();
        for name in ["App.js", "Card.jsx", "types.ts", "Screen.tsx"] {
            fs::write(root.join(name), "// source").unwrap();
        }

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("src")], &[])
            .expect("collection should succeed");

        assert_eq!(
            collection.files,
            vec![
                PathBuf::from("src/App.js"),
                PathBuf::from("src/Card.jsx"),
                PathBuf::from("src/Screen.tsx"),
                PathBuf::from("src/types.ts"),
            ]
        );
        assert!(collection.root_diagnostics.is_empty());
    }

    #[test]
    fn files_excludes_declaration_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("src");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Button.tsx"), "export {}").unwrap();
        fs::write(root.join("Button.d.ts"), "declare const Button").unwrap();

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("src")], &[])
            .expect("collection should succeed");

        assert_eq!(collection.files, vec![PathBuf::from("src/Button.tsx")]);
    }

    #[test]
    fn files_applies_default_ignore_patterns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/App.tsx"), "export {}").unwrap();
        fs::write(tmp.path().join("src/App.stories.tsx"), "export {}").unwrap();
        fs::write(tmp.path().join("src/App.test.tsx"), "export {}").unwrap();
        fs::write(tmp.path().join("src/App.spec.ts"), "export {}").unwrap();
        fs::create_dir_all(tmp.path().join("src/node_modules/pkg")).unwrap();
        fs::write(
            tmp.path().join("src/node_modules/pkg/index.js"),
            "export {}",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("src/generated/deep")).unwrap();
        fs::write(tmp.path().join("src/generated/deep/App.tsx"), "export {}").unwrap();
        fs::create_dir_all(tmp.path().join("src/__generated__")).unwrap();
        fs::write(tmp.path().join("src/__generated__/Card.tsx"), "export {}").unwrap();

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("src")], &[])
            .expect("collection should succeed");

        assert_eq!(collection.files, vec![PathBuf::from("src/App.tsx")]);
    }

    #[test]
    fn files_applies_configured_ignore_patterns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::create_dir_all(tmp.path().join("src/generated")).unwrap();
        fs::write(tmp.path().join("src/App.tsx"), "export {}").unwrap();
        fs::write(tmp.path().join("src/generated/App.tsx"), "export {}").unwrap();

        let collection = collect_react_source_files(
            tmp.path(),
            &[PathBuf::from("src")],
            &["src/generated/**".to_owned()],
        )
        .expect("collection should succeed");

        assert_eq!(collection.files, vec![PathBuf::from("src/App.tsx")]);
    }

    #[test]
    fn files_deduplicates_and_sorts_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("apps/web/src")).unwrap();
        fs::create_dir_all(tmp.path().join("apps/shared/src")).unwrap();
        fs::write(tmp.path().join("apps/web/src/App.tsx"), "export {}").unwrap();
        fs::write(tmp.path().join("apps/shared/src/Card.tsx"), "export {}").unwrap();

        let collection = collect_react_source_files(
            tmp.path(),
            &[
                PathBuf::from("apps/web/src"),
                PathBuf::from("apps/shared/src"),
                PathBuf::from("apps/web/src"),
            ],
            &[],
        )
        .expect("collection should succeed");

        assert_eq!(
            collection.files,
            vec![
                PathBuf::from("apps/shared/src/Card.tsx"),
                PathBuf::from("apps/web/src/App.tsx"),
            ]
        );
    }

    #[test]
    fn files_emits_root_not_found_for_missing_literal_root() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let collection =
            collect_react_source_files(tmp.path(), &[PathBuf::from("missing-root")], &[])
                .expect("collection should succeed");

        assert!(collection.files.is_empty());
        assert_eq!(collection.root_diagnostics.len(), 1);
        assert_eq!(collection.root_diagnostics[0].code, "root_not_found");
    }

    #[test]
    fn files_emits_root_glob_not_found_for_unmatched_wildcard_root() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("*/src")], &[])
            .expect("collection should succeed");

        assert!(collection.files.is_empty());
        assert_eq!(collection.root_diagnostics.len(), 1);
        assert_eq!(collection.root_diagnostics[0].code, "root_glob_not_found");
    }

    #[test]
    fn files_resolves_wildcard_roots_like_compose() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for module in ["app", "feature"] {
            let source_dir = tmp.path().join(module).join("src");
            fs::create_dir_all(&source_dir).unwrap();
            fs::write(source_dir.join("Screen.tsx"), "export {}").unwrap();
        }

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("*/src")], &[])
            .expect("collection should succeed");

        assert_eq!(collection.files.len(), 2);
        assert!(collection.root_diagnostics.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn files_skips_symlinked_directories() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real_dir = tmp.path().join("real-src");
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(real_dir.join("App.tsx"), "export {}").unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::os::unix::fs::symlink(&real_dir, tmp.path().join("src/link")).unwrap();

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("src")], &[])
            .expect("collection should succeed");

        assert!(collection.files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn files_skips_ignored_directories_before_recursion() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/App.tsx"), "export {}").unwrap();
        let node_modules = tmp.path().join("src/node_modules");
        fs::create_dir_all(node_modules.join("pkg")).unwrap();
        fs::write(node_modules.join("pkg/Ignored.tsx"), "export {}").unwrap();
        let mut permissions = fs::metadata(&node_modules).unwrap().permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&node_modules, permissions).unwrap();

        let collection = collect_react_source_files(tmp.path(), &[PathBuf::from("src")], &[])
            .expect("collection should succeed without descending into node_modules");

        assert_eq!(collection.files, vec![PathBuf::from("src/App.tsx")]);
    }

    #[test]
    fn files_glob_matcher_supports_default_ignore_patterns() {
        assert!(path_matches_glob(
            "apps/web/node_modules/pkg/index.js",
            "**/node_modules/**"
        ));
        assert!(path_matches_glob("src/App.d.ts", "**/*.d.ts"));
        assert!(path_matches_glob(
            "apps/web/src/App.stories.tsx",
            "**/*.stories.{js,jsx,ts,tsx}"
        ));
        assert!(path_matches_glob(
            "apps/web/src/App.stories.js",
            "**/*.stories.{js,jsx,ts,tsx}"
        ));
        assert!(path_matches_glob(
            "apps/web/src/App.test.tsx",
            "**/*.{spec,test}.{js,jsx,ts,tsx}"
        ));
        assert!(path_matches_glob(
            "apps/web/src/App.spec.ts",
            "**/*.{spec,test}.{js,jsx,ts,tsx}"
        ));
        assert!(path_matches_glob(
            "src/generated/deep/App.tsx",
            "**/generated/**"
        ));
        assert!(path_matches_glob(
            "src/__generated__/Card.tsx",
            "**/__generated__/**"
        ));
        assert!(path_matches_glob(
            "src/generated/deep/App.tsx",
            "src/generated/**"
        ));
        assert!(!path_matches_glob(
            "apps/web/src/App.tsx",
            "**/*.stories.tsx"
        ));
        assert!(!path_matches_glob("src/App.tsx", "**/*.d.ts"));
    }
}
