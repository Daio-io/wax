//! Source root pattern resolution for language packs.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Whether a configured source root was literal or used wildcard path segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootPatternKind {
    /// A literal repo-relative source root with no wildcard path segments.
    Literal,
    /// A source root containing one or more `*` or `**` path segments.
    Wildcard,
}

/// Concrete source roots resolved from one configured root entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootResolution {
    /// The kind of configured root that was resolved.
    pub kind: RootPatternKind,
    /// Existing absolute directories matched by the configured root.
    pub roots: Vec<PathBuf>,
}

/// Errors produced while resolving source root patterns.
#[derive(Debug, Error)]
pub enum RootResolutionError {
    /// A filesystem operation failed while expanding wildcard path segments.
    #[error("{context}: {source}")]
    Io {
        /// Human-readable context for the failed filesystem operation.
        context: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// Resolves a repo-relative source root, expanding wildcard path components.
///
/// This is intentionally smaller than full glob syntax: `*` expands one directory
/// level and `**` expands zero or more directory levels. `?` and mixed wildcard
/// segments such as `app-*` are treated literally. Prefer anchoring recursive
/// roots behind a literal prefix to avoid walking the whole repository.
pub fn resolve_source_roots(
    repo_root: &Path,
    root: &Path,
) -> Result<RootResolution, RootResolutionError> {
    if !has_wildcard_segment(root) {
        let abs_root = repo_root.join(root);
        return Ok(RootResolution {
            kind: RootPatternKind::Literal,
            roots: abs_root.exists().then_some(abs_root).into_iter().collect(),
        });
    }

    let mut candidates = vec![repo_root.to_path_buf()];
    for component in root.components() {
        let text = component.as_os_str();
        if text == "*" {
            let mut expanded = expand_one_directory_level(&candidates)?;
            expanded.sort();
            candidates = expanded;
        } else if text == "**" {
            let mut expanded = expand_recursive_directory_levels(&candidates)?;
            expanded.sort();
            candidates = expanded;
        } else {
            candidates = candidates
                .into_iter()
                .map(|candidate| candidate.join(text))
                .collect();
        }
    }

    let mut roots = candidates
        .into_iter()
        .filter(|candidate| candidate.exists())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();

    Ok(RootResolution {
        kind: RootPatternKind::Wildcard,
        roots,
    })
}

fn expand_one_directory_level(candidates: &[PathBuf]) -> Result<Vec<PathBuf>, RootResolutionError> {
    let mut expanded = Vec::new();
    for candidate in candidates {
        collect_child_directories(candidate, &mut expanded)?;
    }
    Ok(expanded)
}

fn expand_recursive_directory_levels(
    candidates: &[PathBuf],
) -> Result<Vec<PathBuf>, RootResolutionError> {
    let mut expanded = Vec::new();
    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        expanded.push(candidate.clone());
        collect_descendant_directories(candidate, &mut expanded)?;
    }
    Ok(expanded)
}

fn collect_descendant_directories(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), RootResolutionError> {
    let children = child_directories(dir)?;
    for child in children {
        out.push(child.clone());
        collect_descendant_directories(&child, out)?;
    }
    Ok(())
}

fn collect_child_directories(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), RootResolutionError> {
    out.extend(child_directories(dir)?);
    Ok(())
}

fn child_directories(dir: &Path) -> Result<Vec<PathBuf>, RootResolutionError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(dir).map_err(|source| RootResolutionError::Io {
        context: format!("read wildcard root segment {}", dir.display()),
        source,
    })?;
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| RootResolutionError::Io {
            context: format!("read wildcard root entry {}", dir.display()),
            source,
        })?;
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)
            .map_err(|source| RootResolutionError::Io {
                context: format!("read metadata for {}", path.display()),
                source,
            })?
            .file_type();
        if file_type.is_dir() && !file_type.is_symlink() {
            directories.push(path);
        }
    }
    directories.sort();
    Ok(directories)
}

/// Returns true when a path contains a component that is exactly `*` or `**`.
pub fn has_wildcard_segment(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "*" || component.as_os_str() == "**")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn literal_root_resolves_when_directory_exists() {
        let repo_root = temp_repo("literal-root");
        fs::create_dir_all(repo_root.join("app/src/main/kotlin")).unwrap();

        let resolved =
            resolve_source_roots(&repo_root, PathBuf::from("app/src/main/kotlin").as_path())
                .expect("literal root should resolve");

        assert_eq!(resolved.kind, RootPatternKind::Literal);
        assert_eq!(resolved.roots, vec![repo_root.join("app/src/main/kotlin")]);

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn wildcard_root_resolves_matching_modules_in_sorted_order() {
        let repo_root = temp_repo("wildcard-root");
        fs::create_dir_all(repo_root.join("feature/src/main/kotlin")).unwrap();
        fs::create_dir_all(repo_root.join("app/src/main/kotlin")).unwrap();
        fs::create_dir_all(repo_root.join("core/src/main")).unwrap();
        fs::create_dir_all(repo_root.join("docs")).unwrap();

        let resolved =
            resolve_source_roots(&repo_root, PathBuf::from("*/src/main/kotlin").as_path())
                .expect("wildcard root should resolve");

        assert_eq!(resolved.kind, RootPatternKind::Wildcard);
        assert_eq!(
            resolved.roots,
            vec![
                repo_root.join("app/src/main/kotlin"),
                repo_root.join("feature/src/main/kotlin")
            ]
        );

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn recursive_wildcard_root_resolves_nested_modules() {
        let repo_root = temp_repo("recursive-wildcard-root");
        fs::create_dir_all(repo_root.join("capsule/shared/feature/src/main/kotlin")).unwrap();
        fs::create_dir_all(repo_root.join("capsule/design-system/src/main/kotlin")).unwrap();
        fs::create_dir_all(repo_root.join("capsule/shared/feature/src/test/kotlin")).unwrap();
        fs::create_dir_all(repo_root.join("other/shared/feature/src/main/kotlin")).unwrap();

        let resolved = resolve_source_roots(
            &repo_root,
            PathBuf::from("capsule/**/src/main/kotlin").as_path(),
        )
        .expect("recursive wildcard root should resolve");

        assert_eq!(resolved.kind, RootPatternKind::Wildcard);
        assert_eq!(
            resolved.roots,
            vec![
                repo_root.join("capsule/design-system/src/main/kotlin"),
                repo_root.join("capsule/shared/feature/src/main/kotlin")
            ]
        );

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn recursive_wildcard_root_deduplicates_converging_matches() {
        let repo_root = temp_repo("recursive-wildcard-root-dedup");
        fs::create_dir_all(repo_root.join("capsule/shared/feature/src/main/kotlin")).unwrap();

        let resolved = resolve_source_roots(
            &repo_root,
            PathBuf::from("capsule/**/**/src/main/kotlin").as_path(),
        )
        .expect("recursive wildcard root should resolve");

        assert_eq!(
            resolved.roots,
            vec![repo_root.join("capsule/shared/feature/src/main/kotlin")]
        );

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn unmatched_recursive_wildcard_root_resolves_empty_wildcard_result() {
        let repo_root = temp_repo("recursive-wildcard-root-empty");
        fs::create_dir_all(repo_root.join("capsule/shared/feature/src/test/kotlin")).unwrap();

        let resolved = resolve_source_roots(
            &repo_root,
            PathBuf::from("capsule/**/src/main/kotlin").as_path(),
        )
        .expect("recursive wildcard root should resolve");

        assert_eq!(resolved.kind, RootPatternKind::Wildcard);
        assert!(resolved.roots.is_empty());

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn recursive_wildcard_root_matches_zero_directory_levels() {
        let repo_root = temp_repo("recursive-wildcard-root-zero-level");
        fs::create_dir_all(repo_root.join("capsule/src/main/kotlin")).unwrap();

        let resolved = resolve_source_roots(
            &repo_root,
            PathBuf::from("capsule/**/src/main/kotlin").as_path(),
        )
        .expect("recursive wildcard root should resolve");

        assert_eq!(
            resolved.roots,
            vec![repo_root.join("capsule/src/main/kotlin")]
        );

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn recursive_wildcard_root_skips_symlinked_directories() {
        let repo_root = temp_repo("recursive-wildcard-root-symlink");
        fs::create_dir_all(repo_root.join("linked-target/src/main/kotlin")).unwrap();
        fs::create_dir_all(repo_root.join("capsule")).unwrap();
        std::os::unix::fs::symlink(
            repo_root.join("linked-target"),
            repo_root.join("capsule/linked"),
        )
        .unwrap();

        let resolved = resolve_source_roots(
            &repo_root,
            PathBuf::from("capsule/**/src/main/kotlin").as_path(),
        )
        .expect("recursive wildcard root should resolve");

        assert!(resolved.roots.is_empty());

        fs::remove_dir_all(repo_root).unwrap();
    }

    #[test]
    fn literal_missing_root_resolves_empty_literal_result() {
        let repo_root = temp_repo("literal-missing-root");

        let resolved =
            resolve_source_roots(&repo_root, PathBuf::from("app/src/main/kotlin").as_path())
                .expect("literal missing root should not fail");

        assert_eq!(resolved.kind, RootPatternKind::Literal);
        assert!(resolved.roots.is_empty());

        fs::remove_dir_all(repo_root).unwrap();
    }

    fn temp_repo(name: &str) -> PathBuf {
        let unique = format!(
            "wax-api-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
