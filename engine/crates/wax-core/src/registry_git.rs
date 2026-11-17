//! Fetches conventional registry files from Git repositories.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use thiserror::Error;
use wax_contract::LanguageId;

use crate::advisory_lock::AdvisoryLock;

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
    /// A full commit was unavailable without network access.
    #[error("Git commit {commit} is not available offline")]
    OfflineCacheMiss {
        /// Requested object id.
        commit: String,
    },
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
    /// The cache repository path is a symlink or escapes the cache directory.
    #[error("Git registry cache path {path} is unsafe (symlink or escapes cache directory)")]
    UnsafeCachePath {
        /// Rejected cache repository path.
        path: PathBuf,
    },
    /// The cache repository could not be locked for exclusive fetch access.
    #[error("failed to lock Git registry cache {path}: {source}")]
    CacheLock {
        /// Lock file path.
        path: PathBuf,
        /// Filesystem error.
        source: io::Error,
    },
    /// A Git process could not be spawned for a reason other than Git being absent.
    #[error("failed to run git for {operation}: {source}")]
    GitProcess {
        /// Operation being attempted.
        operation: &'static str,
        /// Process-spawn error.
        source: io::Error,
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
    let path = conventional_git_registry_path(language_id);
    let mut fetched = fetch_git_files(git_url, tag_or_sha, &[&path], cache_dir, false)?;
    Ok(GitRegistryFetch {
        commit: fetched.commit,
        bytes: fetched.files.remove(&path).unwrap_or_default(),
    })
}

/// Reads a registry at an exact full commit, fetching that object if needed.
///
/// # Errors
/// Returns a typed Git error for invalid or unavailable objects.
pub fn fetch_git_registry_at_commit(
    git_url: &str,
    commit: &str,
    language_id: &LanguageId,
    cache_dir: &Path,
) -> Result<GitRegistryFetch, RegistryGitError> {
    let commit = validate_commit(commit)?;
    fetch_git_registry(git_url, &commit, language_id, cache_dir)
}

/// Exact regular-file bytes read together at one immutable Git commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFilesFetch {
    /// Full commit object id.
    pub commit: String,
    /// Repository-relative paths and their exact bytes.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Fetches a reference once and reads regular blobs at that commit, without checkout.
/// Offline reads require a full commit already in the bare cache.
///
/// # Errors
/// Returns a typed error for unsafe paths, non-regular files, or unavailable objects.
pub fn fetch_git_files(
    url: &str,
    reference: &str,
    paths: &[&str],
    cache_dir: &Path,
    offline: bool,
) -> Result<GitFilesFetch, RegistryGitError> {
    validate_input("URL", url)?;
    validate_input("ref", reference)?;
    for path in paths {
        validate_git_path(path)?;
    }
    let full_commit = validate_commit(reference).ok();
    if offline && full_commit.is_none() {
        return Err(RegistryGitError::OfflineCacheMiss {
            commit: reference.to_owned(),
        });
    }
    let (repo, _lock) = prepare_cache(url, cache_dir)?;
    let cached = if let Some(commit) = &full_commit {
        let object = format!("{commit}^{{commit}}");
        let output = run_git(
            "check cached commit",
            [
                OsStr::new("--git-dir"),
                repo.as_os_str(),
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(&object),
            ],
        )?;
        if output.status.success() {
            Some(parse_commit(&output)?)
        } else {
            None
        }
    } else {
        None
    };
    let commit = if let Some(commit) = cached {
        commit
    } else {
        if offline {
            return Err(RegistryGitError::OfflineCacheMiss {
                commit: reference.to_owned(),
            });
        }
        let output = run_git(
            "fetch registry ref",
            [
                OsStr::new("--git-dir"),
                repo.as_os_str(),
                OsStr::new("fetch"),
                OsStr::new("--force"),
                OsStr::new("--depth=1"),
                OsStr::new("--no-tags"),
                OsStr::new("--"),
                OsStr::new(url),
                OsStr::new(reference),
            ],
        )?;
        if !output.status.success() {
            return Err(fetch_failed(url, reference, &output));
        }
        resolve_fetch_head(&repo)?
    };
    if let Some(expected) = full_commit
        && expected != commit
    {
        return Err(RegistryGitError::LockedCommitMismatch {
            requested: expected,
            resolved: commit,
        });
    }
    let files = paths
        .iter()
        .map(|path| Ok(((*path).to_owned(), read_blob(&repo, &commit, path)?)))
        .collect::<Result<_, RegistryGitError>>()?;
    Ok(GitFilesFetch { commit, files })
}

/// Validates a slash-separated, repository-relative Git artifact path.
///
/// # Errors
/// Rejects traversal, empty, option-like, absolute, backslash and control segments.
pub fn validate_git_path(path: &str) -> Result<(), RegistryGitError> {
    if wax_contract::validate_artifact_path(path).is_err() {
        return Err(RegistryGitError::InvalidInput {
            kind: "path",
            value: path.to_owned(),
        });
    }
    Ok(())
}

fn validate_input(kind: &'static str, value: &str) -> Result<(), RegistryGitError> {
    if value.trim().is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        let value = if kind == "URL" {
            redact_git_remote(value)
        } else {
            value.to_owned()
        };
        return Err(RegistryGitError::InvalidInput { kind, value });
    }
    Ok(())
}

/// Strips URL userinfo so Git remotes never echo embedded credentials in errors or labels.
pub(crate) fn redact_git_remote(git: &str) -> String {
    let Some(scheme_end) = git.find("://") else {
        return git.to_owned();
    };
    let after_scheme = scheme_end + 3;
    let Some(at_offset) = git[after_scheme..].find('@') else {
        return git.to_owned();
    };
    let host_start = after_scheme + at_offset + 1;
    if git[host_start..].is_empty() {
        return git.to_owned();
    }
    format!("{}{}", &git[..after_scheme], &git[host_start..])
}

fn fetch_failed(url: &str, reference: &str, output: &Output) -> RegistryGitError {
    let redacted_url = redact_git_remote(url);
    let mut stderr = stderr_summary(output);
    if url != redacted_url.as_str() {
        stderr = stderr.replace(url, &redacted_url);
    }
    RegistryGitError::FetchFailed {
        url: redacted_url,
        reference: reference.to_owned(),
        status: status_string(output),
        stderr,
    }
}

fn validate_commit(commit: &str) -> Result<String, RegistryGitError> {
    validate_input("commit", commit)?;
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RegistryGitError::InvalidInput {
            kind: "commit",
            value: commit.to_owned(),
        });
    }
    Ok(commit.to_ascii_lowercase())
}

fn prepare_cache(url: &str, cache_dir: &Path) -> Result<(PathBuf, AdvisoryLock), RegistryGitError> {
    let mut digest = Sha256::new();
    digest.update(url.as_bytes());
    let key: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let repo = cache_dir.join(&key);
    let lock_path = cache_dir.join(format!("{key}.lock"));

    fs::create_dir_all(cache_dir).map_err(|source| RegistryGitError::CacheDirectory {
        path: cache_dir.to_owned(),
        source,
    })?;
    let lock = AdvisoryLock::acquire(&lock_path).map_err(|source| RegistryGitError::CacheLock {
        path: lock_path.clone(),
        source,
    })?;
    ensure_safe_cache_repo(&repo, cache_dir)?;

    let metadata = match fs::symlink_metadata(&repo) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(RegistryGitError::CacheDirectory { path: repo, source });
        }
    };

    if metadata.is_none() {
        let output = run_git(
            "initialize cache",
            [
                OsStr::new("init"),
                OsStr::new("--bare"),
                OsStr::new("--"),
                repo.as_os_str(),
            ],
        )?;
        if !output.status.success() {
            return Err(RegistryGitError::CacheInit {
                path: repo,
                stderr: stderr_summary(&output),
            });
        }
        ensure_safe_cache_repo(&repo, cache_dir)?;
    }

    Ok((repo, lock))
}

fn ensure_safe_cache_repo(repo: &Path, cache_dir: &Path) -> Result<(), RegistryGitError> {
    match fs::symlink_metadata(repo) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(RegistryGitError::UnsafeCachePath {
                path: repo.to_owned(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(RegistryGitError::CacheDirectory {
                path: repo.to_owned(),
                source,
            });
        }
    }

    let canonical_cache =
        fs::canonicalize(cache_dir).map_err(|source| RegistryGitError::CacheDirectory {
            path: cache_dir.to_owned(),
            source,
        })?;
    let canonical_repo =
        fs::canonicalize(repo).map_err(|source| RegistryGitError::CacheDirectory {
            path: repo.to_owned(),
            source,
        })?;
    if !canonical_repo.starts_with(&canonical_cache) {
        return Err(RegistryGitError::UnsafeCachePath {
            path: repo.to_owned(),
        });
    }
    Ok(())
}

fn resolve_fetch_head(repo: &Path) -> Result<String, RegistryGitError> {
    let output = run_git(
        "resolve fetched commit",
        [
            OsStr::new("--git-dir"),
            repo.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("FETCH_HEAD^{commit}"),
        ],
    )?;
    if !output.status.success() {
        return Err(RegistryGitError::CommitResolution {
            stderr: stderr_summary(&output),
        });
    }
    parse_commit(&output)
}

fn parse_commit(output: &Output) -> Result<String, RegistryGitError> {
    let commit = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    if commit.is_empty() {
        return Err(RegistryGitError::CommitResolution {
            stderr: stderr_summary(output),
        });
    }
    Ok(commit)
}

fn read_blob(repo: &Path, commit: &str, path: &str) -> Result<Vec<u8>, RegistryGitError> {
    // ls-tree checks the mode as cat-file alone also accepts symlink blobs.
    let entry = run_git(
        "check regular blob",
        [
            OsStr::new("--git-dir"),
            repo.as_os_str(),
            OsStr::new("ls-tree"),
            OsStr::new("-z"),
            OsStr::new(commit),
            OsStr::new("--"),
            OsStr::new(path),
        ],
    )?;
    if entry.stdout.is_empty() {
        return Err(RegistryGitError::RegistryFileAbsent {
            path: path.to_owned(),
            commit: commit.to_owned(),
        });
    }
    if !entry.stdout.starts_with(b"100644 blob ") && !entry.stdout.starts_with(b"100755 blob ") {
        return Err(RegistryGitError::InvalidInput {
            kind: "regular blob path",
            value: path.to_owned(),
        });
    }
    let object = format!("{commit}:{path}");
    let output = run_git(
        "read registry file",
        [
            OsStr::new("--git-dir"),
            repo.as_os_str(),
            OsStr::new("cat-file"),
            OsStr::new("blob"),
            OsStr::new(&object),
        ],
    )?;
    if !output.status.success() {
        return Err(RegistryGitError::RegistryRead {
            path: path.to_owned(),
            commit: commit.to_owned(),
            stderr: stderr_summary(&output),
        });
    }
    Ok(output.stdout)
}

fn run_git<'a>(
    operation: &'static str,
    args: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Output, RegistryGitError> {
    Command::new("git").args(args).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            RegistryGitError::GitNotFound
        } else {
            RegistryGitError::GitProcess {
                operation,
                source: error,
            }
        }
    })
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

/// Canonical Git identity shared by config, cache, locks and offline validation.
#[must_use]
pub fn git_source_identity(
    url: &str,
    reference: &str,
    subdir: Option<&str>,
    system: Option<&str>,
) -> String {
    match subdir {
        None => format!("git:{}#{}", url.trim(), reference.trim()),
        Some(subdir) => {
            // A tuple serializes infallibly; the JSON array separates every field unambiguously.
            let canonical = serde_json::json!([
                redact_git_remote(url.trim()),
                reference.trim(),
                subdir,
                system
            ]);
            format!(
                "git-system:{}",
                crate::digest::sha256_hex(canonical.to_string().as_bytes())
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        work: PathBuf,
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
        command(
            &["init", "--bare", "-q", "--", remote.to_str().unwrap()],
            &root,
        );
        fs::create_dir_all(work.join(".wax/registries")).expect("registry directory");
        fs::write(work.join(".wax/registries/compose.json"), b"first\n").expect("first registry");
        command(&["add", "."], &work);
        command(&["commit", "-m", "first", "-q"], &work);
        let first = git_output(&["rev-parse", "HEAD"], &work);
        command(&["tag", "v1"], &work);
        command(&["tag", "-a", "v1-annotated", "-m", "annotated"], &work);
        command(
            &["remote", "add", "origin", remote.to_str().unwrap()],
            &work,
        );
        command(&["push", "-q", "origin", "--tags", "HEAD"], &work);

        fs::write(work.join(".wax/registries/compose.json"), b"second\n").expect("second registry");
        command(&["add", "."], &work);
        command(&["commit", "-m", "second", "-q"], &work);
        let second = git_output(&["rev-parse", "HEAD"], &work);
        command(&["push", "-q", "origin", "HEAD"], &work);

        Fixture {
            root,
            work,
            remote,
            first,
            second,
        }
    }

    impl Fixture {
        fn move_v1_tag(&self) {
            command(&["tag", "-f", "v1"], &self.work);
            command(&["push", "-q", "--force", "origin", "v1"], &self.work);
        }
    }

    fn git_output(args: &[&str], cwd: &Path) -> String {
        String::from_utf8(command(args, cwd).stdout)
            .expect("git output is UTF-8")
            .trim()
            .to_ascii_lowercase()
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
        assert_eq!(result.commit, fixture.first);
        assert_eq!(result.bytes, b"first\n");
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
        assert_eq!(old.commit, fixture.first);
        fixture.move_v1_tag();
        let moved = fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &language(), &cache)
            .unwrap();
        assert_eq!(moved.commit, fixture.second);
        assert_eq!(moved.bytes, b"second\n");
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
    fn fetch_failed_redacts_https_userinfo_from_url_and_stderr() {
        let url = "https://user:ghp_secret@github.com/org/repo.git";
        let output = Output {
            status: std::process::Command::new("false")
                .output()
                .expect("run false")
                .status,
            stdout: Vec::new(),
            stderr: format!("fatal: repository '{url}' not found\n").into_bytes(),
        };
        let error = fetch_failed(url, "v1", &output);
        let message = error.to_string();
        let RegistryGitError::FetchFailed {
            url: reported_url,
            stderr,
            ..
        } = error
        else {
            panic!("expected FetchFailed");
        };
        assert_eq!(reported_url, "https://github.com/org/repo.git");
        assert!(!reported_url.contains("secret"));
        assert!(!stderr.contains("secret"));
        assert!(stderr.contains("https://github.com/org/repo.git"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn redact_git_remote_strips_https_userinfo() {
        assert_eq!(
            redact_git_remote("https://user:ghp_secret@github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            redact_git_remote("https://github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            redact_git_remote("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
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
    fn rejects_option_shaped_git_inputs() {
        let cache = std::env::temp_dir().join(format!(
            "wax-registry-git-option-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&cache);
        assert!(matches!(
            fetch_git_registry("--upload-pack=./payload", "v1", &language(), &cache),
            Err(RegistryGitError::InvalidInput { kind: "URL", .. })
        ));
        assert!(matches!(
            fetch_git_registry(
                "https://example.invalid/repo.git",
                "--upload-pack=./payload",
                &language(),
                &cache
            ),
            Err(RegistryGitError::InvalidInput { kind: "ref", .. })
        ));
    }

    #[test]
    fn accepts_uppercase_locked_commit_ids() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let cache = fixture.root.join("cache");
        let uppercase = fixture.first.to_ascii_uppercase();
        assert_ne!(uppercase, fixture.first);
        let locked = fetch_git_registry_at_commit(
            fixture.remote.to_str().unwrap(),
            &uppercase,
            &language(),
            &cache,
        )
        .unwrap();
        assert_eq!(locked.commit, fixture.first);
        assert_eq!(locked.bytes, b"first\n");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_cache_repositories() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let cache = fixture.root.join("cache");
        fs::create_dir_all(&cache).unwrap();
        let mut digest = Sha256::new();
        digest.update(fixture.remote.to_str().unwrap().as_bytes());
        let key: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let repo = cache.join(&key);
        std::os::unix::fs::symlink(fixture.work.join(".git"), &repo).unwrap();
        assert!(matches!(
            fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &language(), &cache),
            Err(RegistryGitError::UnsafeCachePath { .. })
        ));
    }

    #[test]
    fn concurrent_fetches_share_cache_without_races() {
        if !git_available() {
            return;
        }
        let fixture = fixture();
        let cache = fixture.root.join("cache");
        let url = fixture.remote.to_str().unwrap().to_owned();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let url = url.clone();
                let cache = cache.clone();
                std::thread::spawn(move || {
                    fetch_git_registry(&url, "v1", &language(), &cache).unwrap()
                })
            })
            .collect();
        for handle in handles {
            let result = handle.join().expect("thread");
            assert_eq!(result.commit, fixture.first);
            assert_eq!(result.bytes, b"first\n");
        }
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
            &["init", "--bare", "-q", "--", other_remote.to_str().unwrap()],
            &other_root,
        );
        command(
            &["remote", "add", "origin", other_remote.to_str().unwrap()],
            &other_work,
        );
        command(&["push", "-q", "origin", "HEAD"], &other_work);
        let cache = fixture.root.join("cache");
        fetch_git_registry(fixture.remote.to_str().unwrap(), "v1", &language(), &cache).unwrap();
        fetch_git_registry(other_remote.to_str().unwrap(), "HEAD", &language(), &cache).unwrap();
        let entries = fs::read_dir(&cache)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_none())
            .count();
        assert_eq!(entries, 2);
    }
}
