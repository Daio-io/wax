//! Fetches conventional registry files from Git repositories.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use thiserror::Error;
use wax_contract::LanguageId;

const MAX_STDERR_SUMMARY: usize = 512;

/// The registry bytes and the full commit object that produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRegistryFetch {
    /// Full commit object id resolved by Git.
    pub commit: String,
    /// Exact bytes of the conventional registry file.
    pub bytes: Vec<u8>,
}

/// Errors returned while resolving and reading a Git-backed registry.
#[derive(Debug, Error)]
pub enum RegistryGitError {
    /// The Git URL or ref was empty or contained invalid control characters.
    #[error("invalid Git {kind}: {value:?}")]
    InvalidInput {
        /// Input category.
        kind: &'static str,
        /// Rejected value.
        value: String,
    },
    /// The system Git executable was not found.
    #[error("system git executable was not found")]
    GitNotFound,
    /// The repository cache directory could not be created.
    #[error("failed to create Git registry cache directory {path}: {source}")]
    CacheDirectory {
        /// Cache directory.
        path: PathBuf,
        /// Filesystem error.
        source: io::Error,
    },
    /// The bare repository could not be initialized.
    #[error("failed to initialize Git registry cache {path}: {stderr}")]
    CacheInit {
        /// Bare repository path.
        path: PathBuf,
        /// Trimmed diagnostic summary.
        stderr: String,
    },
    /// Git fetch returned a non-zero exit status.
    #[error("Git fetch failed for {url:?} and ref {reference:?} (status {status}): {stderr}")]
    FetchFailed {
        /// Requested URL.
        url: String,
        /// Requested tag or object id.
        reference: String,
        /// Process exit status, if available.
        status: String,
        /// Trimmed diagnostic summary.
        stderr: String,
    },
    /// Git could not peel a fetched reference to a commit.
    #[error("failed to resolve fetched Git object: {stderr}")]
    CommitResolution {
        /// Trimmed diagnostic summary.
        stderr: String,
    },
    /// The fetched object did not match a locked commit.
    #[error("locked commit mismatch: requested {requested}, resolved {resolved}")]
    LockedCommitMismatch {
        /// Requested locked commit.
        requested: String,
        /// Commit returned by Git.
        resolved: String,
    },
    /// The conventional registry file is absent at the resolved commit.
    #[error("registry file {path} is absent at commit {commit}")]
    RegistryFileAbsent {
        /// Expected repo-internal path.
        path: String,
        /// Resolved commit.
        commit: String,
    },
    /// Git could not read the conventional registry file.
    #[error("failed to read registry file {path} at commit {commit}: {stderr}")]
    RegistryRead {
        /// Repo-internal registry path.
        path: String,
        /// Resolved commit.
        commit: String,
        /// Trimmed diagnostic summary.
        stderr: String,
    },
}

/// Returns the fixed repo-internal path for a language registry.
pub fn conventional_git_registry_path(language_id: &LanguageId) -> String {
    format!(".wax/registries/{language_id}.json")
}

/// Fetches a tag or commit ref and reads its conventional language registry.
///
/// # Errors
///
/// Returns [`RegistryGitError`] when Git cannot fetch or resolve the ref, the
/// cache cannot be initialized, or the conventional file cannot be read.
pub fn fetch_git_registry(
    git_url: &str,
    tag_or_sha: &str,
    language_id: &LanguageId,
    cache_dir: &Path,
) -> Result<GitRegistryFetch, RegistryGitError> {
    validate_input("URL", git_url)?;
    validate_input("ref", tag_or_sha)?;
    let cache_repo = prepare_cache(git_url, cache_dir)?;
    let output = run_git(&[
        "--git-dir",
        path_arg(&cache_repo),
        "fetch",
        "--force",
        "--depth=1",
        "--no-tags",
        git_url,
        tag_or_sha,
    ])?;
    if !output.status.success() {
        return Err(RegistryGitError::FetchFailed {
            url: git_url.to_owned(),
            reference: tag_or_sha.to_owned(),
            status: status_string(&output),
            stderr: stderr_summary(&output),
        });
    }
    let commit = resolve_fetch_head(&cache_repo)?;
    read_registry(&cache_repo, &commit, language_id)
}

/// Reads a registry at an exact full commit, fetching that object if needed.
///
/// # Errors
///
/// Returns [`RegistryGitError`] when the locked object is invalid, unavailable,
/// does not match the requested commit, or its conventional file cannot be read.
pub fn fetch_git_registry_at_commit(
    git_url: &str,
    commit: &str,
    language_id: &LanguageId,
    cache_dir: &Path,
) -> Result<GitRegistryFetch, RegistryGitError> {
    validate_input("URL", git_url)?;
    validate_commit(commit)?;
    let cache_repo = prepare_cache(git_url, cache_dir)?;

    let cached = run_git(&[
        "--git-dir",
        path_arg(&cache_repo),
        "rev-parse",
        "--verify",
        &format!("{commit}^{{commit}}"),
    ])?;
    let resolved = if cached.status.success() {
        parse_commit(&cached)?
    } else {
        let fetched = run_git(&[
            "--git-dir",
            path_arg(&cache_repo),
            "fetch",
            "--force",
            "--depth=1",
            "--no-tags",
            git_url,
            commit,
        ])?;
        if !fetched.status.success() {
            return Err(RegistryGitError::FetchFailed {
                url: git_url.to_owned(),
                reference: commit.to_owned(),
                status: status_string(&fetched),
                stderr: stderr_summary(&fetched),
            });
        }
        let fetched_commit = resolve_fetch_head(&cache_repo)?;
        if fetched_commit != commit {
            return Err(RegistryGitError::LockedCommitMismatch {
                requested: commit.to_owned(),
                resolved: fetched_commit,
            });
        }
        fetched_commit
    };
    if resolved != commit {
        return Err(RegistryGitError::LockedCommitMismatch {
            requested: commit.to_owned(),
            resolved,
        });
    }
    read_registry(&cache_repo, commit, language_id)
}

fn validate_input(kind: &'static str, value: &str) -> Result<(), RegistryGitError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(RegistryGitError::InvalidInput {
            kind,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_commit(commit: &str) -> Result<(), RegistryGitError> {
    validate_input("commit", commit)?;
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RegistryGitError::InvalidInput {
            kind: "commit",
            value: commit.to_owned(),
        });
    }
    Ok(())
}

fn prepare_cache(url: &str, cache_dir: &Path) -> Result<PathBuf, RegistryGitError> {
    let mut digest = Sha256::new();
    digest.update(url.as_bytes());
    let key: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let repo = cache_dir.join(key);
    std::fs::create_dir_all(cache_dir).map_err(|source| RegistryGitError::CacheDirectory {
        path: cache_dir.to_owned(),
        source,
    })?;
    if !repo.exists() {
        let output = run_git(&["init", "--bare", path_arg(&repo)])?;
        if !output.status.success() {
            return Err(RegistryGitError::CacheInit {
                path: repo,
                stderr: stderr_summary(&output),
            });
        }
    }
    Ok(repo)
}

fn resolve_fetch_head(repo: &Path) -> Result<String, RegistryGitError> {
    let output = run_git(&[
        "--git-dir",
        path_arg(repo),
        "rev-parse",
        "--verify",
        "FETCH_HEAD^{commit}",
    ])?;
    if !output.status.success() {
        return Err(RegistryGitError::CommitResolution {
            stderr: stderr_summary(&output),
        });
    }
    parse_commit(&output)
}

fn parse_commit(output: &Output) -> Result<String, RegistryGitError> {
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if commit.is_empty() {
        return Err(RegistryGitError::CommitResolution {
            stderr: stderr_summary(output),
        });
    }
    Ok(commit)
}

fn read_registry(
    repo: &Path,
    commit: &str,
    language_id: &LanguageId,
) -> Result<GitRegistryFetch, RegistryGitError> {
    let path = conventional_git_registry_path(language_id);
    let object = format!("{commit}:{path}");
    let exists = run_git(&["--git-dir", path_arg(repo), "cat-file", "-e", &object])?;
    if !exists.status.success() {
        return Err(RegistryGitError::RegistryFileAbsent {
            path,
            commit: commit.to_owned(),
        });
    }
    let output = run_git(&["--git-dir", path_arg(repo), "show", &object])?;
    if !output.status.success() {
        return Err(RegistryGitError::RegistryRead {
            path,
            commit: commit.to_owned(),
            stderr: stderr_summary(&output),
        });
    }
    Ok(GitRegistryFetch {
        commit: commit.to_owned(),
        bytes: output.stdout,
    })
}

fn run_git(args: &[&str]) -> Result<Output, RegistryGitError> {
    Command::new("git").args(args).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            RegistryGitError::GitNotFound
        } else {
            RegistryGitError::CommitResolution {
                stderr: error.to_string(),
            }
        }
    })
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("cache paths must be valid UTF-8")
}
fn stderr_summary(output: &Output) -> String {
    summarize(&String::from_utf8_lossy(&output.stderr))
}
fn summarize(value: &str) -> String {
    value.trim().chars().take(MAX_STDERR_SUMMARY).collect()
}
fn status_string(output: &Output) -> String {
    output
        .status
        .code()
        .map_or_else(|| "signal".to_owned(), |code| code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        remote: PathBuf,
        first: String,
        second: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git_available() -> bool {
        match Command::new("git").arg("--version").output() {
            Ok(output) if output.status.success() => true,
            Ok(output) => panic!("git --version failed: {}", stderr_summary(&output)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                eprintln!("skipping git-dependent registry_git test: system git was not found");
                false
            }
            Err(error) => panic!("could not spawn git --version: {error}"),
        }
    }

    fn command(args: &[&str], cwd: &Path) -> Output {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Wax Test",
                "-c",
                "user.email=wax-test@example.com",
            ])
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            stderr_summary(&output)
        );
        output
    }

    fn fixture() -> Fixture {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("wax-registry-git-{}-{id}", std::process::id()));
        let work = root.join("work");
        let remote = root.join("remote.git");
        fs::create_dir_all(&work).expect("fixture worktree");
        command(&["init", "-q"], &work);
        command(&["init", "--bare", "-q", path_arg(&remote)], &root);
        fs::create_dir_all(work.join(".wax/registries")).expect("registry directory");
        fs::write(work.join(".wax/registries/compose.json"), b"first\n").expect("first registry");
        command(&["add", "."], &work);
        command(&["commit", "-m", "first", "-q"], &work);
        let first = git_output(&["rev-parse", "HEAD"], &work);
        command(&["tag", "v1"], &work);
        command(&["tag", "-a", "v1-annotated", "-m", "annotated"], &work);
        command(&["remote", "add", "origin", path_arg(&remote)], &work);
        command(&["push", "-q", "origin", "--tags", "HEAD"], &work);

        fs::write(work.join(".wax/registries/compose.json"), b"second\n").expect("second registry");
        command(&["add", "."], &work);
        command(&["commit", "-m", "second", "-q"], &work);
        let second = git_output(&["rev-parse", "HEAD"], &work);
        command(&["tag", "-f", "v1"], &work);
        command(&["push", "-q", "--force", "origin", "v1", "HEAD"], &work);

        Fixture {
            root,
            remote,
            first,
            second,
        }
    }

    fn git_output(args: &[&str], cwd: &Path) -> String {
        String::from_utf8(command(args, cwd).stdout)
            .expect("git output is UTF-8")
            .trim()
            .to_owned()
    }

    fn language() -> LanguageId {
        LanguageId::try_from("compose").expect("valid language id")
    }

    #[test]
    fn resolves_refs_and_returns_exact_bytes() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let cache = fixture.root.join("cache");
        let result =
            fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &language(), &cache)
                .unwrap();
        assert_eq!(result.commit, fixture.second);
        assert_eq!(result.bytes, b"second\n");
        let annotated = fetch_git_registry(
            fixture.remote.to_str().unwrap(),
            "v1-annotated",
            &language(),
            &cache,
        )
        .unwrap();
        assert_eq!(annotated.commit, fixture.first);
        assert_eq!(annotated.bytes, b"first\n");
        let sha = fetch_git_registry(
            fixture.remote.to_str().unwrap(),
            &fixture.first,
            &language(),
            &cache,
        )
        .unwrap();
        assert_eq!(sha.commit, fixture.first);
        assert_eq!(
            conventional_git_registry_path(&language()),
            ".wax/registries/compose.json"
        );
    }

    #[test]
    fn locked_commit_survives_moving_tag_and_uses_cache() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let cache = fixture.root.join("cache");
        let old = fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &language(), &cache)
            .unwrap();
        assert_eq!(old.commit, fixture.second);
        let locked = fetch_git_registry_at_commit(
            fixture.remote.to_str().unwrap(),
            &fixture.first,
            &language(),
            &cache,
        )
        .unwrap();
        assert_eq!(locked.bytes, b"first\n");
        let again = fetch_git_registry_at_commit(
            fixture.remote.to_str().unwrap(),
            &fixture.first,
            &language(),
            &cache,
        )
        .unwrap();
        assert_eq!(again.commit, fixture.first);
        assert_eq!(again.bytes, b"first\n");
    }

    #[test]
    fn reports_missing_file_and_missing_ref() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let cache = fixture.root.join("cache");
        let missing = LanguageId::try_from("swift").unwrap();
        assert!(matches!(
            fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &missing, &cache),
            Err(RegistryGitError::RegistryFileAbsent { .. })
        ));
        assert!(matches!(
            fetch_git_registry(
                fixture.remote.to_str().unwrap(),
                "does-not-exist",
                &language(),
                &cache
            ),
            Err(RegistryGitError::FetchFailed { .. })
        ));
    }

    #[test]
    fn language_path_is_not_traversable() {
        let id = LanguageId::try_from("../secret");
        assert!(id.is_err());
        let id = LanguageId::try_from("compose").unwrap();
        assert_eq!(
            conventional_git_registry_path(&id),
            ".wax/registries/compose.json"
        );
    }

    #[test]
    fn different_urls_have_different_cache_repositories() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let other_root = fixture.root.join("other");
        fs::create_dir_all(&other_root).unwrap();
        let other_work = other_root.join("work");
        fs::create_dir_all(&other_work).unwrap();
        command(&["init", "-q"], &other_work);
        fs::create_dir_all(other_work.join(".wax/registries")).unwrap();
        fs::write(other_work.join(".wax/registries/compose.json"), b"other\n").unwrap();
        command(&["add", "."], &other_work);
        command(&["commit", "-m", "other", "-q"], &other_work);
        let other_remote = other_root.join("remote.git");
        command(
            &["init", "--bare", "-q", path_arg(&other_remote)],
            &other_root,
        );
        command(
            &["remote", "add", "origin", path_arg(&other_remote)],
            &other_work,
        );
        command(&["push", "-q", "origin", "HEAD"], &other_work);
        let cache = fixture.root.join("cache");
        fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &language(), &cache).unwrap();
        fetch_git_registry(other_remote.to_str().unwrap(), "HEAD", &language(), &cache).unwrap();
        let entries = fs::read_dir(cache).unwrap().count();
        assert_eq!(entries, 2);
    }
}
