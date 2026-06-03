# Registry Sources and Wax Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move repo-local wax files into `.wax/wax.*.json`, support optional local/hosted registry sources, and lock registry content by digest for deterministic validation and scans.

**Architecture:** Add repo file discovery helpers in `wax-core` so all commands can prefer the new `.wax/` layout while continuing to read legacy files. Add registry source parsing/resolution in `wax-core`, materialize non-repo-local registry content into `.wax/cache/registries/`, and rewrite language-pack config to a repo-relative local `registry` path before scan. Extend `wax.lock.json` with registry locks per language, add a lock refresh path through `wax language update`, and wire the new behavior through `validate`, `scan`, `init`, and user-facing docs.

**Tech Stack:** Rust 2024, serde JSON, reqwest blocking HTTP client, SHA-256 via `sha2`, existing `wax-core` and `wax-cli` test fixtures.

---

## Reference Spec

- Design spec: `docs/specs/2026-06-02-registry-sources-and-wax-layout-design.md`
- Active roadmap source: `docs/plans/README.md`
- Current product specs to keep consistent:
  - `docs/specs/2026-05-16-language-packs-and-distribution.md`
  - `docs/specs/2026-05-13-component-tracker-design.md`

## File Structure

- Create `engine/crates/wax-core/src/config/repo_files.rs`
  - Own discovery of `.wax/wax.config.json`, legacy `.waxrc`, `.wax/wax.lock.json`, and legacy `wax.lock.json`.
  - Own warnings for ignored legacy files when preferred files exist.
- Modify `engine/crates/wax-core/src/config.rs`
  - Export `repo_files`.
- Modify `engine/crates/wax-core/src/config/waxrc.rs`
  - Keep current `WaxRc` type name for compatibility.
  - Add typed per-language `registry` parsing while preserving `extra` passthrough for language-pack config.
- Modify `engine/crates/wax-core/src/config/lockfile.rs`
  - Add registry lock entries keyed by language id.
- Create `engine/crates/wax-core/src/registry_source.rs`
  - Parse `registry` and legacy `design_system_registry`.
  - Read/fetch registry content.
  - Validate JSON shape.
  - Compute SHA-256 digest.
  - Materialize external sources under `.wax/cache/registries/`.
  - Rewrite language config to a local repo-relative `registry` path.
  - Keep design-system registry source handling separate from `engine/crates/wax-core/src/registry.rs`, which remains the language-pack index loader.
- Modify `engine/crates/wax-core/src/lib.rs`
  - Use repo file discovery and registry source resolution before scan jobs are built.
  - Check registry lock digests during scan.
- Modify `engine/crates/wax-core/src/validate.rs`
  - Use repo file discovery and registry source resolution.
  - Warn for legacy files and deprecated `design_system_registry`.
  - Check registry locks.
- Modify `engine/crates/wax-cli/src/commands/init.rs`
  - Write `.wax/wax.config.json`, `.wax/wax.lock.json`, `.wax/wax.registry.json`.
  - Add `/.wax/cache/` and `/.wax/out/` to `.gitignore` without duplicates.
- Modify `engine/crates/wax-cli/src/commands/language.rs`
  - Use repo file discovery for `language update` and `language doctor` lock/config paths.
- Modify language packs:
  - `engine/crates/wax-lang-basic/src/line_scan.rs`
  - `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
  - Accept `registry` as the canonical local path and keep `design_system_registry` as a deprecated alias for the migration window.
- Update docs and fixtures:
  - `README.md`
  - `docs/specs/2026-05-16-language-packs-and-distribution.md`
  - `docs/specs/2026-05-13-component-tracker-design.md`
  - `engine/fixtures/config/example.waxrc`
  - `engine/fixtures/config/example-registry.json` if its path references need changing.
  - `engine/crates/wax-contract/schemas/waxrc.schema.json`

---

## Task 1: Repo File Discovery

**Files:**
- Create: `engine/crates/wax-core/src/config/repo_files.rs`
- Modify: `engine/crates/wax-core/src/config.rs`
- Test: `engine/crates/wax-core/tests/repo_files.rs`

- [x] **Step 1: Write failing tests for preferred and legacy config discovery**

Create `engine/crates/wax-core/tests/repo_files.rs` with:

```rust
use std::fs;
use wax_core::config::repo_files::{RepoFileSet, RepoFileWarning, discover_repo_files};

mod common;

#[test]
fn prefers_centralized_config_and_lock_paths() {
    let root = common::TestRepo::new();
    fs::create_dir_all(root.path.join(".wax")).unwrap();
    fs::write(root.path.join(".wax/wax.config.json"), "{}\n").unwrap();
    fs::write(root.path.join(".wax/wax.lock.json"), "{}\n").unwrap();
    fs::write(root.path.join(".waxrc"), "{}\n").unwrap();
    fs::write(root.path.join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(&root.path);

    assert_eq!(
        files,
        RepoFileSet {
            config_path: root.path.join(".wax/wax.config.json"),
            lockfile_path: root.path.join(".wax/wax.lock.json"),
            warnings: vec![
                RepoFileWarning::IgnoredLegacyConfig {
                    preferred: root.path.join(".wax/wax.config.json"),
                    legacy: root.path.join(".waxrc"),
                },
                RepoFileWarning::IgnoredLegacyLockfile {
                    preferred: root.path.join(".wax/wax.lock.json"),
                    legacy: root.path.join("wax.lock.json"),
                },
            ],
        }
    );
}

#[test]
fn falls_back_to_legacy_config_and_lock_paths() {
    let root = common::TestRepo::new();
    fs::write(root.path.join(".waxrc"), "{}\n").unwrap();
    fs::write(root.path.join("wax.lock.json"), "{}\n").unwrap();

    let files = discover_repo_files(&root.path);

    assert_eq!(files.config_path, root.path.join(".waxrc"));
    assert_eq!(files.lockfile_path, root.path.join("wax.lock.json"));
    assert!(files.warnings.is_empty());
}

#[test]
fn returns_preferred_paths_when_files_do_not_exist() {
    let root = common::TestRepo::new();

    let files = discover_repo_files(&root.path);

    assert_eq!(files.config_path, root.path.join(".wax/wax.config.json"));
    assert_eq!(files.lockfile_path, root.path.join(".wax/wax.lock.json"));
    assert!(files.warnings.is_empty());
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test repo_files
```

Expected: fail with unresolved import `wax_core::config::repo_files`.

- [x] **Step 3: Implement repo file discovery**

Add `engine/crates/wax-core/src/config/repo_files.rs`:

```rust
//! Repository-local wax file discovery.

use std::path::{Path, PathBuf};

/// Preferred repo-local wax config path.
pub const PREFERRED_CONFIG_RELATIVE_PATH: &str = ".wax/wax.config.json";
/// Legacy repo-local wax config path.
pub const LEGACY_CONFIG_RELATIVE_PATH: &str = ".waxrc";
/// Preferred repo-local wax lockfile path.
pub const PREFERRED_LOCKFILE_RELATIVE_PATH: &str = ".wax/wax.lock.json";
/// Legacy repo-local wax lockfile path.
pub const LEGACY_LOCKFILE_RELATIVE_PATH: &str = "wax.lock.json";
/// Default local registry path used when language config omits `registry`.
pub const DEFAULT_REGISTRY_RELATIVE_PATH: &str = ".wax/wax.registry.json";
/// Registry cache directory used for materialized external sources.
pub const REGISTRY_CACHE_RELATIVE_DIR: &str = ".wax/cache/registries";
/// Generated scan output directory.
pub const SCAN_OUTPUT_RELATIVE_DIR: &str = ".wax/out";

/// Repo-local wax files selected for a command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFileSet {
    /// Selected config path.
    pub config_path: PathBuf,
    /// Selected lockfile path.
    pub lockfile_path: PathBuf,
    /// Non-fatal discovery warnings.
    pub warnings: Vec<RepoFileWarning>,
}

/// Warnings emitted when legacy files are present but ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoFileWarning {
    /// A legacy `.waxrc` exists but the preferred config exists too.
    IgnoredLegacyConfig {
        /// Preferred config path.
        preferred: PathBuf,
        /// Ignored legacy config path.
        legacy: PathBuf,
    },
    /// A legacy top-level lockfile exists but the preferred lockfile exists too.
    IgnoredLegacyLockfile {
        /// Preferred lockfile path.
        preferred: PathBuf,
        /// Ignored legacy lockfile path.
        legacy: PathBuf,
    },
}

/// Discovers preferred or legacy wax repo files under `repo_root`.
pub fn discover_repo_files(repo_root: impl AsRef<Path>) -> RepoFileSet {
    let repo_root = repo_root.as_ref();
    let preferred_config = repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    let legacy_config = repo_root.join(LEGACY_CONFIG_RELATIVE_PATH);
    let preferred_lock = repo_root.join(PREFERRED_LOCKFILE_RELATIVE_PATH);
    let legacy_lock = repo_root.join(LEGACY_LOCKFILE_RELATIVE_PATH);

    let mut warnings = Vec::new();

    let config_path = if preferred_config.is_file() {
        if legacy_config.is_file() {
            warnings.push(RepoFileWarning::IgnoredLegacyConfig {
                preferred: preferred_config.clone(),
                legacy: legacy_config,
            });
        }
        preferred_config
    } else if legacy_config.is_file() {
        legacy_config
    } else {
        preferred_config
    };

    let lockfile_path = if preferred_lock.is_file() {
        if legacy_lock.is_file() {
            warnings.push(RepoFileWarning::IgnoredLegacyLockfile {
                preferred: preferred_lock.clone(),
                legacy: legacy_lock,
            });
        }
        preferred_lock
    } else if legacy_lock.is_file() {
        legacy_lock
    } else {
        preferred_lock
    };

    RepoFileSet {
        config_path,
        lockfile_path,
        warnings,
    }
}
```

Modify `engine/crates/wax-core/src/config.rs`:

```rust
//! Repository configuration loading.

pub mod lockfile;
pub mod repo_files;
pub mod waxrc;
```

- [x] **Step 4: Run tests to verify they pass**

Run:

```bash
cd engine
cargo test -p wax-core --test repo_files
```

Expected: pass.

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-core/src/config.rs engine/crates/wax-core/src/config/repo_files.rs engine/crates/wax-core/tests/repo_files.rs
git commit -m "feat: discover centralized wax repo files"
```

---

## Task 2: Registry Config Parsing

**Files:**
- Modify: `engine/crates/wax-core/src/config/waxrc.rs`
- Test: `engine/crates/wax-core/tests/waxrc_load.rs`
- Fixture: `engine/fixtures/config/with-registry-string.waxrc`
- Fixture: `engine/fixtures/config/with-registry-object.waxrc`

- [x] **Step 1: Add failing config parser tests**

Append to `engine/crates/wax-core/tests/waxrc_load.rs`:

```rust
#[test]
fn parses_registry_string_without_removing_pack_config() {
    let rc = load_waxrc(fixture_path("with-registry-string.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(
        language.registry_source().unwrap().source,
        ".wax/compose.registry.json"
    );
    assert_eq!(
        language.extra["registry"],
        serde_json::Value::String(".wax/compose.registry.json".to_owned())
    );
    assert_eq!(
        language.extra["roots"],
        serde_json::json!(["app/src/main/kotlin"])
    );
}

#[test]
fn parses_registry_source_object() {
    let rc = load_waxrc(fixture_path("with-registry-object.waxrc")).unwrap();
    let language = &rc.languages[0];

    assert_eq!(
        language.registry_source().unwrap().source,
        "https://example.com/acme-ds/registry/v2.4.1/compose.json"
    );
}
```

Create fixture `engine/fixtures/config/with-registry-string.waxrc`:

```json
{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "registry": ".wax/compose.registry.json",
      "roots": ["app/src/main/kotlin"]
    }
  ]
}
```

Create fixture `engine/fixtures/config/with-registry-object.waxrc`:

```json
{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "registry": {
        "source": "https://example.com/acme-ds/registry/v2.4.1/compose.json"
      },
      "roots": ["app/src/main/kotlin"]
    }
  ]
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test waxrc_load parses_registry
```

Expected: fail with no method named `registry_source`.

- [x] **Step 3: Add typed registry config accessors**

Modify `engine/crates/wax-core/src/config/waxrc.rs` by adding these types near `LanguageEntry`:

```rust
/// Parsed registry source setting from a language entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRegistrySource {
    /// Raw source string from `registry`, `registry.source`, or legacy `design_system_registry`.
    pub source: String,
    /// Field path source used for diagnostics.
    pub field_name: &'static str,
    /// Whether this came from deprecated `design_system_registry`.
    pub deprecated: bool,
}

impl LanguageEntry {
    /// Returns the configured registry source if one was declared.
    pub fn registry_source(&self) -> Option<LanguageRegistrySource> {
        if let Some(value) = self.extra.get("registry") {
            match value {
                serde_json::Value::String(source) => {
                    return Some(LanguageRegistrySource {
                        source: source.clone(),
                        field_name: "registry",
                        deprecated: false,
                    });
                }
                serde_json::Value::Object(object) => {
                    if let Some(source) = object.get("source").and_then(serde_json::Value::as_str) {
                        return Some(LanguageRegistrySource {
                            source: source.to_owned(),
                            field_name: "registry.source",
                            deprecated: false,
                        });
                    }
                }
                _ => {}
            }
        }

        self.extra
            .get("design_system_registry")
            .and_then(serde_json::Value::as_str)
            .map(|source| LanguageRegistrySource {
                source: source.to_owned(),
                field_name: "design_system_registry",
                deprecated: true,
            })
    }
}
```

Update `WaxRcError` display strings and docs in this file from `.waxrc` to
`wax config` where the error can now refer to either `.wax/wax.config.json` or
legacy `.waxrc`. Keep the concrete path in each error so users can see which
file failed.

- [x] **Step 4: Run tests to verify they pass**

Run:

```bash
cd engine
cargo test -p wax-core --test waxrc_load parses_registry
```

Expected: pass.

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-core/src/config/waxrc.rs engine/crates/wax-core/tests/waxrc_load.rs engine/fixtures/config/with-registry-string.waxrc engine/fixtures/config/with-registry-object.waxrc
git commit -m "feat: parse registry config sources"
```

---

## Task 3: Registry Source Resolution

**Files:**
- Create: `engine/crates/wax-core/src/registry_source.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Test: `engine/crates/wax-core/tests/registry_source.rs`

- [x] **Step 1: Write failing registry source tests**

Create `engine/crates/wax-core/tests/registry_source.rs`:

```rust
use std::fs;
use wax_core::registry_source::{
    RegistrySourceError, RegistrySourceInput, resolve_registry_source,
};

mod common;

const REGISTRY_JSON: &str = r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton"}]}"#;

#[test]
fn missing_registry_defaults_to_centralized_local_registry() {
    let repo = common::TestRepo::new();
    fs::create_dir_all(repo.path.join(".wax")).unwrap();
    fs::write(repo.path.join(".wax/wax.registry.json"), REGISTRY_JSON).unwrap();

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: &repo.path,
        language_id: "compose",
        source: None,
    })
    .unwrap();

    assert_eq!(resolved.repo_relative_path, ".wax/wax.registry.json");
    assert_eq!(resolved.sha256.len(), 64);
    assert!(!resolved.deprecated);
}

#[test]
fn registry_string_resolves_repo_relative_path() {
    let repo = common::TestRepo::new();
    fs::write(repo.path.join("compose.registry.json"), REGISTRY_JSON).unwrap();

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: &repo.path,
        language_id: "compose",
        source: Some("compose.registry.json"),
    })
    .unwrap();

    assert_eq!(resolved.repo_relative_path, "compose.registry.json");
}

#[test]
fn file_url_materializes_under_cache() {
    let repo = common::TestRepo::new();
    let outside = repo.path.with_extension("outside-registry.json");
    fs::write(&outside, REGISTRY_JSON).unwrap();

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: &repo.path,
        language_id: "compose",
        source: Some(&format!("file://{}", outside.display())),
    })
    .unwrap();

    assert!(resolved.repo_relative_path.starts_with(".wax/cache/registries/compose-"));
    assert!(repo.path.join(&resolved.repo_relative_path).is_file());
}

#[test]
fn http_url_materializes_under_cache() {
    let repo = common::TestRepo::new();
    let server = common::HttpFixtureServer::spawn(REGISTRY_JSON);

    let resolved = resolve_registry_source(RegistrySourceInput {
        repo_root: &repo.path,
        language_id: "compose",
        source: Some(&server.url()),
    })
    .unwrap();

    assert!(resolved.repo_relative_path.starts_with(".wax/cache/registries/compose-"));
    assert!(repo.path.join(&resolved.repo_relative_path).is_file());
}

#[test]
fn absolute_path_is_rejected() {
    let repo = common::TestRepo::new();
    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: &repo.path,
        language_id: "compose",
        source: Some("/tmp/registry.json"),
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::PlainAbsolutePath { .. }));
}

#[test]
fn malformed_registry_is_rejected() {
    let repo = common::TestRepo::new();
    fs::create_dir_all(repo.path.join(".wax")).unwrap();
    fs::write(repo.path.join(".wax/wax.registry.json"), "{\"components\":[]}").unwrap();

    let err = resolve_registry_source(RegistrySourceInput {
        repo_root: &repo.path,
        language_id: "compose",
        source: None,
    })
    .unwrap_err();

    assert!(matches!(err, RegistrySourceError::InvalidShape { .. }));
}
```

If `common::HttpFixtureServer` does not exist, add it to this test file as a
small `TcpListener` fixture like the local HTTP server in
`engine/crates/wax-core/src/registry.rs` tests. It should serve one 200 response
with `REGISTRY_JSON` and expose a `url()` method returning `http://127.0.0.1:<port>/registry.json`.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test registry_source
```

Expected: fail with unresolved module `registry_source`.

- [x] **Step 3: Implement registry source resolver**

Add `pub mod registry_source;` to `engine/crates/wax-core/src/lib.rs`.

Create `engine/crates/wax-core/src/registry_source.rs` with:

```rust
//! Design-system registry source resolution.

use crate::config::repo_files::{DEFAULT_REGISTRY_RELATIVE_PATH, REGISTRY_CACHE_RELATIVE_DIR};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Inputs for resolving one language registry.
#[derive(Debug, Clone, Copy)]
pub struct RegistrySourceInput<'a> {
    /// Repository root.
    pub repo_root: &'a Path,
    /// Language id string used for cache filenames.
    pub language_id: &'a str,
    /// Optional source string from config.
    pub source: Option<&'a str>,
}

/// Resolved registry ready for language-pack config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRegistrySource {
    /// Normalized source string used for lock comparison.
    pub source: String,
    /// Repo-relative materialized registry path.
    pub repo_relative_path: String,
    /// SHA-256 digest of registry content.
    pub sha256: String,
    /// Whether the source came from deprecated config.
    pub deprecated: bool,
}

/// Registry source resolution failures.
#[derive(Debug, Error)]
pub enum RegistrySourceError {
    /// Unsupported source scheme.
    #[error("unsupported registry source scheme in {source}; use repo-relative path, file://, http://, or https://")]
    UnsupportedScheme {
        /// Source string.
        source: String,
    },
    /// Plain absolute paths are not allowed.
    #[error("registry source {source} is an absolute path; use file:// for outside-repo files")]
    PlainAbsolutePath {
        /// Source string.
        source: String,
    },
    /// Repo-relative source attempted to escape the repo.
    #[error("registry source {source} must not escape the repository root")]
    PathEscapesRepo {
        /// Source string.
        source: String,
    },
    /// Invalid file URL.
    #[error("invalid file:// registry source {source}: {reason}")]
    InvalidFileUrl {
        /// Source string.
        source: String,
        /// Human-readable reason.
        reason: &'static str,
    },
    /// Registry could not be read.
    #[error("failed to read registry source {source}: {io}")]
    Read {
        /// Source string.
        source: String,
        /// Underlying I/O error.
        #[source]
        io: std::io::Error,
    },
    /// Registry could not be fetched.
    #[error("failed to fetch registry source {source}: {http}")]
    Fetch {
        /// Source string.
        source: String,
        /// Underlying HTTP error.
        #[source]
        http: reqwest::Error,
    },
    /// HTTP source returned a non-success status.
    #[error("failed to fetch registry source {source}: HTTP {status}")]
    HttpStatus {
        /// Source string.
        source: String,
        /// HTTP status code.
        status: reqwest::StatusCode,
    },
    /// Registry JSON is malformed.
    #[error("malformed registry JSON from {source}: {json}")]
    MalformedJson {
        /// Source string.
        source: String,
        /// JSON parse error.
        #[source]
        json: serde_json::Error,
    },
    /// Registry shape is invalid.
    #[error("invalid registry JSON from {source}: {reason}")]
    InvalidShape {
        /// Source string.
        source: String,
        /// Shape error.
        reason: &'static str,
    },
    /// Cache write failed.
    #[error("failed to materialize registry source {source} to {path}: {io}")]
    CacheWrite {
        /// Source string.
        source: String,
        /// Cache path.
        path: String,
        /// Underlying I/O error.
        #[source]
        io: std::io::Error,
    },
}

/// Resolves registry content and returns a local repo-relative registry path.
pub fn resolve_registry_source(
    input: RegistrySourceInput<'_>,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    resolve_registry_source_with_deprecation(input, false)
}

/// Resolves registry content and preserves whether the config key was deprecated.
pub fn resolve_registry_source_with_deprecation(
    input: RegistrySourceInput<'_>,
    deprecated: bool,
) -> Result<ResolvedRegistrySource, RegistrySourceError> {
    let source = input
        .source
        .filter(|source| !source.trim().is_empty())
        .unwrap_or(DEFAULT_REGISTRY_RELATIVE_PATH);

    let (bytes, repo_relative_path, external) = read_source(input.repo_root, source)?;
    validate_registry_json(source, &bytes)?;
    let sha256 = hex_lower_sha256(&bytes);

    let repo_relative_path = if external {
        materialize_external_registry(input.repo_root, input.language_id, source, &sha256, &bytes)?
    } else {
        repo_relative_path
    };

    Ok(ResolvedRegistrySource {
        source: source.to_owned(),
        repo_relative_path,
        sha256,
        deprecated,
    })
}

fn read_source(
    repo_root: &Path,
    source: &str,
) -> Result<(Vec<u8>, String, bool), RegistrySourceError> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let response = reqwest::blocking::get(source).map_err(|http| RegistrySourceError::Fetch {
            source: source.to_owned(),
            http,
        })?;
        if !response.status().is_success() {
            return Err(RegistrySourceError::HttpStatus {
                source: source.to_owned(),
                status: response.status(),
            });
        }
        return response
            .bytes()
            .map(|bytes| (bytes.to_vec(), String::new(), true))
            .map_err(|http| RegistrySourceError::Fetch {
                source: source.to_owned(),
                http,
            });
    }

    if source.starts_with("file://") {
        let path = file_url_to_path(source)?;
        let bytes = fs::read(&path).map_err(|io| RegistrySourceError::Read {
            source: source.to_owned(),
            io,
        })?;
        return Ok((bytes, String::new(), true));
    }

    if source.contains("://") {
        return Err(RegistrySourceError::UnsupportedScheme {
            source: source.to_owned(),
        });
    }

    let path = Path::new(source);
    if path.is_absolute() {
        return Err(RegistrySourceError::PlainAbsolutePath {
            source: source.to_owned(),
        });
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(RegistrySourceError::PathEscapesRepo {
            source: source.to_owned(),
        });
    }

    let bytes = fs::read(repo_root.join(path)).map_err(|io| RegistrySourceError::Read {
        source: source.to_owned(),
        io,
    })?;
    Ok((bytes, source.to_owned(), false))
}

fn validate_registry_json(source: &str, bytes: &[u8]) -> Result<(), RegistrySourceError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|json| RegistrySourceError::MalformedJson {
        source: source.to_owned(),
        json,
    })?;
    let Some(object) = value.as_object() else {
        return Err(RegistrySourceError::InvalidShape {
            source: source.to_owned(),
            reason: "expected top-level object",
        });
    };
    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(RegistrySourceError::InvalidShape {
            source: source.to_owned(),
            reason: "missing or unsupported schema_version",
        });
    }
    match object.get("components") {
        Some(Value::Array(_)) => Ok(()),
        Some(_) => Err(RegistrySourceError::InvalidShape {
            source: source.to_owned(),
            reason: "components must be an array",
        }),
        None => Err(RegistrySourceError::InvalidShape {
            source: source.to_owned(),
            reason: "missing components array",
        }),
    }
}

fn materialize_external_registry(
    repo_root: &Path,
    language_id: &str,
    source: &str,
    sha256: &str,
    bytes: &[u8],
) -> Result<String, RegistrySourceError> {
    let relative = format!("{REGISTRY_CACHE_RELATIVE_DIR}/{language_id}-{sha256}.json");
    let path = repo_root.join(&relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|io| RegistrySourceError::CacheWrite {
            source: source.to_owned(),
            path: path.display().to_string(),
            io,
        })?;
    }
    fs::write(&path, bytes).map_err(|io| RegistrySourceError::CacheWrite {
        source: source.to_owned(),
        path: path.display().to_string(),
        io,
    })?;
    Ok(relative)
}

fn hex_lower_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_url_to_path(url: &str) -> Result<PathBuf, RegistrySourceError> {
    let rest = url.strip_prefix("file://").ok_or_else(|| RegistrySourceError::InvalidFileUrl {
        source: url.to_owned(),
        reason: "missing file:// prefix",
    })?;
    if rest.is_empty() {
        return Err(RegistrySourceError::InvalidFileUrl {
            source: url.to_owned(),
            reason: "missing path",
        });
    }
    Ok(PathBuf::from(rest))
}
```

- [x] **Step 4: Run tests to verify they pass**

Run:

```bash
cd engine
cargo test -p wax-core --test registry_source
```

Expected: pass.

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-core/src/lib.rs engine/crates/wax-core/src/registry_source.rs engine/crates/wax-core/tests/registry_source.rs
git commit -m "feat: resolve design system registry sources"
```

---

## Task 4: Lockfile Registry Digests

**Files:**
- Modify: `engine/crates/wax-core/src/config/lockfile.rs`
- Fixture: `engine/fixtures/config/minimal.wax.lock.json`
- Fixture: `engine/fixtures/config/minimal-v1-no-registries.wax.lock.json`
- Test: `engine/crates/wax-core/tests/lockfile_load.rs`

- [x] **Step 1: Add failing lockfile schema and registry tests**

Append to `engine/crates/wax-core/tests/lockfile_load.rs`:

```rust
#[test]
fn parses_registry_locks() {
    let lock = load_lockfile(fixture_path("minimal.wax.lock.json")).unwrap();
    let registry = lock
        .registries
        .get(&"compose".parse().unwrap())
        .expect("compose registry lock should exist");

    assert_eq!(registry.source, ".wax/wax.registry.json");
    assert_eq!(
        registry.sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
}

#[test]
fn parses_v1_lockfile_without_registry_locks_for_migration() {
    let lock = load_lockfile(fixture_path("minimal-v1-no-registries.wax.lock.json")).unwrap();

    assert_eq!(lock.schema_version, 1);
    assert!(lock.registries.is_empty());
}
```

Update `engine/fixtures/config/minimal.wax.lock.json` by adding a top-level `registries` object:

```json
"registries": {
  "compose": {
    "source": ".wax/wax.registry.json",
    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  }
}
```

Place it after `locked_at` and before `languages`.

Create `engine/fixtures/config/minimal-v1-no-registries.wax.lock.json` by copying
the previous minimal lockfile shape without the new `registries` object. Keep
`schema_version` set to `1`.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test lockfile_load parses_registry_locks
```

Expected: fail because `WaxLock` has no `registries` field or schema v2 support.

- [x] **Step 3: Extend lockfile types and schema handling**

Change `WAX_LOCK_SCHEMA_VERSION` in `engine/crates/wax-core/src/config/lockfile.rs`:

```rust
/// Current `wax.lock.json` schema version written by this engine.
pub const WAX_LOCK_SCHEMA_VERSION: u32 = 2;
const MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION: u32 = 1;
```

Modify `engine/crates/wax-core/src/config/lockfile.rs`:

```rust
/// Repository lockfile pinning the language pack artifacts selected for a repo.
#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WaxLock {
    /// Version of the `wax.lock.json` JSON schema.
    pub schema_version: u32,
    /// Engine orchestration API version expected by the locked language packs.
    pub engine_api_version: u32,
/// Version of the wax engine that wrote this lockfile.
pub wax_version: String,
/// Time this lockfile was produced, when recorded by the writer.
#[serde(default, with = "time::serde::rfc3339::option")]
pub locked_at: Option<OffsetDateTime>,
/// Locked design-system registry sources by language id.
pub registries: BTreeMap<LanguageId, LockedRegistry>,
/// Locked language pack artifacts by validated language id.
pub languages: BTreeMap<LanguageId, LockedLanguage>,
}

/// Lockfile entry for one resolved design-system registry.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedRegistry {
    /// Registry source string from config or default resolution.
    pub source: String,
    /// SHA-256 digest of the exact registry JSON content.
    pub sha256: String,
}
```

Update all `WaxLock` literal construction sites to include:

```rust
registries: BTreeMap::new(),
```

Update schema validation in `load_lockfile` so versions `1` and `2` are accepted:

```rust
if version.schema_version < MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION
    || version.schema_version > WAX_LOCK_SCHEMA_VERSION
{
    return Err(LockfileError::UnsupportedSchemaVersion {
        path: path_display,
        found: version.schema_version,
        min_supported: MIN_SUPPORTED_WAX_LOCK_SCHEMA_VERSION,
        max_supported: WAX_LOCK_SCHEMA_VERSION,
    });
}
```

Before deserializing into `WaxLock`, inject an empty `registries` object only for
schema version `1` inputs so migration stays backward-compatible while schema
version `2` still requires an explicit `registries` field.

Writers must emit schema version `2`. Readers accept v1 so existing repositories
can run the lock refresh command before strict registry-lock checks are enforced.

- [x] **Step 4: Run focused tests**

Run:

```bash
cd engine
cargo test -p wax-core --test lockfile_load parses_registry_locks
```

Expected: pass.

- [x] **Step 5: Run broader lockfile tests**

Run:

```bash
cd engine
cargo test -p wax-core --test lockfile_load
```

Expected: pass.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-core/src/config/lockfile.rs engine/crates/wax-core/tests/lockfile_load.rs engine/fixtures/config/minimal.wax.lock.json engine/fixtures/config/minimal-v1-no-registries.wax.lock.json
git commit -m "feat: lock registry source digests"
```

---

## Task 5: Validate Registry Sources and Layout

**Files:**
- Modify: `engine/crates/wax-core/src/validate.rs`
- Test: `engine/crates/wax-core/tests/validate_repo.rs`
- Test: `engine/crates/wax-cli/tests/validate_command.rs`

- [x] **Step 1: Add failing validation tests for default registry and deprecated alias**

Append to `engine/crates/wax-core/tests/validate_repo.rs`:

```rust
#[test]
fn validate_repo_accepts_default_centralized_registry() {
    let root = TestRepo::new();
    fs::create_dir_all(root.path.join(".wax")).unwrap();
    fs::write(
        root.path.join(".wax/wax.config.json"),
        r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true}]}"#,
    )
    .unwrap();
    fs::write(
        root.path.join(".wax/wax.registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    write_lockfile_with_registry(&root.path, ".wax/wax.registry.json");

    let report = validate_repo(&root.path).unwrap();

    assert!(report.warnings.is_empty());
}

#[test]
fn validate_repo_warns_for_legacy_design_system_registry() {
    let root = TestRepo::new();
    fs::write(
        root.path.join(".waxrc"),
        r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true,"design_system_registry":"design-system/registry.json"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(root.path.join("design-system")).unwrap();
    fs::write(
        root.path.join("design-system/registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    write_legacy_lockfile_with_registry(&root.path, "design-system/registry.json");

    let report = validate_repo(&root.path).unwrap();

    assert!(report.warnings.iter().any(|warning| {
        matches!(warning, ValidateWarning::DeprecatedDesignSystemRegistry { .. })
    }));
}
```

Add helper functions in the same file:

```rust
fn write_lockfile_with_registry(repo_root: &Path, source: &str) {
    fs::write(repo_root.join(".wax/wax.lock.json"), lockfile_json(repo_root, source)).unwrap();
}

fn write_legacy_lockfile_with_registry(repo_root: &Path, source: &str) {
    fs::write(repo_root.join("wax.lock.json"), lockfile_json(repo_root, source)).unwrap();
}

fn lockfile_json(repo_root: &Path, source: &str) -> String {
    let resolved = wax_core::registry_source::resolve_registry_source(
        wax_core::registry_source::RegistrySourceInput {
            repo_root,
            language_id: "compose",
            source: Some(source),
        },
    )
    .unwrap();
    format!(
        r#"{{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "locked_at": null,
  "registries": {{
    "compose": {{
      "source": "{source}",
      "sha256": "{}"
    }}
  }},
  "languages": {{
    "compose": {{
      "version": "0.1.0-alpha.0",
      "api_version": 1,
      "source": "file:///tmp/index.json",
      "resolved": {{
        "target": "x86_64-unknown-linux-gnu",
        "url": "file:///tmp/wax-lang-compose.tar.gz",
        "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "signature": null
      }}
    }}
  }}
}}"#,
        resolved.sha256
    )
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test validate_repo validate_repo_accepts_default_centralized_registry validate_repo_warns_for_legacy_design_system_registry
```

Expected: fail because validate still requires `design_system_registry`.

- [x] **Step 3: Implement validate integration**

Modify `ValidateWarning` in `engine/crates/wax-core/src/validate.rs`:

```rust
/// Non-fatal warnings that should be shown to users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateWarning {
    /// Registry parsed but did not declare components.
    EmptyRegistryComponents {
        /// Language this registry belongs to.
        language_id: LanguageId,
        /// Registry path relative to repo root.
        registry_path: String,
    },
    /// Deprecated `design_system_registry` key was used.
    DeprecatedDesignSystemRegistry {
        /// Language this registry belongs to.
        language_id: LanguageId,
        /// Field path.
        field: String,
    },
    /// Legacy config was ignored in favor of centralized config.
    IgnoredLegacyConfig {
        /// Ignored legacy config path.
        path: String,
    },
    /// Legacy lockfile was ignored in favor of centralized lockfile.
    IgnoredLegacyLockfile {
        /// Ignored legacy lockfile path.
        path: String,
    },
}
```

Add validation errors:

```rust
/// Registry source resolution failed.
#[error("invalid .wax config field {field}: {source}")]
RegistrySource {
    /// Config field path.
    field: String,
    /// Source error.
    #[source]
    source: crate::registry_source::RegistrySourceError,
},
/// Enabled language registry is missing from lockfile.
#[error("enabled language {language_id} registry is missing from wax lockfile")]
MissingRegistryLock {
    /// Language id.
    language_id: LanguageId,
},
/// Enabled language registry digest differs from lockfile.
#[error("enabled language {language_id} registry digest drift: lockfile={lockfile_sha256} resolved={resolved_sha256}")]
RegistryDigestDrift {
    /// Language id.
    language_id: LanguageId,
    /// Lockfile digest.
    lockfile_sha256: String,
    /// Resolved digest.
    resolved_sha256: String,
},
```

Change `validate_repo` to:

```rust
let repo_files = crate::config::repo_files::discover_repo_files(repo_root);
let waxrc = load_waxrc(&repo_files.config_path)?;
if !enabled.is_empty() {
    load_lockfile(&repo_files.lockfile_path)?;
}
```

Then, for each enabled language:

```rust
let registry_setting = entry.registry_source();
let field = format!(
    "languages[{index}].{}",
    registry_setting
        .as_ref()
        .map(|setting| setting.field_name)
        .unwrap_or("registry")
);
let resolved = crate::registry_source::resolve_registry_source_with_deprecation(
    crate::registry_source::RegistrySourceInput {
        repo_root,
        language_id: entry.id.as_str(),
        source: registry_setting.as_ref().map(|setting| setting.source.as_str()),
    },
    registry_setting.as_ref().is_some_and(|setting| setting.deprecated),
)
.map_err(|source| ValidateError::RegistrySource {
    field: field.clone(),
    source,
})?;
if resolved.deprecated {
    warnings.push(ValidateWarning::DeprecatedDesignSystemRegistry {
        language_id: entry.id.clone(),
        field: field.clone(),
    });
}
```

Use `resolved.repo_relative_path` for `EmptyRegistryComponents`.

Load the lockfile once and compare:

```rust
let lockfile = if enabled.is_empty() {
    None
} else {
    Some(load_lockfile(&repo_files.lockfile_path)?)
};
if let Some(lockfile) = &lockfile {
    let Some(locked_registry) = lockfile.registries.get(&entry.id) else {
        return Err(ValidateError::MissingRegistryLock {
            language_id: entry.id.clone(),
        });
    };
    if locked_registry.sha256 != resolved.sha256 {
        return Err(ValidateError::RegistryDigestDrift {
            language_id: entry.id.clone(),
            lockfile_sha256: locked_registry.sha256.clone(),
            resolved_sha256: resolved.sha256,
        });
    }
}
```

Convert repo file warnings into validate warnings:

```rust
for warning in repo_files.warnings {
    match warning {
        crate::config::repo_files::RepoFileWarning::IgnoredLegacyConfig { legacy, .. } => {
            warnings.push(ValidateWarning::IgnoredLegacyConfig {
                path: legacy.display().to_string(),
            });
        }
        crate::config::repo_files::RepoFileWarning::IgnoredLegacyLockfile { legacy, .. } => {
            warnings.push(ValidateWarning::IgnoredLegacyLockfile {
                path: legacy.display().to_string(),
            });
        }
    }
}
```

- [x] **Step 4: Update CLI warning rendering**

Modify `engine/crates/wax-cli/src/commands/validate.rs` warning rendering to include:

```rust
ValidateWarning::DeprecatedDesignSystemRegistry { language_id, field } => {
    writeln!(
        stderr,
        "warning: language {language_id} uses deprecated {field}; use registry instead"
    )
}
ValidateWarning::IgnoredLegacyConfig { path } => {
    writeln!(stderr, "warning: ignored legacy config {path}")
}
ValidateWarning::IgnoredLegacyLockfile { path } => {
    writeln!(stderr, "warning: ignored legacy lockfile {path}")
}
```

- [x] **Step 5: Run focused validation tests**

Run:

```bash
cd engine
cargo test -p wax-core --test validate_repo
cargo test -p wax-cli --test validate_command
```

Expected: pass.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-core/src/validate.rs engine/crates/wax-core/tests/validate_repo.rs engine/crates/wax-cli/src/commands/validate.rs engine/crates/wax-cli/tests/validate_command.rs
git commit -m "feat: validate registry sources"
```

---

## Task 6: Scan Registry Sources and Rewrite Pack Config

**Files:**
- Modify: `engine/crates/wax-core/src/lib.rs`
- Test: `engine/crates/wax-core/tests/scan_resolve.rs`

- [x] **Step 1: Add failing scan tests for default registry and remote materialization**

Append to `engine/crates/wax-core/tests/scan_resolve.rs`:

```rust
#[test]
fn scan_repo_rewrites_default_registry_to_pack_config() {
    let fixture = ScanFixture::new();
    fixture.write_centralized_config_without_registry();
    fixture.write_default_registry();
    fixture.write_lockfile_with_registry(".wax/wax.registry.json");
    fixture.install_ready_pack("compose");

    Engine::scan_repo(&fixture.repo).expect("scan should pass");

    let request = fixture.read_last_pack_request("compose");
    assert_eq!(request["config"]["registry"], ".wax/wax.registry.json");
    assert!(request["config"].get("design_system_registry").is_none());
}

#[test]
fn scan_repo_materializes_file_registry_before_pack_spawn() {
    let fixture = ScanFixture::new();
    let outside = fixture.repo.with_extension("external-registry.json");
    std::fs::write(
        &outside,
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    let source = format!("file://{}", outside.display());
    fixture.write_centralized_config_with_registry_source(&source);
    fixture.write_lockfile_with_registry(&source);
    fixture.install_ready_pack("compose");

    Engine::scan_repo(&fixture.repo).expect("scan should pass");

    let request = fixture.read_last_pack_request("compose");
    let registry = request["config"]["registry"].as_str().unwrap();
    assert!(registry.starts_with(".wax/cache/registries/compose-"));
    assert!(fixture.repo.join(registry).is_file());
}
```

Add fixture helper methods in `scan_resolve.rs`:

```rust
impl ScanFixture {
    fn write_centralized_config_without_registry(&self) {
        std::fs::create_dir_all(self.repo.join(".wax")).unwrap();
        std::fs::write(
            self.repo.join(".wax/wax.config.json"),
            r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true,"roots":["src"]}]}"#,
        )
        .unwrap();
    }

    fn write_centralized_config_with_registry_source(&self, source: &str) {
        std::fs::create_dir_all(self.repo.join(".wax")).unwrap();
        let config = serde_json::json!({
            "schema_version": 1,
            "languages": [
                {
                    "id": "compose",
                    "enabled": true,
                    "registry": {
                        "source": source
                    },
                    "roots": ["src"]
                }
            ]
        });
        std::fs::write(
            self.repo.join(".wax/wax.config.json"),
            format!("{}\n", serde_json::to_string(&config).unwrap()),
        )
        .unwrap();
    }

    fn write_default_registry(&self) {
        std::fs::create_dir_all(self.repo.join(".wax")).unwrap();
        std::fs::write(
            self.repo.join(".wax/wax.registry.json"),
            r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
        )
        .unwrap();
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test scan_resolve scan_repo_rewrites_default_registry_to_pack_config scan_repo_materializes_file_registry_before_pack_spawn
```

Expected: fail because scan still loads `.waxrc` and passes old config unchanged.

- [x] **Step 3: Implement scan integration**

Modify `Engine::scan_repo_with_options` in `engine/crates/wax-core/src/lib.rs`:

```rust
let repo_files = config::repo_files::discover_repo_files(repo_root);
let waxrc = load_waxrc(&repo_files.config_path)?;
let scan_concurrency = effective_scan_concurrency(&waxrc.engine, &options);
let lockfile = load_lockfile(&repo_files.lockfile_path)?;
```

When collecting enabled languages, resolve and rewrite config:

```rust
let mut language_configs = BTreeMap::new();
for entry in waxrc.languages {
    if !entry.enabled {
        continue;
    }
    let registry_setting = entry.registry_source();
    let resolved_registry = registry_source::resolve_registry_source_with_deprecation(
        registry_source::RegistrySourceInput {
            repo_root,
            language_id: entry.id.as_str(),
            source: registry_setting.as_ref().map(|setting| setting.source.as_str()),
        },
        registry_setting.as_ref().is_some_and(|setting| setting.deprecated),
    )?;
    verify_registry_lock(&entry.id, &resolved_registry, &lockfile)?;

    let mut config = entry.extra;
    config.remove("design_system_registry");
    config.insert(
        "registry".to_owned(),
        serde_json::Value::String(resolved_registry.repo_relative_path),
    );
    language_configs.insert(entry.id.clone(), config);
    enabled_ids.insert(entry.id);
}
```

Add an `EngineError` variant:

```rust
/// Registry source could not be resolved for scan.
#[error(transparent)]
RegistrySource(#[from] registry_source::RegistrySourceError),
/// Registry lock did not match resolved registry content.
#[error("registry lock mismatch for language {language_id}: {reason}")]
RegistryLock {
    /// Language id.
    language_id: LanguageId,
    /// Reason.
    reason: String,
},
```

Add helper:

```rust
fn verify_registry_lock(
    language_id: &LanguageId,
    resolved: &registry_source::ResolvedRegistrySource,
    lockfile: &config::lockfile::WaxLock,
) -> Result<(), EngineError> {
    let locked = lockfile
        .registries
        .get(language_id)
        .ok_or_else(|| EngineError::RegistryLock {
            language_id: language_id.clone(),
            reason: "missing registry lock entry".to_owned(),
        })?;
    if locked.source != resolved.source {
        return Err(EngineError::RegistryLock {
            language_id: language_id.clone(),
            reason: format!("source changed from {} to {}", locked.source, resolved.source),
        });
    }
    if locked.sha256 != resolved.sha256 {
        return Err(EngineError::RegistryLock {
            language_id: language_id.clone(),
            reason: format!("digest changed from {} to {}", locked.sha256, resolved.sha256),
        });
    }
    Ok(())
}
```

- [x] **Step 4: Run focused scan tests**

Run:

```bash
cd engine
cargo test -p wax-core --test scan_resolve scan_repo_rewrites_default_registry_to_pack_config scan_repo_materializes_file_registry_before_pack_spawn
```

Expected: pass.

- [x] **Step 5: Run scan test suite**

Run:

```bash
cd engine
cargo test -p wax-core --test scan_resolve
cargo test -p wax-core --test scan_auto_install
cargo test -p wax-core --test scan_output
cargo test -p wax-core --test scan_concurrency
```

Expected: pass after updating test fixture lockfiles with `registries`.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-core/src/lib.rs engine/crates/wax-core/tests/scan_resolve.rs engine/crates/wax-core/tests/scan_auto_install.rs engine/crates/wax-core/tests/scan_output.rs engine/crates/wax-core/tests/scan_concurrency.rs
git commit -m "feat: apply registry sources during scan"
```

---

## Task 7: Language Pack Registry Alias

**Files:**
- Modify: `engine/crates/wax-lang-basic/src/line_scan.rs`
- Modify: `engine/crates/wax-lang-basic/tests/config_validation.rs`
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-compose/tests/config_validation.rs`

- [x] **Step 1: Add failing tests for canonical `registry` key**

In both config validation test files, add:

```rust
#[test]
fn registry_key_is_accepted_as_canonical_registry_path() {
    let mut config = valid_config();
    let registry = config.remove("design_system_registry").unwrap();
    config.insert("registry".to_owned(), registry);

    let facts = scan_with_config(config).expect("registry key should scan");

    assert_eq!(facts.counts.design_system_component_count, 2);
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-lang-basic --test config_validation registry_key_is_accepted_as_canonical_registry_path
cargo test -p wax-lang-compose --test config_validation registry_key_is_accepted_as_canonical_registry_path
```

Expected: fail because parsers require `design_system_registry`.

- [x] **Step 3: Update basic parser**

Modify `parse_basic_scan_config` in `engine/crates/wax-lang-basic/src/line_scan.rs`:

```rust
let has_registry = config.contains_key("registry") || config.contains_key("design_system_registry");
```

Replace the registry lookup with:

```rust
let registry = config
    .get("registry")
    .or_else(|| config.get("design_system_registry"))
    .ok_or_else(|| LineScanError::ConfigInvalid {
        reason: "registry is required when basic scan config is present".to_owned(),
    })?;
let registry = registry
    .as_str()
    .ok_or_else(|| LineScanError::ConfigInvalid {
        reason: "registry must be a non-empty string".to_owned(),
    })?;
if registry.is_empty() {
    return Err(LineScanError::ConfigInvalid {
        reason: "registry must be a non-empty string".to_owned(),
    });
}
validate_repo_relative_path(registry, "registry")?;
```

- [x] **Step 4: Update compose parser**

Modify `parse_compose_scan_config` in `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs` the same way:

```rust
let has_registry = config.contains_key("registry") || config.contains_key("design_system_registry");
```

and:

```rust
let registry = config
    .get("registry")
    .or_else(|| config.get("design_system_registry"))
    .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
        reason: "registry is required when compose scan config is present".to_owned(),
    })?;
let registry = registry
    .as_str()
    .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
        reason: "registry must be a non-empty string".to_owned(),
    })?;
if registry.is_empty() {
    return Err(TreeSitterScanError::ConfigInvalid {
        reason: "registry must be a non-empty string".to_owned(),
    });
}
```

The internal struct field may remain named `design_system_registry: PathBuf` in
this task if that keeps the change small. The user-facing config key and error
messages should say `registry`; internal renaming can happen in a later cleanup
if it becomes confusing.

- [x] **Step 5: Run focused tests**

Run:

```bash
cd engine
cargo test -p wax-lang-basic --test config_validation
cargo test -p wax-lang-compose --test config_validation
```

Expected: pass.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-lang-basic/src/line_scan.rs engine/crates/wax-lang-basic/tests/config_validation.rs engine/crates/wax-lang-compose/src/tree_sitter_scan.rs engine/crates/wax-lang-compose/tests/config_validation.rs
git commit -m "feat: accept registry key in language packs"
```

---

## Task 8: Centralized Init Layout

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Modify: `engine/crates/wax-cli/tests/init_command.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`

- [x] **Step 1: Add failing init tests**

Append to `engine/crates/wax-cli/tests/init_command.rs`:

```rust
#[test]
fn init_writes_centralized_wax_layout_and_gitignore() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    fs::write(repo.join(".gitignore"), "target/\n").unwrap();

    run_init_for_test(repo, &["compose"]);

    assert!(repo.join(".wax/wax.config.json").is_file());
    assert!(repo.join(".wax/wax.lock.json").is_file());
    assert!(repo.join(".wax/wax.registry.json").is_file());
    assert!(!repo.join(".waxrc").exists());
    assert!(!repo.join("wax.lock.json").exists());

    let gitignore = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/.wax/cache/"));
    assert!(gitignore.contains("/.wax/out/"));
}

#[test]
fn init_does_not_duplicate_gitignore_entries() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    fs::write(repo.join(".gitignore"), "/.wax/cache/\n/.wax/out/\n").unwrap();

    run_init_for_test(repo, &["compose"]);

    let gitignore = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert_eq!(gitignore.matches("/.wax/cache/").count(), 1);
    assert_eq!(gitignore.matches("/.wax/out/").count(), 1);
}

#[test]
fn init_scaffolds_only_default_centralized_registry() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();

    run_init_for_test(repo, &["compose"]);

    assert!(repo.join(".wax/wax.registry.json").is_file());
    assert!(!repo.join("design-system/registry.json").exists());
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-cli --test init_command init_writes_centralized_wax_layout_and_gitignore init_does_not_duplicate_gitignore_entries
```

Expected: fail because init still writes `.waxrc` and top-level `wax.lock.json`.

- [x] **Step 3: Update init paths and config template generation**

Modify `run_init` in `engine/crates/wax-cli/src/commands/init.rs`:

```rust
let wax_dir = options.repo_root.join(".wax");
let config_path = options.repo_root.join(wax_core::config::repo_files::PREFERRED_CONFIG_RELATIVE_PATH);
let lockfile_path = options.repo_root.join(wax_core::config::repo_files::PREFERRED_LOCKFILE_RELATIVE_PATH);
if config_path.exists() {
    return Err(InitCommandError::WaxConfigAlreadyExists { path: config_path });
}
```

Rename `WaxRcAlreadyExists` to:

```rust
/// Repository configuration already exists.
#[error("wax config already exists at {path}; remove it or run init in a fresh directory")]
WaxConfigAlreadyExists {
    /// Existing configuration path.
    path: PathBuf,
},
```

Change generated config to omit per-language registry keys by default. In `build_waxrc_contents`, after filtering languages:

```rust
for entry in &mut filtered {
    if let Some(object) = entry.as_object_mut() {
        object.remove("design_system_registry");
        object.remove("registry");
    }
}
```

Replace `scaffold_design_system_registries` with a simpler centralized scaffold
that writes only `DEFAULT_REGISTRY_RELATIVE_PATH` when `scaffold_registries` is
true. Do not keep the old per-language `design_system_registry` scaffold loop,
because new config omits registry keys by default.

Write:

```rust
fs::create_dir_all(&wax_dir)?;
write_file_atomically(&config_path, &waxrc_contents)?;
save_lockfile(&lockfile_path, &lockfile)?;
if options.scaffold_registries {
    write_file_atomically(
        &options.repo_root.join(wax_core::config::repo_files::DEFAULT_REGISTRY_RELATIVE_PATH),
        &format!("{EXAMPLE_DESIGN_SYSTEM_REGISTRY}\n"),
    )?;
}
update_gitignore(&options.repo_root)?;
```

Add helper:

```rust
fn update_gitignore(repo_root: &Path) -> Result<(), InitCommandError> {
    let path = repo_root.join(".gitignore");
    let mut contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(InitCommandError::Io {
                context: format!("read {}", path.display()),
                source,
            });
        }
    };

    for entry in ["/.wax/cache/", "/.wax/out/"] {
        if !contents.lines().any(|line| line.trim() == entry) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(entry);
            contents.push('\n');
        }
    }

    fs::write(&path, contents).map_err(|source| InitCommandError::Io {
        context: format!("write {}", path.display()),
        source,
    })
}
```

When creating the initial lockfile, insert registry locks:

```rust
let registry_source = wax_core::registry_source::resolve_registry_source(
    wax_core::registry_source::RegistrySourceInput {
        repo_root: &options.repo_root,
        language_id: resolved.manifest.id.as_str(),
        source: None,
    },
)?;
lockfile.registries.insert(
    resolved.manifest.id.clone(),
    wax_core::config::lockfile::LockedRegistry {
        source: registry_source.source,
        sha256: registry_source.sha256,
    },
);
```

Add `RegistrySource(#[from] wax_core::registry_source::RegistrySourceError)` to `InitCommandError`.

- [x] **Step 4: Update CLI help comments**

Modify `engine/crates/wax-cli/src/main.rs` doc comments that say `.waxrc` and top-level `wax.lock.json`:

```rust
/// Repository root that will receive `.wax/wax.config.json` and `.wax/wax.lock.json`.
```

and:

```rust
/// Repository root containing wax config and lock files.
```

- [x] **Step 5: Run init tests**

Run:

```bash
cd engine
cargo test -p wax-cli --test init_command
```

Expected: pass after updating existing expectations to `.wax/wax.config.json`, `.wax/wax.lock.json`, and `.wax/wax.registry.json`.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-cli/src/commands/init.rs engine/crates/wax-cli/tests/init_command.rs engine/crates/wax-cli/src/main.rs
git commit -m "feat: initialize centralized wax layout"
```

---

## Task 9: Language Commands Use Discovered Repo Files

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/language.rs`
- Test: existing tests in `engine/crates/wax-cli/src/commands/language.rs`

- [x] **Step 1: Add failing unit tests for discovered files and registry refresh**

Add tests in the existing `#[cfg(test)]` module in `engine/crates/wax-cli/src/commands/language.rs`:

```rust
#[test]
fn language_update_uses_centralized_lockfile_when_present() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".wax")).unwrap();
    fs::write(temp.path().join(".wax/wax.lock.json"), minimal_lockfile_json()).unwrap();
    fs::write(temp.path().join("wax.lock.json"), legacy_lockfile_json()).unwrap();

    let lockfile = load_optional_lockfile_for_repo(temp.path()).unwrap().unwrap();

    assert!(lockfile.languages.contains_key(&LanguageId::from_str("compose").unwrap()));
}

#[test]
fn language_doctor_reads_centralized_config() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".wax")).unwrap();
    fs::write(
        temp.path().join(".wax/wax.config.json"),
        r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true}]}"#,
    )
    .unwrap();
    fs::write(temp.path().join(".wax/wax.lock.json"), minimal_lockfile_json()).unwrap();

    let mut output = Vec::new();
    run_doctor(
        DoctorOptions {
            repo_root: temp.path().to_path_buf(),
            state_path: Some(temp.path().join("state.json")),
        },
        &mut output,
    )
    .unwrap();

    assert!(String::from_utf8(output).unwrap().contains("language: compose"));
}

#[test]
fn language_update_refreshes_registry_locks_for_enabled_languages() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".wax")).unwrap();
    fs::write(
        temp.path().join(".wax/wax.config.json"),
        r#"{"schema_version":1,"languages":[{"id":"compose","enabled":true}]}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".wax/wax.registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.button","symbol":"Button"}]}"#,
    )
    .unwrap();
    fs::write(temp.path().join(".wax/wax.lock.json"), minimal_v1_lockfile_json()).unwrap();

    refresh_registry_locks_for_repo(temp.path()).unwrap();

    let lock = wax_core::config::lockfile::load_lockfile(temp.path().join(".wax/wax.lock.json")).unwrap();
    let registry = lock.registries.get(&LanguageId::from_str("compose").unwrap()).unwrap();
    assert_eq!(registry.source, ".wax/wax.registry.json");
    assert_eq!(registry.sha256.len(), 64);
    assert_eq!(lock.schema_version, wax_core::config::lockfile::WAX_LOCK_SCHEMA_VERSION);
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-cli language_update_uses_centralized_lockfile_when_present language_doctor_reads_centralized_config
cargo test -p wax-cli language_update_refreshes_registry_locks_for_enabled_languages
```

Expected: fail because helpers still read legacy paths and no registry refresh helper exists.

- [x] **Step 3: Implement repo file discovery in language commands**

In `run_update`:

```rust
let repo_files = wax_core::config::repo_files::discover_repo_files(&options.repo_root);
let lockfile_path = repo_files.lockfile_path;
let mut lockfile = load_optional_lockfile(&lockfile_path)?;
```

In `run_doctor`:

```rust
let repo_files = wax_core::config::repo_files::discover_repo_files(&options.repo_root);
let waxrc = load_waxrc(&repo_files.config_path)?;
let lockfile = load_optional_lockfile(&repo_files.lockfile_path)?;
```

Add a private test helper:

```rust
fn load_optional_lockfile_for_repo(repo_root: &Path) -> Result<Option<WaxLock>, LanguageCommandError> {
    let repo_files = wax_core::config::repo_files::discover_repo_files(repo_root);
    load_optional_lockfile(&repo_files.lockfile_path)
}
```

- [x] **Step 4: Implement registry lock refresh**

Add to `LanguageCommandError`:

```rust
/// Registry source resolution failed.
#[error(transparent)]
RegistrySource(#[from] wax_core::registry_source::RegistrySourceError),
```

Add helper in `engine/crates/wax-cli/src/commands/language.rs`:

```rust
fn refresh_registry_locks_for_repo(repo_root: &Path) -> Result<(), LanguageCommandError> {
    let repo_files = wax_core::config::repo_files::discover_repo_files(repo_root);
    let waxrc = load_waxrc(&repo_files.config_path)?;
    let mut lockfile = load_lockfile(&repo_files.lockfile_path)?;

    for entry in waxrc.languages.iter().filter(|entry| entry.enabled) {
        let registry_setting = entry.registry_source();
        let resolved = wax_core::registry_source::resolve_registry_source_with_deprecation(
            wax_core::registry_source::RegistrySourceInput {
                repo_root,
                language_id: entry.id.as_str(),
                source: registry_setting.as_ref().map(|setting| setting.source.as_str()),
            },
            registry_setting.as_ref().is_some_and(|setting| setting.deprecated),
        )?;
        lockfile.registries.insert(
            entry.id.clone(),
            wax_core::config::lockfile::LockedRegistry {
                source: resolved.source,
                sha256: resolved.sha256,
            },
        );
    }

    lockfile.schema_version = wax_core::config::lockfile::WAX_LOCK_SCHEMA_VERSION;
    save_lockfile(&repo_files.lockfile_path, &lockfile)
}
```

Call this helper from `run_update` after language-pack lock entries are updated and before writing the final lockfile. The helper is intentionally reusable so a future explicit `wax registry update` command can share it.

- [x] **Step 5: Run language command tests**

Run:

```bash
cd engine
cargo test -p wax-cli language_
```

Expected: pass after updating existing fixtures with `registries`.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-cli/src/commands/language.rs
git commit -m "feat: use centralized wax files in language commands"
```

---

## Task 10: Contract Schema and Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Modify: `docs/specs/2026-05-13-component-tracker-design.md`
- Modify: `engine/crates/wax-contract/schemas/waxrc.schema.json`
- Modify: `engine/crates/wax-contract/schemas/wax-lock.schema.json` if present; otherwise add a note in docs that no public lockfile schema is published yet.
- Modify: `engine/fixtures/config/example.waxrc`

- [ ] **Step 1: Update `.waxrc` schema to centralized config shape**

Modify `engine/crates/wax-contract/schemas/waxrc.schema.json` so language entries accept optional `registry`:

```json
"registry": {
  "oneOf": [
    {
      "type": "string",
      "minLength": 1
    },
    {
      "type": "object",
      "additionalProperties": false,
      "required": ["source"],
      "properties": {
        "source": {
          "type": "string",
          "minLength": 1
        }
      }
    }
  ]
}
```

Keep `design_system_registry` in the schema as deprecated-compatible:

```json
"design_system_registry": {
  "type": "string",
  "minLength": 1
}
```

Update schema descriptions and docs to refer to `.wax/wax.config.json` as the
canonical config file while preserving compatibility with `.waxrc`.

- [ ] **Step 2: Update lockfile schema documentation**

If `engine/crates/wax-contract/schemas/wax-lock.schema.json` exists, add:

```json
"registries": {
  "type": "object",
  "additionalProperties": {
    "type": "object",
    "additionalProperties": false,
    "required": ["source", "sha256"],
    "properties": {
      "source": {
        "type": "string",
        "minLength": 1
      },
      "sha256": {
        "type": "string",
        "pattern": "^[a-fA-F0-9]{64}$"
      }
    }
  }
}
```

If there is no public lockfile schema file, add a short note to
`docs/specs/2026-05-16-language-packs-and-distribution.md` that lockfile schema
version 2 adds top-level `registries` entries and that schema publication is
tracked separately.

- [ ] **Step 3: Update fixture config**

Modify `engine/fixtures/config/example.waxrc` to omit registry fields by default:

```json
{
  "schema_version": 1,
  "engine": {
    "scan_concurrency": 2
  },
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "roots": ["app/src/main/kotlin"]
    },
    {
      "id": "react",
      "enabled": true,
      "roots": ["apps/web/src"]
    }
  ]
}
```

- [ ] **Step 4: Update README onboarding**

Replace the onboarding file list and registry path text with:

```markdown
`wax init` writes `.wax/wax.config.json`, `.wax/wax.lock.json`, and `.wax/wax.registry.json`.
Populate `.wax/wax.registry.json` with canonical components. Generated scan output lands in `.wax/out/`, which init adds to `.gitignore`.
```

Update the schema snippet to point at the canonical config:

```json
{
  "$schema": "https://raw.githubusercontent.com/Daio-io/wax/main/engine/crates/wax-contract/schemas/waxrc.schema.json"
}
```

Add hosted registry example:

```json
"registry": {
  "source": "https://example.com/acme-ds/registry/v2.4.1/compose.json"
}
```

- [ ] **Step 5: Update specs**

In `docs/specs/2026-05-16-language-packs-and-distribution.md`, replace `.waxrc`, top-level `wax.lock.json`, and `design-system/registry.json` default references with:

```text
.wax/wax.config.json
.wax/wax.lock.json
.wax/wax.registry.json
```

In `docs/specs/2026-05-13-component-tracker-design.md`, update the registry design principle from "it lives in the repo" to:

```markdown
- the default registry lives at `.wax/wax.registry.json`
- external registry sources are allowed through `.wax/wax.config.json` `registry`
- validation and scan operate on lockfile-pinned registry content
```

- [ ] **Step 6: Run docs/schema adjacent checks**

Run:

```bash
cd engine
cargo test -p wax-core --test waxrc_load
cargo test -p wax-cli --test init_command
```

Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add README.md docs/specs/2026-05-16-language-packs-and-distribution.md docs/specs/2026-05-13-component-tracker-design.md engine/crates/wax-contract/schemas/waxrc.schema.json engine/fixtures/config/example.waxrc
git commit -m "docs: document centralized registry configuration"
```

---

## Task 11: Full Verification and Plan Checkboxes

**Files:**
- Modify active plan docs if this work is inserted as a task in `docs/plans/README.md` or a new product plan.
- Modify `docs/plans/2026-06-02-registry-sources-and-wax-layout.md` task checkboxes as work completes.

- [ ] **Step 1: Run formatting**

Run:

```bash
cd engine
cargo fmt --all
```

Expected: command exits 0.

- [ ] **Step 2: Run engine checks**

Run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all commands exit 0.

- [ ] **Step 3: Run release/install adjacent checks**

Run:

```bash
scripts/test-generate-pack-index.sh
scripts/install.sh --help
```

Expected: both commands exit 0.

- [ ] **Step 4: Update plan checkboxes**

Mark completed implementation tasks in this file using checked boxes:

```markdown
- [x] **Step 1: Run formatting**
```

If this feature is added to an official plan under `docs/plans/`, update that plan's task checkbox and every completed step checkbox in the same commit.

- [ ] **Step 5: Commit verification/doc checkbox updates**

```bash
git add docs/plans/2026-06-02-registry-sources-and-wax-layout.md docs/plans
git commit -m "chore: mark registry source plan progress"
```

If `docs/plans` has no related changes, commit only this implementation plan's checkbox updates.
