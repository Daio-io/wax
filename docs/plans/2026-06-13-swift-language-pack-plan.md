# SwiftUI Language Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `wax-lang-swift` as a production SwiftUI language pack with Compose-parity scan facts, generic registry discovery support, release artifacts, pack-index publication, and user documentation.

**Architecture:** `wax-lang-swift` is a new Rust workspace crate and stdio binary. It uses `tree-sitter-swift` to parse `.swift` files, loads the existing Wax registry format, extracts local SwiftUI components and registry-backed usage calls, and implements the generic `discover` wire protocol. `wax-core` and `wax-cli` stay pack-agnostic; release and install surfaces only learn the new binary name.

**Tech Stack:** Rust 2024, `tree-sitter`, `tree-sitter-swift`, `serde_json`, `time`, `wax-contract`, `wax-lang-api`, existing subprocess scan/discover protocol.

---

## Reference Docs

- Design spec: `docs/plans/2026-06-12-swift-language-pack-design.md`
- Roadmap source: `docs/plans/README.md`
- Language-pack contract: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Generic discover ADR: `docs/adr/2026-06-10-generic-registry-discovery-protocol.md`
- Compose implementation reference: `engine/crates/wax-lang-compose/`
- React discover reference: `engine/crates/wax-lang-react/src/discover.rs`

## Execution Model

Each task below should be one focused PR unless maintainers explicitly batch adjacent docs-only or release-surface work. Keep plan checkboxes current in the same PR that completes the work.

Before implementation starts, create a feature branch from current `main`:

```bash
git switch main
git pull --ff-only origin main
git switch -c dai/swift-language-pack
```

## File Structure

- Create `engine/crates/wax-lang-swift/Cargo.toml`
  - Declares the new language-pack crate, binary, dependencies, and dev dependencies.
- Create `engine/crates/wax-lang-swift/build.rs`
  - Reads the bundled `tree-sitter-swift` dependency version for parser metadata when practical. If the crate does not expose enough metadata, define a constant beside the parser wrapper and cover it in tests.
- Create `engine/crates/wax-lang-swift/src/lib.rs`
  - Public `SwiftLanguage`, scan/discover errors, scaffold/configured scan routing, contract validation.
- Create `engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs`
  - Stdio request loop for `scan` and `discover`.
- Create `engine/crates/wax-lang-swift/src/swift_ast.rs`
  - Parser initialization, `.swift` file collection, strict/permissive parse helpers, AST utilities.
- Create `engine/crates/wax-lang-swift/src/component_detect.rs`
  - Shared SwiftUI predicates used by scan and discover.
- Create `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
  - Scan config, registry loading, root resolution, extraction, diagnostics, and facts-ready scan result.
- Create `engine/crates/wax-lang-swift/src/discover.rs`
  - Registry symbol discovery from design-system roots.
- Create `engine/crates/wax-lang-swift/tests/fixtures/small/...`
  - End-to-end SwiftUI scan fixture and golden counts.
- Create `engine/crates/wax-lang-swift/tests/fixtures/discover/...`
  - Registry discovery fixture.
- Create `engine/crates/wax-lang-swift/tests/golden_small.rs`
  - End-to-end scan assertions.
- Create `engine/crates/wax-lang-swift/tests/registry_discover.rs`
  - Discovery assertions.
- Create `engine/crates/wax-lang-swift/tests/config_validation.rs`
  - Config and registry validation through the public scanner.
- Create `engine/crates/wax-lang-swift/tests/stdio_cli.rs`
  - Stdio scan/discover and typed error assertions.
- Modify `engine/Cargo.toml`
  - Add the workspace member after `wax-lang-react`; add release metadata only in the release task.
- Modify release/install/docs surfaces in the final promotion tasks:
  - `.github/workflows/release.yml`
  - `scripts/build-release.sh`
  - `scripts/generate-pack-index.sh`
  - `scripts/test-generate-pack-index.sh`
  - `scripts/check-release-workflow.rb`
  - `engine/crates/wax-core/src/registry.rs`
  - `engine/fixtures/registry/*.json`
  - `engine/fixtures/config/example.waxrc`
  - `README.md`
  - `packages/cli/package.json`

## Phase 1 - Crate Scaffold and Stdio Shell

### Task 1: Add the `wax-lang-swift` crate scaffold

**Files:**
- Modify: `engine/Cargo.toml`
- Create: `engine/crates/wax-lang-swift/Cargo.toml`
- Create: `engine/crates/wax-lang-swift/build.rs`
- Create: `engine/crates/wax-lang-swift/src/lib.rs`
- Create: `engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs`
- Create: `engine/crates/wax-lang-swift/tests/stdio_cli.rs`

- [x] **Step 1: Write the failing stdio scaffold test**

Create `engine/crates/wax-lang-swift/tests/stdio_cli.rs`:

```rust
use std::io::Write;
use std::process::{Command, Stdio};
use wax_lang_api::{WIRE_API_VERSION, WirePackResponse};

#[test]
fn stdio_scan_with_empty_config_returns_swift_scaffold_facts() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    let request = format!(
        "{{\"type\":\"scan\",\"api_version\":{WIRE_API_VERSION},\"language_id\":\"swift\",\"repo_root\":\"/tmp/repo\",\"snapshot_id\":\"snap-swift-scaffold\",\"config\":{{}}}}\n"
    );
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write request");

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "wax-lang-swift exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let response: WirePackResponse =
        serde_json::from_slice(&output.stdout).expect("parse response");
    match response {
        WirePackResponse::ScanFacts {
            api_version,
            language_id,
            facts,
        } => {
            assert_eq!(api_version, WIRE_API_VERSION);
            assert_eq!(language_id.as_str(), "swift");
            assert_eq!(facts.language.id.as_str(), "swift");
            assert_eq!(facts.language.ecosystem, "swiftui");
            assert_eq!(facts.language.parser_name, "tree-sitter-swift");
            assert_eq!(facts.snapshot_id, "snap-swift-scaffold");
            assert_eq!(facts.counts.usage_site_count, 0);
            assert!(
                facts
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "swift_scaffold")
            );
        }
        other => panic!("expected scan facts, got {other:?}"),
    }
}
```

- [x] **Step 2: Run the test to verify it fails**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test stdio_cli stdio_scan_with_empty_config_returns_swift_scaffold_facts
```

Expected: FAIL because `wax-lang-swift` is not a workspace package.

- [x] **Step 3: Add the crate to the workspace**

Modify `engine/Cargo.toml`:

```toml
members = [
    "crates/wax-contract",
    "crates/wax-core",
    "crates/wax-cli",
    "crates/wax-lang-api",
    "crates/wax-lang-basic",
    "crates/wax-lang-compose",
    "crates/wax-lang-react",
    "crates/wax-lang-swift",
]
```

- [x] **Step 4: Create the package manifest**

Create `engine/crates/wax-lang-swift/Cargo.toml`:

```toml
[package]
name = "wax-lang-swift"
version.workspace = true
edition.workspace = true
description = "SwiftUI language pack for the wax engine"
build = "build.rs"

[[bin]]
name = "wax-lang-swift"
path = "src/bin/wax-lang-swift.rs"

[dependencies]
clap = { version = "4.5", features = ["derive"] }
serde_json = "1"
time = { version = "0.3", features = ["serde", "serde-well-known"] }
tree-sitter = "0.22"
tree-sitter-swift = "0.7.3"
wax-contract = { path = "../wax-contract" }
wax-lang-api = { path = "../wax-lang-api" }

[build-dependencies]
toml = "0.9"

[dev-dependencies]
tempfile = "3"
```

This version is the current crates.io release verified during plan authoring with `cargo search tree-sitter-swift --limit 5`.

- [x] **Step 5: Create parser version build metadata**

Create `engine/crates/wax-lang-swift/build.rs`:

```rust
fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
}
```

- [x] **Step 6: Create scaffold facts in the library**

Create `engine/crates/wax-lang-swift/src/lib.rs`:

```rust
//! SwiftUI language pack implementation.

#![deny(missing_docs)]

use time::OffsetDateTime;
use wax_contract::{
    CountSummary, Diagnostic, DiagnosticSeverity, LanguageId, LanguageMetadata, Metrics,
    SCHEMA_VERSION, ScanFacts, ScanFactsError, ScanStatus,
};
use wax_lang_api::{DiscoverRequest, ScanRequest, build_version};

/// Parser version bundled through the `tree-sitter-swift` dependency.
pub const TREE_SITTER_SWIFT_GRAMMAR_VERSION: &str = "0.7.3";

/// Result of a Swift registry symbol discovery request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverSymbolsResult {
    /// Discovered design-system symbol names.
    pub symbols: Vec<String>,
    /// Non-fatal diagnostics emitted during discovery.
    pub diagnostics: Vec<Diagnostic>,
}

/// Errors returned by [`SwiftLanguage::scan`].
#[derive(Debug)]
pub enum SwiftScanError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
    /// Failed to produce contract-valid facts.
    InvalidFacts(ScanFactsError),
}

impl std::fmt::Display for SwiftScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
            Self::InvalidFacts(err) => write!(f, "swift facts validation failed: {err}"),
        }
    }
}

impl std::error::Error for SwiftScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLanguageId(_) => None,
            Self::InvalidFacts(err) => Some(err),
        }
    }
}

/// Errors returned by [`SwiftLanguage::discover`].
#[derive(Debug)]
pub enum SwiftDiscoverError {
    /// The request contains an invalid language id.
    InvalidLanguageId(String),
}

impl std::fmt::Display for SwiftDiscoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLanguageId(id) => write!(f, "invalid swift language id: {id}"),
        }
    }
}

impl std::error::Error for SwiftDiscoverError {}

/// SwiftUI language extractor.
#[derive(Debug, Default)]
pub struct SwiftLanguage;

impl SwiftLanguage {
    /// Creates a SwiftUI language extractor.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Executes a Swift scan for the provided request.
    pub fn scan(&self, request: &ScanRequest) -> Result<ScanFacts, SwiftScanError> {
        let swift_language_id =
            LanguageId::try_from("swift").expect("hardcoded swift id must be valid");

        if request.language_id != swift_language_id {
            return Err(SwiftScanError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let mut facts = scaffold_facts(request, swift_language_id);
        facts.recompute_counts().map_err(SwiftScanError::InvalidFacts)?;
        facts.validate().map_err(SwiftScanError::InvalidFacts)?;
        Ok(facts)
    }

    /// Discovers likely public SwiftUI design-system component symbols.
    pub fn discover(
        &self,
        request: &DiscoverRequest,
    ) -> Result<DiscoverSymbolsResult, SwiftDiscoverError> {
        let swift_language_id =
            LanguageId::try_from("swift").expect("hardcoded swift id must be valid");

        if request.language_id != swift_language_id {
            return Err(SwiftDiscoverError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        Ok(DiscoverSymbolsResult {
            symbols: Vec::new(),
            diagnostics: Vec::new(),
        })
    }
}

fn scaffold_facts(request: &ScanRequest, language_id: LanguageId) -> ScanFacts {
    ScanFacts {
        schema_version: SCHEMA_VERSION,
        language: LanguageMetadata {
            id: language_id,
            version: build_version().to_owned(),
            ecosystem: "swiftui".to_owned(),
            parser_name: "tree-sitter-swift".to_owned(),
            parser_version: TREE_SITTER_SWIFT_GRAMMAR_VERSION.to_owned(),
        },
        snapshot_id: request.snapshot_id.clone(),
        scanned_at: OffsetDateTime::now_utc(),
        status: ScanStatus::Partial,
        design_system_components: Vec::new(),
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "swift_scaffold".to_owned(),
            message: "SwiftUI extraction is scaffolded; configure registry and roots to scan."
                .to_owned(),
            location: None,
        }],
        metrics: Metrics {
            adoption_coverage_ratio: None,
            parse_extract_ms: 0,
            files_scanned: 0,
        },
        counts: CountSummary {
            design_system_component_count: 0,
            local_component_count: 0,
            usage_site_count: 0,
            resolved_count: 0,
            candidate_count: 0,
        },
    }
}
```

- [x] **Step 7: Create the stdio binary**

Create `engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs`:

```rust
use clap::Parser;
use std::io::{self, BufRead, Write};
use wax_contract::LanguageId;
use wax_lang_api::{
    DiscoverRequest, DiscoverRequestType, ScanRequestType, WIRE_API_VERSION, WireErrorCode,
    WirePackRequest, WirePackResponse,
};
use wax_lang_swift::{SwiftDiscoverError, SwiftLanguage, SwiftScanError};

#[derive(Debug, Parser)]
#[command(name = "wax-lang-swift")]
struct Cli {
    /// Run language pack in stdio mode.
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if !cli.stdio {
        eprintln!("usage: wax-lang-swift --stdio");
        std::process::exit(2);
    }

    run_stdio()
}

fn run_stdio() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    run_stdio_with_reader(stdin.lock(), &mut stdout)
}

fn run_stdio_with_reader<R: BufRead, W: Write>(
    reader: R,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    for line_result in reader.lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let request: WirePackRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let response = WirePackResponse::Error {
                    api_version: WIRE_API_VERSION,
                    language_id: swift_language_id(),
                    code: WireErrorCode::ConfigInvalid,
                    message: format!("invalid pack request JSON: {err}"),
                    diagnostics: Vec::new(),
                };
                serde_json::to_writer(&mut *writer, &response)?;
                writer.write_all(b"\n")?;
                writer.flush()?;
                return Ok(());
            }
        };

        let response = match request {
            WirePackRequest::Scan {
                api_version,
                language_id,
                repo_root,
                snapshot_id,
                config,
            } => {
                if api_version != WIRE_API_VERSION {
                    WirePackResponse::Error {
                        api_version: WIRE_API_VERSION,
                        language_id,
                        code: WireErrorCode::ApiVersionUnsupported,
                        message: format!(
                            "wire api_version {api_version} is unsupported; expected {WIRE_API_VERSION}"
                        ),
                        diagnostics: Vec::new(),
                    }
                } else {
                    let scan_request = wax_lang_api::ScanRequest {
                        request_type: ScanRequestType::Scan,
                        api_version,
                        language_id: language_id.clone(),
                        repo_root,
                        snapshot_id,
                        config,
                    };
                    let swift = SwiftLanguage::new();
                    match swift.scan(&scan_request) {
                        Ok(facts) => WirePackResponse::ScanFacts {
                            api_version,
                            language_id,
                            facts: Box::new(facts),
                        },
                        Err(err) => scan_error_response(api_version, language_id, err),
                    }
                }
            }
            WirePackRequest::Discover {
                api_version,
                language_id,
                repo_root,
                roots,
            } => {
                if api_version != WIRE_API_VERSION {
                    WirePackResponse::Error {
                        api_version: WIRE_API_VERSION,
                        language_id,
                        code: WireErrorCode::ApiVersionUnsupported,
                        message: format!(
                            "wire api_version {api_version} is unsupported; expected {WIRE_API_VERSION}"
                        ),
                        diagnostics: Vec::new(),
                    }
                } else {
                    let discover_request = DiscoverRequest {
                        request_type: DiscoverRequestType::Discover,
                        api_version,
                        language_id: language_id.clone(),
                        repo_root,
                        roots,
                    };
                    let swift = SwiftLanguage::new();
                    match swift.discover(&discover_request) {
                        Ok(result) => WirePackResponse::DiscoverSymbols {
                            api_version,
                            language_id,
                            symbols: result.symbols,
                            diagnostics: result.diagnostics,
                        },
                        Err(err) => discover_error_response(api_version, language_id, err),
                    }
                }
            }
        };

        serde_json::to_writer(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        return Ok(());
    }

    Ok(())
}

fn scan_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: SwiftScanError,
) -> WirePackResponse {
    let code = match &err {
        SwiftScanError::InvalidLanguageId(_) | SwiftScanError::InvalidFacts(_) => {
            WireErrorCode::ScanFailed
        }
    };
    WirePackResponse::Error {
        api_version,
        language_id,
        code,
        message: err.to_string(),
        diagnostics: Vec::new(),
    }
}

fn discover_error_response(
    api_version: u32,
    language_id: LanguageId,
    err: SwiftDiscoverError,
) -> WirePackResponse {
    let code = match &err {
        SwiftDiscoverError::InvalidLanguageId(_) => WireErrorCode::ConfigInvalid,
    };
    WirePackResponse::Error {
        api_version,
        language_id,
        code,
        message: err.to_string(),
        diagnostics: Vec::new(),
    }
}

fn swift_language_id() -> LanguageId {
    LanguageId::try_from("swift").expect("hardcoded swift id must be valid")
}
```

- [x] **Step 8: Run scaffold tests**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test stdio_cli
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: PASS.

- [x] **Step 9: Commit**

```bash
git add engine/Cargo.toml engine/crates/wax-lang-swift
git commit -m "feat: scaffold SwiftUI language pack"
```

## Phase 2 - Config, Registry, Files, and Parser

### Task 2: Add Swift scan config parsing and registry loading

**Files:**
- Modify: `engine/crates/wax-lang-swift/src/lib.rs`
- Create: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
- Create: `engine/crates/wax-lang-swift/tests/config_validation.rs`

- [x] **Step 1: Write failing config tests**

Create `engine/crates/wax-lang-swift/tests/config_validation.rs` with Compose-parity coverage. Start with these tests:

```rust
use std::fs;
use wax_contract::ScanStatus;
use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType};
use wax_lang_swift::SwiftLanguage;

fn request(repo_root: &std::path::Path, config: ScanConfig) -> ScanRequest {
    ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: 1,
        language_id: "swift".try_into().unwrap(),
        repo_root: repo_root.to_string_lossy().to_string(),
        snapshot_id: "snap-swift-config".to_owned(),
        config,
    }
}

#[test]
fn configured_scan_requires_registry_and_roots() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = ScanConfig::new();
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));

    let err = SwiftLanguage::new()
        .scan(&request(tempdir.path(), config))
        .expect_err("missing registry should fail");

    assert!(err.to_string().contains("registry is required"));
}

#[test]
fn configured_scan_rejects_parent_directory_registry() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = ScanConfig::new();
    config.insert("registry".to_owned(), serde_json::json!("../registry.json"));
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));

    let err = SwiftLanguage::new()
        .scan(&request(tempdir.path(), config))
        .expect_err("parent registry should fail");

    assert!(err.to_string().contains("parent directory"));
}

#[test]
fn configured_scan_loads_registry_and_reports_missing_root_as_partial() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(tempdir.path().join(".wax")).unwrap();
    fs::write(
        tempdir.path().join(".wax/swift.registry.json"),
        r#"{"schema_version":1,"components":[{"id":"ds.primary-button","symbol":"PrimaryButton","targets":["swift"]}]}"#,
    )
    .unwrap();

    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::json!(".wax/swift.registry.json"),
    );
    config.insert("roots".to_owned(), serde_json::json!(["Sources"]));

    let facts = SwiftLanguage::new()
        .scan(&request(tempdir.path(), config))
        .expect("scan should return partial facts");

    assert_eq!(facts.status, ScanStatus::Partial);
    assert_eq!(facts.design_system_components.len(), 1);
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "root_not_found")
    );
}
```

Before this task is complete, expand the same file with these additional tests using the Compose `config_validation.rs` structure as the behavioral template:

- `registry_key_is_accepted_as_canonical_registry_path`: configure `registry`, scan the small fixture, and assert a complete scan with Swift design-system components.
- `design_system_registry_key_still_scans`: configure legacy `design_system_registry`, scan the small fixture, and assert the same counts as the canonical key.
- `registry_key_wins_when_both_registry_keys_are_present`: configure `registry` to the main fixture and `design_system_registry` to `alt-design-system/registry.json`; assert the main registry symbols are used.
- `empty_roots_array_is_config_error_not_scaffold`: configure `roots: []` and assert the error text contains `roots must be a non-empty array of strings`.
- `non_string_root_entry_is_config_error`: configure `roots: [42]` and assert the error text contains `roots[0] must be a non-empty string`.
- `roots_without_registry_is_config_error`: configure only `roots` and assert the error text contains `registry is required`.
- `absolute_registry_path_is_config_error`: configure an absolute `registry` path and assert the error text contains `repo-relative path`.
- `configured_scan_reports_parse_failed_for_invalid_source`: scan one valid and one broken Swift file, assert `ScanStatus::Partial`, and assert a `parse_failed` diagnostic.

Add `engine/crates/wax-lang-swift/tests/fixtures/small/alt-design-system/registry.json` for `registry_key_wins_when_both_registry_keys_are_present`.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test config_validation
```

Expected: FAIL because configured scan parsing is not implemented.

- [x] **Step 3: Define config and registry structures**

Create the top of `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`:

```rust
//! Tree-sitter-swift backed SwiftUI scanner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use wax_contract::{
    DesignSystemComponent, Diagnostic, DiagnosticSeverity, LocalComponent, MatchStatus, ScanStatus,
    SourceLocation, UsageSite,
};
use wax_lang_api::{RootPatternKind, RootResolutionError, ScanConfig, resolve_source_roots};

/// Parsed Swift scan configuration from the engine request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwiftScanConfig {
    /// Repo-relative path to the design-system registry JSON file.
    pub design_system_registry: PathBuf,
    /// Repo-relative Swift source roots to scan.
    pub roots: Vec<PathBuf>,
}

/// Whether the request should run the tree-sitter scanner or return scaffold facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwiftConfigMode {
    /// No Swift scan keys were provided.
    Scaffold,
    /// Registry and roots were provided and validated.
    Configured(SwiftScanConfig),
}

/// Errors produced by the tree-sitter Swift scanner.
#[derive(Debug)]
pub enum TreeSitterScanError {
    /// Scan config payload was present but invalid.
    ConfigInvalid {
        /// Human-readable validation failure.
        reason: String,
    },
    /// Registry JSON could not be read or parsed.
    RegistryInvalid {
        /// Registry path that failed.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },
    /// Tree-sitter parser failed to initialize.
    ParserInitFailed {
        /// Human-readable reason.
        reason: String,
    },
    /// A filesystem operation failed.
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}
```

- [x] **Step 4: Implement scan error display traits**

Add below `TreeSitterScanError` in `tree_sitter_scan.rs`:

```rust
impl std::fmt::Display for TreeSitterScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigInvalid { reason } => write!(f, "invalid swift scan config: {reason}"),
            Self::RegistryInvalid { path, reason } => {
                write!(
                    f,
                    "invalid design-system registry at {}: {reason}",
                    path.display()
                )
            }
            Self::ParserInitFailed { reason } => {
                write!(f, "tree-sitter parser init failed: {reason}")
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for TreeSitterScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConfigInvalid { .. }
            | Self::RegistryInvalid { .. }
            | Self::ParserInitFailed { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}
```

- [x] **Step 5: Implement config parsing**

Add to `tree_sitter_scan.rs`:

```rust
/// Loads Swift scan settings from the engine request payload.
pub fn parse_swift_scan_config(
    config: &ScanConfig,
) -> Result<SwiftConfigMode, TreeSitterScanError> {
    let has_registry =
        config.contains_key("registry") || config.contains_key("design_system_registry");
    let has_roots = config.contains_key("roots");
    if !has_registry && !has_roots {
        return Ok(SwiftConfigMode::Scaffold);
    }

    let registry = config
        .get("registry")
        .or_else(|| config.get("design_system_registry"))
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "registry is required when swift scan config is present".to_owned(),
        })?;
    let registry = registry
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "registry must be a non-empty string".to_owned(),
        })?;
    validate_repo_relative_path(registry, "registry")?;

    let roots_value = config
        .get("roots")
        .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
            reason: "roots is required when swift scan config is present".to_owned(),
        })?;
    let roots_array =
        roots_value
            .as_array()
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: "roots must be a non-empty array of strings".to_owned(),
            })?;
    if roots_array.is_empty() {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: "roots must be a non-empty array of strings".to_owned(),
        });
    }

    let mut roots = Vec::with_capacity(roots_array.len());
    for (index, entry) in roots_array.iter().enumerate() {
        let root = entry
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| TreeSitterScanError::ConfigInvalid {
                reason: format!("roots[{index}] must be a non-empty string"),
            })?;
        validate_repo_relative_path(root, &format!("roots[{index}]"))?;
        roots.push(PathBuf::from(root));
    }

    Ok(SwiftConfigMode::Configured(SwiftScanConfig {
        design_system_registry: PathBuf::from(registry),
        roots,
    }))
}

fn validate_repo_relative_path(path: &str, field: &str) -> Result<(), TreeSitterScanError> {
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: format!("{field} must be a repo-relative path"),
        });
    }
    if parsed
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(TreeSitterScanError::ConfigInvalid {
            reason: format!("{field} must not contain parent directory segments"),
        });
    }
    Ok(())
}
```

- [x] **Step 6: Implement registry loading with aliases and targets**

Add to `tree_sitter_scan.rs`:

```rust
struct RegistryIndex {
    canonical_symbols: Vec<String>,
    resolve_targets: BTreeMap<String, String>,
}

fn load_registry(path: &Path) -> Result<RegistryIndex, TreeSitterScanError> {
    let raw = fs::read_to_string(path).map_err(|source| TreeSitterScanError::Io {
        context: format!("read design-system registry {}", path.display()),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("registry JSON is invalid: {err}"),
        })?;
    let components = value
        .get("components")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry JSON must contain a components array".to_owned(),
        })?;

    let mut canonical_symbols = Vec::new();
    let mut resolve_targets = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if !component_available_to_swift(component, index, path)? {
            continue;
        }
        let symbol = component
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                path: path.to_path_buf(),
                reason: format!("components[{index}] is missing symbol"),
            })?;
        canonical_symbols.push(symbol.to_owned());
        resolve_targets.insert(symbol.to_owned(), symbol.to_owned());
        if let Some(aliases) = component
            .get("aliases")
            .and_then(serde_json::Value::as_array)
        {
            for (alias_index, alias) in aliases.iter().enumerate() {
                let alias_symbol =
                    alias
                        .as_str()
                        .ok_or_else(|| TreeSitterScanError::RegistryInvalid {
                            path: path.to_path_buf(),
                            reason: format!(
                                "components[{index}].aliases[{alias_index}] must be a string"
                            ),
                        })?;
                resolve_targets.insert(alias_symbol.to_owned(), symbol.to_owned());
            }
        }
    }

    if canonical_symbols.is_empty() {
        return Err(TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: "registry must declare at least one Swift component symbol".to_owned(),
        });
    }

    canonical_symbols.sort();
    Ok(RegistryIndex {
        canonical_symbols,
        resolve_targets,
    })
}

fn component_available_to_swift(
    component: &serde_json::Value,
    index: usize,
    path: &Path,
) -> Result<bool, TreeSitterScanError> {
    let Some(targets_value) = component.get("targets") else {
        return Ok(true);
    };
    if targets_value.is_null() {
        return Ok(true);
    }
    let Some(targets) = targets_value.as_array() else {
        return Err(TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("components[{index}].targets must be an array of strings"),
        });
    };
    for (target_index, target) in targets.iter().enumerate() {
        let target = target.as_str().ok_or_else(|| TreeSitterScanError::RegistryInvalid {
            path: path.to_path_buf(),
            reason: format!("components[{index}].targets[{target_index}] must be a string"),
        })?;
        if target == "swift" {
            return Ok(true);
        }
    }
    Ok(false)
}
```

If Compose still lacks `targets` filtering when this task runs, add a focused pre-implementation check:

```bash
cd engine
rg -n "targets|component_available_to" crates/wax-lang-compose/src crates/wax-lang-react/src
```

Then either add Compose target filtering in a prerequisite PR or document in the task PR why Swift intentionally leads on target filtering. Do not leave Swift, React, and Compose silently inconsistent.

- [x] **Step 7: Add a minimal configured scan result**

Add to `tree_sitter_scan.rs`:

```rust
/// Output of the tree-sitter scanner before contract validation.
#[derive(Debug)]
pub struct TreeSitterScanResult {
    /// Known design-system components from the registry file.
    pub design_system_components: Vec<DesignSystemComponent>,
    /// Local SwiftUI declarations discovered in Swift sources.
    pub local_components: Vec<LocalComponent>,
    /// Usage sites matched against the registry.
    pub usage_sites: Vec<UsageSite>,
    /// Number of Swift files scanned.
    pub files_scanned: u32,
    /// Diagnostics emitted during the scan.
    pub diagnostics: Vec<Diagnostic>,
    /// Overall scan status.
    pub status: ScanStatus,
}

/// Runs the tree-sitter Swift scanner for a configured repository layout.
pub fn scan_repository(
    repo_root: &Path,
    config: &SwiftScanConfig,
) -> Result<TreeSitterScanResult, TreeSitterScanError> {
    let registry_path = repo_root.join(&config.design_system_registry);
    let registry = load_registry(&registry_path)?;

    let mut diagnostics = Vec::new();
    for root in &config.roots {
        let resolved = resolve_source_roots(repo_root, root).map_err(map_root_resolution_error)?;
        if resolved.roots.is_empty() {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: root_not_found_code(resolved.kind),
                message: root_not_found_message(root, resolved.kind),
                location: None,
            });
        }
    }

    let mut design_system_components = registry
        .canonical_symbols
        .iter()
        .map(|symbol| DesignSystemComponent {
            id: format!("ds.{symbol}"),
            symbol: symbol.clone(),
            registry_symbol: symbol.clone(),
        })
        .collect::<Vec<_>>();
    design_system_components.sort_by(|left, right| left.symbol.cmp(&right.symbol));

    let has_gaps = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "root_not_found" || diagnostic.code == "root_glob_not_found");

    Ok(TreeSitterScanResult {
        design_system_components,
        local_components: Vec::new(),
        usage_sites: Vec::new(),
        files_scanned: 0,
        diagnostics,
        status: if has_gaps {
            ScanStatus::Partial
        } else {
            ScanStatus::Complete
        },
    })
}
```

Also add `map_root_resolution_error`, `root_not_found_code`, and `root_not_found_message` copied in behavior from Compose but with Swift wording.

- [x] **Step 8: Route configured scans from `lib.rs`**

Modify `engine/crates/wax-lang-swift/src/lib.rs` to declare `mod tree_sitter_scan;`, export `SwiftConfigMode` and `SwiftScanConfig`, add `InvalidConfig`, `ParserInitFailed`, and `Scanner` variants to `SwiftScanError`, and branch in `SwiftLanguage::scan`:

```rust
let mut facts = match tree_sitter_scan::parse_swift_scan_config(&request.config)
    .map_err(map_scan_error)?
{
    SwiftConfigMode::Scaffold => scaffold_facts(request, swift_language_id),
    SwiftConfigMode::Configured(scan_config) => {
        let repo_root = Path::new(&request.repo_root);
        let result = tree_sitter_scan::scan_repository(repo_root, &scan_config)
            .map_err(map_scan_error)?;
        facts_from_scan(request, result)
    }
};
```

Add `facts_from_scan` mirroring Compose and using:

```rust
ecosystem: "swiftui".to_owned(),
parser_name: "tree-sitter-swift".to_owned(),
parser_version: TREE_SITTER_SWIFT_GRAMMAR_VERSION.to_owned(),
```

- [x] **Step 9: Run focused tests**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test config_validation
cargo test -p wax-lang-swift
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: PASS.

- [x] **Step 10: Commit**

```bash
git add engine/crates/wax-lang-swift
git commit -m "feat: parse Swift language config and registry"
```

### Task 3: Add Swift file collection and parser wrapper

**Files:**
- Create: `engine/crates/wax-lang-swift/src/swift_ast.rs`
- Modify: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
- Add tests inside: `engine/crates/wax-lang-swift/src/swift_ast.rs`

- [x] **Step 1: Write parser and file collection tests**

Add this test module to `swift_ast.rs` before implementation or create the file with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn collect_swift_files_recurses_and_skips_non_swift_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tempdir.path().join("Sources/App")).unwrap();
        fs::write(tempdir.path().join("Sources/App/View.swift"), "struct View {}").unwrap();
        fs::write(tempdir.path().join("Sources/App/View.txt"), "not swift").unwrap();

        let mut files = Vec::new();
        collect_swift_files(&tempdir.path().join("Sources"), &mut files).unwrap();
        files.sort();

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("View.swift"));
    }

    #[test]
    fn parse_swift_file_returns_tree_for_valid_source() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tempdir.path().join("Sources")).unwrap();
        fs::write(
            tempdir.path().join("Sources/Card.swift"),
            "import SwiftUI\nstruct Card: View { var body: some View { Text(\"Card\") } }\n",
        )
        .unwrap();

        let mut parser = new_parser().expect("parser");
        let parsed =
            parse_swift_file_permissive(&mut parser, &tempdir.path().join("Sources/Card.swift"))
                .expect("parse");

        assert_eq!(parsed.source.contains("struct Card"), true);
        assert!(!parsed.tree.root_node().has_error());
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-lang-swift swift_ast
```

Expected: FAIL because `swift_ast` helpers are not implemented.

- [x] **Step 3: Implement parser and file helpers**

Create `engine/crates/wax-lang-swift/src/swift_ast.rs`:

```rust
//! Swift tree-sitter parsing helpers.

use std::fs;
use std::path::{Path, PathBuf};

/// Parsed Swift file.
#[derive(Debug)]
pub(crate) struct ParsedSwiftFile {
    /// Parsed tree-sitter syntax tree.
    pub(crate) tree: tree_sitter::Tree,
    /// Source text used to parse the file.
    pub(crate) source: String,
}

/// Errors produced while parsing a Swift source file.
#[derive(Debug)]
pub(crate) enum ParseSwiftFileError {
    /// A filesystem operation failed.
    Io {
        /// Human-readable context.
        context: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Tree-sitter returned a tree containing syntax errors.
    ParseFailed(PathBuf),
}

/// Creates a tree-sitter parser configured for Swift.
pub(crate) fn new_parser() -> Result<tree_sitter::Parser, String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_swift::language())
        .map_err(|err| err.to_string())?;
    Ok(parser)
}

/// Recursively collects Swift source files under `dir`.
pub(crate) fn collect_swift_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)?.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_swift_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("swift") {
            files.push(path);
        }
    }
    Ok(())
}

/// Parses a Swift file and allows partial trees when tree-sitter can recover.
pub(crate) fn parse_swift_file_permissive(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedSwiftFile, ParseSwiftFileError> {
    let source = fs::read_to_string(path).map_err(|source| ParseSwiftFileError::Io {
        context: format!("read Swift source {}", path.display()),
        source,
    })?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| ParseSwiftFileError::ParseFailed(path.to_path_buf()))?;
    if tree.root_node().has_error() {
        return Err(ParseSwiftFileError::ParseFailed(path.to_path_buf()));
    }
    Ok(ParsedSwiftFile { tree, source })
}

/// Parses a Swift file and fails on any syntax error.
pub(crate) fn parse_swift_file_strict(
    parser: &mut tree_sitter::Parser,
    path: &Path,
) -> Result<ParsedSwiftFile, ParseSwiftFileError> {
    parse_swift_file_permissive(parser, path)
}
```

- [x] **Step 4: Wire file collection and parser into scans**

Modify `scan_repository` in `tree_sitter_scan.rs`:

```rust
let mut parser =
    new_parser().map_err(|reason| TreeSitterScanError::ParserInitFailed { reason })?;
let mut swift_files = Vec::new();
let mut diagnostics = Vec::new();
for root in &config.roots {
    let resolved = resolve_source_roots(repo_root, root).map_err(map_root_resolution_error)?;
    if resolved.roots.is_empty() {
        diagnostics.push(...);
    } else {
        for abs_root in resolved.roots {
            collect_swift_files(&abs_root, &mut swift_files).map_err(|source| {
                TreeSitterScanError::Io {
                    context: format!("read Swift root {}", abs_root.display()),
                    source,
                }
            })?;
        }
    }
}
swift_files.sort();
```

In the file loop, increment `files_scanned`; on `ParseSwiftFileError::ParseFailed(_)`, push:

```rust
Diagnostic {
    severity: DiagnosticSeverity::Warning,
    code: "parse_failed".to_owned(),
    message: format!("tree-sitter failed to parse {relative_file}; file skipped"),
    location: None,
}
```

Set status to `Partial` if any `parse_failed`, `root_not_found`, or `root_glob_not_found` diagnostic exists.

- [x] **Step 5: Run parser-focused tests**

Run:

```bash
cd engine
cargo test -p wax-lang-swift swift_ast
cargo test -p wax-lang-swift --test config_validation
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: PASS.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-lang-swift/src/swift_ast.rs engine/crates/wax-lang-swift/src/tree_sitter_scan.rs
git commit -m "feat: parse Swift source files"
```

## Phase 3 - SwiftUI Detection and Scan Facts

### Task 4: Detect SwiftUI components and usage sites

**Files:**
- Create: `engine/crates/wax-lang-swift/src/component_detect.rs`
- Modify: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
- Add tests inside: `engine/crates/wax-lang-swift/src/component_detect.rs`
- Add tests inside: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`

- [x] **Step 1: Write component detection unit tests**

Create `engine/crates/wax-lang-swift/src/component_detect.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::swift_ast::new_parser;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = new_parser().expect("parser");
        parser.parse(source.as_bytes(), None).expect("parse")
    }

    #[test]
    fn detects_view_struct_and_some_view_function() {
        let source = r#"
            struct ProfileCard: View {
                var body: some View { Text("Profile") }
            }

            public func PrimaryButton(title: String) -> some View {
                Button(title) {}
            }
        "#;
        let tree = parse(source);
        let symbols = collect_component_declarations(tree.root_node(), source.as_bytes(), false);

        assert!(symbols.iter().any(|component| component.symbol == "ProfileCard"));
        assert!(symbols.iter().any(|component| component.symbol == "PrimaryButton"));
    }

    #[test]
    fn discovery_skips_private_and_fileprivate_symbols() {
        let source = r#"
            private struct PrivateCard: View {
                var body: some View { Text("Private") }
            }
            fileprivate func FilePrivateButton() -> some View {
                Button("Nope") {}
            }
            internal struct PublicEnoughCard: View {
                var body: some View { Text("Card") }
            }
        "#;
        let tree = parse(source);
        let symbols = collect_component_declarations(tree.root_node(), source.as_bytes(), true);

        assert!(!symbols.iter().any(|component| component.symbol == "PrivateCard"));
        assert!(!symbols.iter().any(|component| component.symbol == "FilePrivateButton"));
        assert!(symbols.iter().any(|component| component.symbol == "PublicEnoughCard"));
    }
}
```

The real helper should return a small internal struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedComponent {
    pub(crate) symbol: String,
    pub(crate) line: u32,
    pub(crate) column: u32,
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-lang-swift component_detect
```

Expected: FAIL because detection helpers are not implemented.

- [x] **Step 3: Implement SwiftUI declaration predicates**

Implement in `component_detect.rs`:

```rust
//! Shared SwiftUI component detection helpers.

/// A SwiftUI component declaration discovered in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedComponent {
    /// Source symbol name.
    pub(crate) symbol: String,
    /// One-based source line.
    pub(crate) line: u32,
    /// One-based source column.
    pub(crate) column: u32,
}

/// Collects SwiftUI component declarations from a parsed Swift syntax tree.
pub(crate) fn collect_component_declarations(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    discovery_visibility: bool,
) -> Vec<DetectedComponent> {
    let mut components = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "struct_declaration" => {
                if let Some(component) = component_from_type_declaration(node, source, discovery_visibility) {
                    components.push(component);
                }
            }
            "function_declaration" => {
                if let Some(component) = component_from_function_declaration(node, source, discovery_visibility) {
                    components.push(component);
                }
            }
            _ => {}
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
    components.sort_by(|left, right| left.symbol.cmp(&right.symbol));
    components
}
```

Use tree-sitter node text helpers to implement:

- `component_from_type_declaration`
  - name starts uppercase.
  - declaration text contains `: View` or an inherited type list containing `View`.
  - declaration text contains `body` and `some View`.
- `component_from_function_declaration`
  - name starts uppercase.
  - declaration text contains `-> some View`.
- `is_private_for_discovery`
  - rejects `private` and `fileprivate` when `discovery_visibility` is true.

Prefer named child traversal where the Swift grammar exposes stable node kinds; use bounded declaration text checks only where the grammar shape is too broad.

- [x] **Step 4: Write usage extraction tests**

Add tests to `tree_sitter_scan.rs`:

```rust
#[test]
fn direct_member_and_alias_calls_resolve_to_registry_symbols() {
    let resolve = resolve_map(&[
        ("PrimaryButton", "PrimaryButton"),
        ("PrimaryCTA", "PrimaryButton"),
        ("Card", "Card"),
    ]);
    let (_, usages) = parse_and_extract(
        r#"
        struct Screen: View {
            var body: some View {
                VStack {
                    PrimaryButton(title: "Save")
                    DesignSystem.PrimaryCTA(title: "Go")
                    DS.Card { Text("Body") }
                }
            }
        }
        "#,
        &resolve,
    );

    assert_eq!(usages.len(), 3);
    assert_eq!(usages[0].registry_symbol.as_deref(), Some("PrimaryButton"));
    assert_eq!(usages[1].registry_symbol.as_deref(), Some("PrimaryButton"));
    assert_eq!(usages[2].registry_symbol.as_deref(), Some("Card"));
}

#[test]
fn comments_strings_and_non_registry_calls_are_ignored() {
    let resolve = resolve_map(&[("PrimaryButton", "PrimaryButton")]);
    let (_, usages) = parse_and_extract(
        r#"
        let label = "PrimaryButton(title:)"
        // PrimaryButton(title: "No")
        func Screen() -> some View {
            LocalCard()
        }
        "#,
        &resolve,
    );

    assert!(usages.is_empty());
}
```

- [x] **Step 5: Implement scan extraction**

Add `extract_from_source` to `tree_sitter_scan.rs`:

```rust
fn extract_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    resolve_targets: &BTreeMap<String, String>,
    local_components: &mut Vec<LocalComponent>,
    usage_sites: &mut Vec<UsageSite>,
) {
    for component in collect_component_declarations(root, source, false) {
        local_components.push(LocalComponent {
            id: format!("local.{file}:{}:{}", component.line, component.symbol),
            symbol: component.symbol,
            location: SourceLocation {
                file: file.to_owned(),
                line: component.line,
                column: Some(component.column),
            },
        });
    }

    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_call_expression_node(node) {
            if let Some((call_symbol, pos)) = call_final_member_name(node, source) {
                if let Some(registry_symbol) = resolve_targets.get(&call_symbol) {
                    let line = pos.row as u32 + 1;
                    let column = pos.column as u32 + 1;
                    usage_sites.push(UsageSite {
                        id: format!("usage.{file}:{line}:{column}:{call_symbol}"),
                        location: SourceLocation {
                            file: file.to_owned(),
                            line,
                            column: Some(column),
                        },
                        symbol: call_symbol.clone(),
                        match_status: MatchStatus::Resolved,
                        registry_symbol: Some(registry_symbol.clone()),
                    });
                }
            }
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push(child);
            }
        }
    }
}
```

Implement `is_call_expression_node` and `call_final_member_name` against the actual `tree-sitter-swift` grammar. Test with:

```bash
cd engine
cargo test -p wax-lang-swift direct_member_and_alias_calls_resolve_to_registry_symbols -- --nocapture
```

If the grammar uses a different node kind than `call_expression`, update the helper and tests together. The invariant is source behavior, not the exact grammar node names.

- [x] **Step 6: Add Compose-parity scan edge-case tests**

Add unit tests in `tree_sitter_scan.rs`:

- `multiline_call_is_detected_at_first_line`: call `PrimaryButton(` across multiple lines and assert the `UsageSite` line/column points at the first call token.
- `missing_root_emits_warning_diagnostic_and_partial_status`: configure a literal missing root and assert `root_not_found` plus `ScanStatus::Partial`.
- `unmatched_wildcard_root_emits_glob_warning`: configure a wildcard root that matches no directories and assert `root_glob_not_found`.
- `wildcard_root_scans_each_matching_module`: create two matching module directories and assert both contribute Swift files and usage sites.
- `recursive_wildcard_root_scans_nested_modules`: create nested module directories and assert recursive wildcard roots scan nested Swift files.
- `partial_parse_still_extracts_symbols_during_scan`: scan one valid file and one broken file, assert valid usage still appears, and assert `parse_failed` plus `ScanStatus::Partial`.

The wildcard tests should mirror the root-resolution behavior in Compose and use `wax-lang-api::resolve_source_roots` indirectly through `scan_repository`.

- [x] **Step 7: Run focused detection tests**

Run:

```bash
cd engine
cargo test -p wax-lang-swift component_detect
cargo test -p wax-lang-swift tree_sitter_scan
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: PASS.

- [x] **Step 8: Commit**

```bash
git add engine/crates/wax-lang-swift/src/component_detect.rs engine/crates/wax-lang-swift/src/tree_sitter_scan.rs
git commit -m "feat: detect SwiftUI components and usage"
```

### Task 5: Add golden SwiftUI scan fixture

**Files:**
- Create: `engine/crates/wax-lang-swift/tests/fixtures/small/design-system/registry.json`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/Sample.swift`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/Extended.swift`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/FalsePositives.swift`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/small/alt-design-system/registry.json`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/small/golden.json`
- Create: `engine/crates/wax-lang-swift/tests/golden_small.rs`

- [x] **Step 1: Create the registry fixture**

Create `engine/crates/wax-lang-swift/tests/fixtures/small/design-system/registry.json`:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "aliases": ["PrimaryCTA"],
      "targets": ["swift"]
    },
    {
      "id": "ds.card",
      "symbol": "Card",
      "targets": ["swift"]
    },
    {
      "id": "ds.compose-only",
      "symbol": "ComposeOnly",
      "targets": ["compose"]
    }
  ]
}
```

- [x] **Step 2: Create the Swift fixture**

Create `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/Sample.swift`:

```swift
import SwiftUI
import DesignSystem

struct LocalScreen: View {
    var body: some View {
        VStack {
            PrimaryButton(title: "Save")
            DesignSystem.PrimaryCTA(title: "Continue")
            DS.Card {
                Text("Details")
            }
            LocalCard()
        }
    }
}

struct LocalCard: View {
    var body: some View {
        Text("Local")
    }
}

func LocalFactory() -> some View {
    Card {
        Text("Factory")
    }
}
```

Create `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/Extended.swift`:

```swift
import SwiftUI
import DesignSystem

struct ExtendedScreen: View {
    var body: some View {
        VStack {
            PrimaryCTA(
                title: "Alias"
            )
            DesignSystem.Card {
                Text("Qualified")
            }
        }
    }
}
```

Create `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/FalsePositives.swift`:

```swift
import SwiftUI

struct FalsePositives: View {
    var body: some View {
        Text("PrimaryButton(title:)")
        // Card { Text("comment") }
        LocalCard()
    }
}
```

- [x] **Step 3: Create golden counts**

Create `engine/crates/wax-lang-swift/tests/fixtures/small/golden.json`:

```json
{
  "usage_site_count": 6,
  "resolved_count": 6,
  "local_component_count": 5,
  "design_system_component_count": 2
}
```

- [x] **Step 4: Write the golden test**

Create `engine/crates/wax-lang-swift/tests/golden_small.rs`:

```rust
use std::fs;
use std::path::Path;
use wax_contract::ScanStatus;
use wax_lang_api::{ScanConfig, ScanRequest, ScanRequestType};
use wax_lang_swift::SwiftLanguage;

#[test]
fn golden_small_swiftui_fixture_matches_counts() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/small");
    let golden: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(fixture.join("golden.json")).unwrap()).unwrap();

    let mut config = ScanConfig::new();
    config.insert(
        "registry".to_owned(),
        serde_json::json!("design-system/registry.json"),
    );
    config.insert("roots".to_owned(), serde_json::json!(["app/Sources"]));

    let request = ScanRequest {
        request_type: ScanRequestType::Scan,
        api_version: 1,
        language_id: "swift".try_into().unwrap(),
        repo_root: fixture.to_string_lossy().to_string(),
        snapshot_id: "snap-swift-golden".to_owned(),
        config,
    };

    let facts = SwiftLanguage::new().scan(&request).unwrap();

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(
        facts.counts.usage_site_count,
        golden["usage_site_count"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.resolved_count,
        golden["resolved_count"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.local_component_count,
        golden["local_component_count"].as_u64().unwrap() as u32
    );
    assert_eq!(
        facts.counts.design_system_component_count,
        golden["design_system_component_count"].as_u64().unwrap() as u32
    );
    assert!(facts.usage_sites.iter().any(|site| {
        site.symbol == "PrimaryCTA" && site.registry_symbol.as_deref() == Some("PrimaryButton")
    }));
    assert!(
        !facts
            .design_system_components
            .iter()
            .any(|component| component.symbol == "ComposeOnly")
    );
}

#[test]
fn scan_status_is_complete_when_configured() {
    let facts = scan_small_fixture();

    assert_eq!(facts.status, ScanStatus::Complete);
    assert_eq!(facts.language.parser_name, "tree-sitter-swift");
}

#[test]
fn alias_usage_resolves_to_canonical_symbol() {
    let facts = scan_small_fixture();
    let alias_site = facts
        .usage_sites
        .iter()
        .find(|site| site.symbol == "PrimaryCTA")
        .expect("alias usage should be present");

    assert_eq!(alias_site.registry_symbol.as_deref(), Some("PrimaryButton"));
}
```

Refactor the test helper to expose `scan_small_fixture() -> wax_contract::ScanFacts` so these tests are separate, matching the Compose golden pattern.

- [x] **Step 5: Run the golden test**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test golden_small
cargo test -p wax-lang-swift
```

Expected: PASS.

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-lang-swift/tests engine/crates/wax-lang-swift/src
git commit -m "test: add SwiftUI golden scan fixture"
```

## Phase 4 - Registry Discovery

### Task 6: Implement Swift registry discovery

**Files:**
- Create: `engine/crates/wax-lang-swift/src/discover.rs`
- Modify: `engine/crates/wax-lang-swift/src/lib.rs`
- Modify: `engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/discover/design-system/Sources/Components.swift`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/discover/design-system/Sources/DuplicateComponents.swift`
- Create: `engine/crates/wax-lang-swift/tests/fixtures/discover/broken/Sources/Broken.swift`
- Create: `engine/crates/wax-lang-swift/tests/registry_discover.rs`

- [x] **Step 1: Create discovery fixture**

Create `engine/crates/wax-lang-swift/tests/fixtures/discover/design-system/Sources/Components.swift`:

```swift
import SwiftUI

public struct PrimaryButton: View {
    public var body: some View { Text("Button") }
}

struct PackageCard: View {
    var body: some View { Text("Card") }
}

public func Badge() -> some View {
    Text("Badge")
}

private struct PrivateTokenView: View {
    var body: some View { Text("Private") }
}

fileprivate func FilePrivateThing() -> some View {
    Text("No")
}

struct lowercase: View {
    var body: some View { Text("No") }
}
```

Create `engine/crates/wax-lang-swift/tests/fixtures/discover/design-system/Sources/DuplicateComponents.swift`:

```swift
import SwiftUI

public struct PrimaryButton: View {
    public var body: some View { Text("Duplicate") }
}

enum NestedNamespace {
    struct NestedCard: View {
        var body: some View { Text("Nested") }
    }
}
```

Create `engine/crates/wax-lang-swift/tests/fixtures/discover/broken/Sources/Broken.swift`:

```swift
import SwiftUI

public struct BrokenCard: View {
    var body: some View {
        Text("Broken")
```

- [x] **Step 2: Write discovery tests**

Create `engine/crates/wax-lang-swift/tests/registry_discover.rs`:

```rust
use std::path::PathBuf;
use wax_lang_swift::discover::discover_registry_symbols;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/design-system/Sources")
}

#[test]
fn discovers_public_and_package_swiftui_symbols() {
    let symbols = discover_registry_symbols(&[fixture_root()]).expect("discover symbols");

    assert_eq!(symbols, vec!["Badge", "PackageCard", "PrimaryButton"]);
}

#[test]
fn missing_discovery_root_fails() {
    let missing = fixture_root().join("missing");
    let err = discover_registry_symbols(&[missing]).expect_err("missing root should fail");

    assert!(err.to_string().contains("discovery root does not exist"));
}

#[test]
fn parse_failures_are_reported() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/broken/Sources");
    let err = discover_registry_symbols(&[root]).expect_err("parse should fail");

    assert!(err.to_string().contains("failed to parse"));
    assert!(err.to_string().contains("Broken.swift"));
}

#[test]
fn duplicate_symbols_are_deduped_and_nested_symbols_are_excluded() {
    let symbols = discover_registry_symbols(&[fixture_root()]).expect("discover symbols");

    assert_eq!(
        symbols.iter().filter(|symbol| *symbol == "PrimaryButton").count(),
        1
    );
    assert!(!symbols.iter().any(|symbol| symbol == "NestedCard"));
}
```

Also add a `SwiftLanguage::discover` wrapper test that sends repo-relative roots through `DiscoverRequest` and asserts the same symbol list. This catches bugs in root joining separately from `discover_registry_symbols`.

- [x] **Step 3: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test registry_discover
```

Expected: FAIL because `discover` is not implemented.

- [x] **Step 4: Implement discover module**

Create `engine/crates/wax-lang-swift/src/discover.rs`:

```rust
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

/// Discovers likely public SwiftUI component symbols from source roots.
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
        for component in collect_component_declarations(
            parsed.tree.root_node(),
            parsed.source.as_bytes(),
            true,
        ) {
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
```

Also implement `Display` and `Error` for `SwiftDiscoverError`, matching Compose wording.

- [x] **Step 5: Wire `SwiftLanguage::discover` to the module**

Modify `lib.rs`:

```rust
pub mod discover;
pub use discover::{SwiftDiscoverError, discover_registry_symbols};
```

In `SwiftLanguage::discover`, convert request roots to absolute paths:

```rust
let repo_root = Path::new(&request.repo_root);
let roots = request
    .roots
    .iter()
    .map(|root| repo_root.join(root))
    .collect::<Vec<_>>();
let symbols = discover_registry_symbols(&roots)?;
Ok(DiscoverSymbolsResult {
    symbols,
    diagnostics: Vec::new(),
})
```

Remove the scaffold-only `SwiftDiscoverError` from `lib.rs` so the discover module owns the type.

- [x] **Step 6: Run discovery tests**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test registry_discover
cargo test -p wax-lang-swift
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: PASS.

- [x] **Step 7: Commit**

```bash
git add engine/crates/wax-lang-swift/src engine/crates/wax-lang-swift/tests
git commit -m "feat: discover SwiftUI registry symbols"
```

### Task 7: Complete stdio scan and discover coverage

**Files:**
- Modify: `engine/crates/wax-lang-swift/tests/stdio_cli.rs`
- Modify: `engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs`

- [ ] **Step 1: Add stdio discover success test**

Append to `stdio_cli.rs`:

```rust
#[test]
fn stdio_discover_returns_symbols() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover");
    let mut child = Command::new(env!("CARGO_BIN_EXE_wax-lang-swift"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn wax-lang-swift");

    let request = format!(
        "{{\"type\":\"discover\",\"api_version\":{WIRE_API_VERSION},\"language_id\":\"swift\",\"repo_root\":\"{}\",\"roots\":[\"design-system/Sources\"]}}\n",
        repo_root.display()
    );
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write request");

    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());

    let response: WirePackResponse = serde_json::from_slice(&output.stdout).unwrap();
    match response {
        WirePackResponse::DiscoverSymbols {
            language_id,
            symbols,
            ..
        } => {
            assert_eq!(language_id.as_str(), "swift");
            assert_eq!(symbols, vec!["Badge", "PackageCard", "PrimaryButton"]);
        }
        other => panic!("expected discover symbols, got {other:?}"),
    }
}
```

- [ ] **Step 2: Add configured stdio scan coverage**

Add a Compose-style configured scan test named `stdio_cli_emits_one_scan_facts_response`. It should send a scan request for `tests/fixtures/small` with `registry` and `roots`, assert `snapshot_id`, `parser_name == "tree-sitter-swift"`, `ScanStatus::Complete`, and assert stdout contains exactly one JSON line.

Add scan error-path stdio tests:

- `stdio_scan_reports_partial_facts_for_parse_failure`: send a configured scan containing a broken Swift file and assert partial facts with `parse_failed`.
- `stdio_scan_missing_registry_returns_registry_not_found`: send a configured scan with a missing registry path and assert `WireErrorCode::RegistryNotFound`.

- [ ] **Step 3: Add typed error tests**

Append tests for:

- `unsupported_api_version_on_scan_returns_tagged_error_response`: send a scan request with `api_version: 2` and assert `WireErrorCode::ApiVersionUnsupported`.
- `unsupported_api_version_on_discover_returns_tagged_error_response`: send a discover request with `api_version: 2` and assert `WireErrorCode::ApiVersionUnsupported`.
- `invalid_json_returns_tagged_error_response`: send `{not json}\n` and assert `WireErrorCode::ConfigInvalid`.

Match React's assertions exactly, changing the language id to `swift` and command to `wax-lang-swift`.

- [ ] **Step 4: Add discover error-path stdio tests**

Add React-parity discover tests:

- `stdio_discover_missing_root_returns_config_invalid`: send a missing discover root and assert `WireErrorCode::ConfigInvalid`.
- `stdio_discover_wrong_language_id_returns_config_invalid`: send `language_id: "compose"` to `wax-lang-swift` and assert `WireErrorCode::ConfigInvalid`.
- `stdio_discover_parse_failure_returns_scan_failed`: send the broken discover fixture root and assert `WireErrorCode::ScanFailed`.

- [ ] **Step 5: Add bin-level wire tests**

Add a `#[cfg(test)] mod tests` block in `src/bin/wax-lang-swift.rs`, matching the Compose/React binary modules, with:

- `invalid_json_returns_tagged_error_response`: assert malformed JSON returns `WireErrorCode::ConfigInvalid`.
- `wrong_language_id_echoes_request_language_id`: send the wrong language id and assert the error response preserves the request language id.
- `unsupported_api_version_returns_tagged_error_response`: assert `WireErrorCode::ApiVersionUnsupported`.
- `scan_response_preserves_snapshot_id`: send a scaffold or configured scan and assert the returned facts keep the request snapshot id.

- [ ] **Step 6: Run stdio tests**

Run:

```bash
cd engine
cargo test -p wax-lang-swift --test stdio_cli
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add engine/crates/wax-lang-swift/tests/stdio_cli.rs engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs
git commit -m "test: cover SwiftUI stdio protocol"
```

## Phase 5 - Release, Install, and Documentation

### Task 8: Promote Swift into release artifacts and pack index

**Files:**
- Modify: `engine/Cargo.toml`
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/build-release.sh`
- Modify: `scripts/generate-pack-index.sh`
- Modify: `scripts/test-generate-pack-index.sh`
- Modify: `scripts/check-release-workflow.rb`
- Modify: `engine/crates/wax-core/src/registry.rs`
- Modify: `engine/fixtures/registry/alpha-index.json`
- Modify: `engine/fixtures/registry/official-manifest.json`

- [ ] **Step 1: Add Swift to release metadata**

Modify `engine/Cargo.toml`:

```toml
alpha_index_binaries = ["wax", "wax-lang-compose", "wax-lang-basic", "wax-lang-react", "wax-lang-swift"]
```

- [ ] **Step 2: Add Swift to release binary loops**

Update `.github/workflows/release.yml`, `scripts/build-release.sh`, and `scripts/check-release-workflow.rb` wherever the binary set lists:

```text
wax wax-lang-compose wax-lang-basic wax-lang-react
```

Change it to:

```text
wax wax-lang-compose wax-lang-basic wax-lang-react wax-lang-swift
```

- [ ] **Step 3: Add Swift to pack index generation**

Modify `scripts/generate-pack-index.sh` language mapping so `swift` maps to `wax-lang-swift`:

```ruby
pack_id = case language_id
when "compose" then "wax-lang-compose"
when "basic" then "wax-lang-basic"
when "react" then "wax-lang-react"
when "swift" then "wax-lang-swift"
else
  abort("unknown language id #{language_id}")
end
```

- [ ] **Step 4: Update pack-index regression expectations**

Modify `scripts/test-generate-pack-index.sh` to include `wax-lang-swift` for every target. Add expected snippets parallel to React:

```json
"wax-lang-swift": {
  "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-swift-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz",
  "sha256": "..."
}
```

Use the deterministic fixture hashes produced by the script's existing test harness; do not invent production hashes.

- [ ] **Step 5: Update registry fixtures**

Add a Swift language entry to `engine/fixtures/registry/alpha-index.json` and `engine/fixtures/registry/official-manifest.json` with the same version, API version, command shape, and target matrix used by React:

```json
{
  "id": "swift",
  "version": "0.1.0-alpha.0",
  "api_version": 1,
  "command": ["./wax-lang-swift", "--stdio"],
  "ecosystem": "swiftui",
  "parser_name": "tree-sitter-swift",
  "parser_version": "0.7.3",
  "targets": {
    "aarch64-apple-darwin": {
      "url": "https://github.com/Daio-io/wax/releases/latest/download/wax-lang-swift-0.1.0-alpha.0-aarch64-apple-darwin.tar.gz",
      "sha256": "fixture-sha256"
    }
  }
}
```

Use the exact schema and target ordering from the existing fixture files.

- [ ] **Step 6: Update release archive assertions**

Update hardcoded archive/checksum counts from 16 to 20 wherever release checks assume 4 binaries x 4 targets. This includes `.github/workflows/release.yml` and `scripts/check-release-workflow.rb`.

- [ ] **Step 7: Update wax-core alpha index assertions**

Update `engine/crates/wax-core/src/registry.rs` tests that assert alpha index language ids so they expect:

```rust
["compose", "basic", "react", "swift"]
```

Run the focused test that owns the assertion:

```bash
cd engine
cargo test -p wax-core assert_alpha_index_matches_release
```

- [ ] **Step 8: Run local release smoke checks**

Run:

```bash
scripts/build-release.sh --target "$(rustc -vV | awk '/host/ { print $2 }')" --out-dir /tmp/wax-swift-release-smoke
cd engine
cargo test -p wax-core validates_pack_index_from_env -- --ignored
```

Expected: the local target artifacts include `wax-lang-swift`, and pack-index validation accepts the generated Swift entry.

- [ ] **Step 9: Run release checks**

Run:

```bash
scripts/test-generate-pack-index.sh
scripts/check-release-workflow.rb
cd engine
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 10: Record workflow dry-run requirement**

Before tagging a release that includes Swift, run the release workflow with `workflow_dispatch` against this branch or the release candidate branch and record the run URL in the task PR. Do not mark the release-promotion task complete until the dry-run has either passed or maintainers have explicitly deferred it.

- [ ] **Step 11: Commit**

```bash
git add engine/Cargo.toml .github/workflows/release.yml scripts/build-release.sh scripts/generate-pack-index.sh scripts/test-generate-pack-index.sh scripts/check-release-workflow.rb engine/fixtures/registry
git commit -m "feat: publish SwiftUI language pack artifacts"
```

### Task 9: Document SwiftUI language pack behavior

**Files:**
- Modify: `README.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Modify: `docs/plans/README.md`
- Modify: `engine/fixtures/config/example.waxrc`
- Modify: `packages/cli/package.json`
- Create: `docs/adr/2026-06-13-swift-language-pack.md`

- [ ] **Step 1: Update README language examples**

Update README sections that list supported packs from:

```text
compose, react, and basic
```

to include:

```text
compose, react, swift, and basic
```

Add a Swift config example:

```json
{
  "id": "swift",
  "enabled": true,
  "registry": ".wax/swift.registry.json",
  "roots": ["App/Sources"]
}
```

Document supported SwiftUI v1 matching:

```text
SwiftUI v1 detects `struct Name: View` components, `func Name(...) -> some View`
components, direct calls such as `PrimaryButton(...)`, and simple member-qualified
calls such as `DesignSystem.PrimaryButton(...)`.
```

- [ ] **Step 2: Update language-pack spec**

In `docs/specs/2026-05-16-language-packs-and-distribution.md`, add Swift to examples that currently mention future language packs, and document:

```text
Swift (`swift`) uses `tree-sitter-swift`, ecosystem `swiftui`, parser name
`tree-sitter-swift`, and the same scan/discover subprocess contract as Compose
and React.
```

- [ ] **Step 3: Add implementation ADR**

Create `docs/adr/2026-06-13-swift-language-pack.md`:

```markdown
# ADR: SwiftUI language pack

**Status:** Accepted (implemented)
**Date:** 2026-06-13
**Related:** [Design](../plans/2026-06-12-swift-language-pack-design.md) · [Implementation plan](../plans/2026-06-13-swift-language-pack-plan.md)

## Context

Wax supports parser-backed language packs for Compose and React. SwiftUI projects need
the same registry-backed scan and per-language discovery workflow without adding
Swift-specific logic to the engine.

## Decision

Add `wax-lang-swift` as a `tree-sitter-swift` backed language pack. Swift v1 detects
`struct Name: View` declarations, `func Name(...) -> some View` declarations, direct
registry-backed calls, and simple member-qualified calls by final member name. It
implements both `scan` and `discover` over the existing stdio wire protocol.

## Consequences

- SwiftUI projects can scan and discover design-system registries through the same
  CLI workflow as Compose and React.
- The scanner remains static and deterministic, but does not perform Swift module
  or type resolution.
- Future SwiftPM/Xcode/SourceKit-aware resolution can build on this pack without
  changing the engine contract.
```

- [ ] **Step 4: Update roadmap status after implementation completes**

Modify `docs/plans/README.md` order 8 row:

```markdown
| 8 | SwiftUI language pack | [2026-06-13-swift-language-pack-plan.md](./2026-06-13-swift-language-pack-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-13-swift-language-pack.md) |
```

If this task is done before the implementation PR merges, set implementation status to `in-progress` instead of `complete`.

- [ ] **Step 5: Align init/config examples**

Update `engine/fixtures/config/example.waxrc` to include an enabled Swift entry:

```json
{
  "id": "swift",
  "enabled": true,
  "registry": ".wax/swift.registry.json",
  "roots": ["App/Sources"]
}
```

If `packages/cli/package.json` enumerates shipped language-pack binaries, add `wax-lang-swift` beside Compose and React. If it does not enumerate binaries, leave the file unchanged and mention that in the task PR summary.

Add or update CLI init tests so `wax init --language swift` writes a Swift language entry, a per-language `.wax/swift.registry.json`, and a matching lockfile registry entry.

- [ ] **Step 6: Run documentation checks**

Run:

```bash
rg -n "compose, react|wax-lang-compose, wax-lang-basic, wax-lang-react|SwiftUI" README.md docs scripts .github engine/fixtures
cd engine
cargo fmt --all --check
cargo test --workspace
```

Expected: no stale supported-language lists remain except historical/archive references.

- [ ] **Step 7: Commit**

```bash
git add README.md docs/specs/2026-05-16-language-packs-and-distribution.md docs/plans/README.md docs/adr/2026-06-13-swift-language-pack.md
git commit -m "docs: document SwiftUI language pack"
```

## Phase 6 - Final Verification

### Task 10: Run full workspace verification and finish plan

**Files:**
- Modify: `docs/plans/2026-06-13-swift-language-pack-plan.md`
- Modify: `docs/plans/README.md`

- [ ] **Step 1: Run formatting**

Run:

```bash
cd engine
cargo fmt --all --check
```

Expected: PASS.

- [ ] **Step 2: Run full tests**

Run:

```bash
cd engine
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Run full clippy**

Run:

```bash
cd engine
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Run release script checks**

Run:

```bash
scripts/test-generate-pack-index.sh
scripts/check-release-workflow.rb
```

Expected: PASS.

- [ ] **Step 5: Mark the plan complete**

In `docs/plans/2026-06-13-swift-language-pack-plan.md`, check every completed task and step. In `docs/plans/README.md`, set order 8 implementation status to `complete` after the implementation PR lands on `main`.

- [ ] **Step 6: Commit final plan status**

```bash
git add docs/plans/2026-06-13-swift-language-pack-plan.md docs/plans/README.md
git commit -m "docs: complete SwiftUI language pack plan"
```

## Plan Self-Review

- Spec coverage: This plan covers scan config, registry matching, SwiftUI declaration detection, direct and member-qualified usage extraction, registry discovery, stdio routing, tests, release artifacts, pack index, docs, and roadmap status from `docs/plans/2026-06-12-swift-language-pack-design.md`.
- Scope: The plan excludes SwiftPM/Xcode/SourceKit module resolution, Swift-specific ignore config, conditional compilation evaluation, result-builder deep analysis, and typealias resolution, matching the design deferred-work list.
- Verification: The plan starts with focused `wax-lang-swift` tests and ends with workspace fmt, tests, clippy, and release script checks.
