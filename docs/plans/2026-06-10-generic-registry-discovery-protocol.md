# Generic Registry Discovery Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple `wax registry discover` from the in-process `wax-lang-compose` dependency by adding a subprocess wire-protocol discover request, so core orchestrates discovery generically and each language pack owns symbol extraction.

**Architecture:** Extend `wax-lang-api` with `discover` request/response variants on the existing stdio JSON protocol (one line in, one line out). `wax-core` resolves the installed pack command (lockfile + global state, same as scan), spawns the pack subprocess, receives symbol names + diagnostics, then converts symbols to registry JSON and writes atomically — unchanged orchestration from today's `registry_discovery.rs`. Compose implements discover by calling existing `discover_registry_symbols`; basic and react return `DiscoverUnsupported` until they add heuristics.

**Tech Stack:** Rust 2024, serde JSON, existing `wax-contract`, `wax-lang-api`, `wax-core`, `wax-cli`, language pack stdio binaries.

---

## Reference docs

- [Registry discovery ADR](../adr/2026-06-04-registry-discovery.md) — current Compose-first in-process exception
- [Registry discovery design](./archive/2026-06-04-registry-discovery-design.md) — UX, root selection, false-positive warnings
- [Language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) — wire protocol invariants
- [Rust engine ADR](../adr/2026-05-16-rust-engine-language-packs.md) — pack subprocess model

## Behavior change (document in ADR addendum)

Today `wax registry discover --language compose` works without a globally installed pack because core links `wax-lang-compose` in-process. After this plan, discover **requires the language pack to be installed** (same resolution path as scan: `wax.lock.json` + `~/.wax/langs/`). Repos using `--root` only still need the pack installed globally; lockfile pins the version.

Clear error when pack missing:

```text
registry discovery requires language pack `compose` to be installed; run `wax language install compose`
```

## File structure

| File | Responsibility |
|------|----------------|
| `engine/crates/wax-lang-api/src/protocol.rs` | Add `WirePackRequest`, `WirePackResponse`, `DiscoverRequest`, discover response variant |
| `engine/crates/wax-lang-api/src/lib.rs` | Re-export new protocol types |
| `engine/crates/wax-lang-api/tests/discover_protocol.rs` | Wire fixture roundtrips and rejection tests |
| `engine/crates/wax-core/src/subprocess_discover.rs` | Spawn pack, send discover request, parse response (mirror `subprocess_lang.rs`) |
| `engine/crates/wax-core/src/registry_discovery.rs` | Replace in-process compose call with subprocess discover; remove `wax-lang-compose` import |
| `engine/crates/wax-core/Cargo.toml` | Remove `wax-lang-compose` dependency |
| `engine/crates/wax-core/tests/subprocess_discover.rs` | Subprocess discover protocol tests with fixture script |
| `engine/crates/wax-core/tests/registry_discovery.rs` | Update to install compose pack fixture before discover |
| `engine/crates/wax-lang-compose/src/discover.rs` | Unchanged logic; add `discover()` wrapper returning symbols + diagnostics |
| `engine/crates/wax-lang-compose/src/lib.rs` | Export `ComposeLanguage::discover` |
| `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs` | Parse `WirePackRequest`, route scan vs discover |
| `engine/crates/wax-lang-basic/src/bin/wax-lang-basic.rs` | Same routing; discover returns `DiscoverUnsupported` |
| `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs` | Same routing; discover returns `DiscoverUnsupported` |
| `engine/crates/wax-cli/tests/registry_discover_command.rs` | Set up lockfile + installed compose pack in test harness |
| `docs/adr/2026-06-10-generic-registry-discovery-protocol.md` | ADR addendum recording subprocess discover decision |
| `docs/specs/2026-05-16-language-packs-and-distribution.md` | Document discover wire messages |
| `docs/plans/README.md` | Add this plan to roadmap |

---

## Phase 1 — Wire protocol

### Task 1: Add discover wire types

**Files:**
- Modify: `engine/crates/wax-lang-api/src/protocol.rs`
- Modify: `engine/crates/wax-lang-api/src/lib.rs`
- Create: `engine/crates/wax-lang-api/tests/discover_protocol.rs`

- [ ] **Step 1: Write the failing test**

Create `engine/crates/wax-lang-api/tests/discover_protocol.rs`:

```rust
use serde_json::json;
use std::str::FromStr;
use wax_contract::{Diagnostic, DiagnosticSeverity, LanguageId};
use wax_lang_api::{
    DiscoverRequest, DiscoverRequestType, WIRE_API_VERSION, WireErrorCode, WirePackRequest,
    WirePackResponse,
};

#[test]
fn discover_request_fixture_roundtrips_required_fields() {
    let fixture = json!({
        "type": "discover",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "roots": ["design-system/src/main/kotlin"]
    });

    let request: WirePackRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn discover_success_response_fixture_roundtrips() {
    let response = json!({
        "type": "discover_symbols",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "symbols": ["PrimaryButton", "SecondaryButton"],
        "diagnostics": [{
            "severity": "info",
            "code": "compose_discover_skipped_private",
            "message": "skipped 1 private composable"
        }]
    });

    let parsed: WirePackResponse = serde_json::from_value(response.clone()).unwrap();
    let back = serde_json::to_value(&parsed).unwrap();

    assert_eq!(back, response);
}

#[test]
fn discover_request_rejects_unknown_fields() {
    let request = json!({
        "type": "discover",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "roots": ["src"],
        "extra": true
    });

    assert!(serde_json::from_value::<WirePackRequest>(request).is_err());
}

#[test]
fn scan_request_still_deserializes_through_wire_pack_request() {
    let fixture = json!({
        "type": "scan",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "snapshot_id": "snap-123",
        "config": {}
    });

    let request: WirePackRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn discover_unsupported_error_code_serializes() {
    let response = json!({
        "type": "error",
        "api_version": WIRE_API_VERSION,
        "language_id": "react",
        "code": "discover_unsupported",
        "message": "react does not support registry discovery yet",
        "diagnostics": []
    });

    let parsed: WirePackResponse = serde_json::from_value(response).unwrap();
    match parsed {
        WirePackResponse::Error { code, .. } => {
            assert_eq!(code, WireErrorCode::DiscoverUnsupported);
        }
        other => panic!("expected error response, got {other:?}"),
    }
}

#[test]
fn in_process_discover_request_converts_to_wire_request() {
    let in_process = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: LanguageId::from_str("compose").unwrap(),
        repo_root: "/repo/root".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
    };

    let wire = WirePackRequest::from(in_process.clone());
    let back: DiscoverRequest = wire.try_into().expect("discover wire request converts back");

    assert_eq!(in_process, back);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-lang-api discover_protocol`
Expected: FAIL — `DiscoverRequest`, `WirePackRequest`, `WirePackResponse`, `DiscoverUnsupported` not defined

- [ ] **Step 3: Implement protocol types**

Add to `engine/crates/wax-lang-api/src/protocol.rs`:

```rust
/// In-process discover request used by the engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DiscoverRequest {
    /// Request kind discriminator.
    #[serde(rename = "type")]
    pub request_type: DiscoverRequestType,
    /// Wire API version expected by the engine.
    pub api_version: u32,
    /// Language pack identifier being queried.
    pub language_id: LanguageId,
    /// Absolute path to the repository root.
    pub repo_root: String,
    /// Repo-relative source roots to inspect.
    pub roots: Vec<String>,
}

/// Request kind for discover requests.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverRequestType {
    /// Execute a discover request.
    Discover,
}

/// Unified wire protocol request envelope for scan and discover.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WirePackRequest {
    /// Scan command issued over stdio.
    Scan {
        api_version: u32,
        language_id: LanguageId,
        repo_root: String,
        snapshot_id: String,
        config: ScanConfig,
    },
    /// Discover command issued over stdio.
    Discover {
        api_version: u32,
        language_id: LanguageId,
        repo_root: String,
        roots: Vec<String>,
    },
}

/// Unified wire protocol response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WirePackResponse {
    /// Successful scan response.
    ScanFacts {
        api_version: u32,
        language_id: LanguageId,
        #[serde(deserialize_with = "deserialize_validated_scan_facts")]
        facts: Box<ScanFacts>,
    },
    /// Successful discover response.
    DiscoverSymbols {
        api_version: u32,
        language_id: LanguageId,
        symbols: Vec<String>,
        diagnostics: Vec<Diagnostic>,
    },
    /// Failed response.
    Error {
        api_version: u32,
        language_id: LanguageId,
        code: WireErrorCode,
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
}
```

Add `DiscoverUnsupported` to `WireErrorCode`:

```rust
/// Language pack does not implement registry discovery.
DiscoverUnsupported,
```

Keep existing `WireScanRequest` / `WireScanResponse` as type aliases or `From` impls so scan-only call sites compile with minimal churn:

```rust
pub type WireScanRequest = WirePackRequest;
pub type WireScanResponse = WirePackResponse;
```

Add `From<ScanRequest> for WirePackRequest`, `From<DiscoverRequest> for WirePackRequest`, and `TryFrom<WirePackRequest> for DiscoverRequest`.

Export new types from `engine/crates/wax-lang-api/src/lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd engine && cargo test -p wax-lang-api discover_protocol`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add engine/crates/wax-lang-api/
git commit -m "feat: add discover wire protocol types to wax-lang-api"
```

---

## Phase 2 — Subprocess discover runner

### Task 2: Add subprocess discover in wax-core

**Files:**
- Create: `engine/crates/wax-core/src/subprocess_discover.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Create: `engine/crates/wax-core/tests/subprocess_discover.rs`

- [ ] **Step 1: Write the failing test**

Create `engine/crates/wax-core/tests/subprocess_discover.rs` with a fixture shell script (same pattern as `engine/crates/wax-core/tests/subprocess_protocol.rs`):

```rust
#[test]
fn subprocess_discover_parses_discover_symbols_response() {
    let script = fixture_discover_script();
    let extractor = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: vec![script.display().to_string()],
        timeout: Duration::from_secs(5),
    });

    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: "compose".try_into().unwrap(),
        repo_root: "/tmp/repo".to_owned(),
        roots: vec!["design-system/src/main/kotlin".to_owned()],
    };

    let result = extractor.discover(request).unwrap();

    assert_eq!(result.symbols, vec!["PrimaryButton".to_owned()]);
    assert!(result.diagnostics.is_empty());
}
```

The fixture script reads stdin, writes:

```json
{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton"],"diagnostics":[]}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-core subprocess_discover_parses_discover_symbols_response`
Expected: FAIL — module/type not found

- [ ] **Step 3: Implement subprocess discover**

Create `engine/crates/wax-core/src/subprocess_discover.rs` mirroring `subprocess_lang.rs`:

```rust
pub struct DiscoverSymbolsResult {
    pub symbols: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct SubprocessLanguageDiscoverer {
    manifest: SubprocessLanguageManifest,
}

impl SubprocessLanguageDiscoverer {
    pub fn discover(&self, request: DiscoverRequest) -> Result<DiscoverSymbolsResult, DiscoverError> {
        // spawn, write WirePackRequest::Discover JSON + newline to stdin,
        // read stdout line, parse WirePackResponse
    }
}
```

Reuse `SubprocessLanguageManifest`, stream spooling, timeout, and cancellation patterns from `subprocess_lang.rs`. Map `WireErrorCode::DiscoverUnsupported` to `DiscoverError::Unsupported`.

Export from `engine/crates/wax-core/src/lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd engine && cargo test -p wax-core subprocess_discover`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add engine/crates/wax-core/src/subprocess_discover.rs engine/crates/wax-core/src/lib.rs engine/crates/wax-core/tests/subprocess_discover.rs
git commit -m "feat: add subprocess registry discover runner in wax-core"
```

---

## Phase 3 — Rewire registry discovery orchestration

### Task 3: Replace in-process Compose with subprocess discover

**Files:**
- Modify: `engine/crates/wax-core/src/registry_discovery.rs`
- Modify: `engine/crates/wax-core/Cargo.toml`
- Modify: `engine/crates/wax-core/tests/registry_discovery.rs`

- [ ] **Step 1: Write the failing test**

Add to `engine/crates/wax-core/tests/registry_discovery.rs`:

```rust
#[test]
fn discover_without_installed_pack_returns_clear_error() {
    let repo = TestRepo::new("discover-missing-pack");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());

    let err = discover_with_config_roots(repo.path()).expect_err("missing installed pack");

    assert!(matches!(err, RegistryDiscoverError::PackNotInstalled { .. }));
}
```

Add helper `write_compose_lockfile` and `install_compose_pack_fixture` (copied/adapted from `engine/crates/wax-core/tests/scan_resolve.rs` patterns).

- [ ] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-core discover_without_installed_pack_returns_clear_error`
Expected: FAIL — test passes today because compose is in-process; or error variant missing

- [ ] **Step 3: Implement subprocess dispatch in registry_discovery**

Replace `build_registry` in `engine/crates/wax-core/src/registry_discovery.rs`:

```rust
fn discover_symbols(
    repo_root: &Path,
    language_id: &LanguageId,
    roots: &[PathBuf],
    pack_command: Vec<String>,
) -> Result<Vec<String>, RegistryDiscoverError> {
    let repo_relative_roots = roots
        .iter()
        .map(|root| {
            root.strip_prefix(repo_root)
                .map(|relative| relative.display().to_string())
                .map_err(|_| RegistryDiscoverError::RootEscapesRepo { /* existing fields */ })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let discoverer = SubprocessLanguageDiscoverer::new(SubprocessLanguageManifest {
        command: pack_command,
        timeout: DEFAULT_DISCOVER_TIMEOUT,
    });

    let request = DiscoverRequest {
        request_type: DiscoverRequestType::Discover,
        api_version: WIRE_API_VERSION,
        language_id: language_id.clone(),
        repo_root: repo_root.display().to_string(),
        roots: repo_relative_roots,
    };

    let result = discoverer.discover(request).map_err(map_discover_error)?;

    Ok(result.symbols)
}
```

Add `resolve_installed_pack_command(language_id, lockfile, global_state)` reusing `load_installed_manifest_for_locked` from `engine/crates/wax-core/src/lib.rs`.

Replace `RegistryDiscoverError::Discover { source: ComposeDiscoverError }` with pack-agnostic variants:

```rust
PackNotInstalled { language_id: LanguageId },
DiscoverSubprocess(#[from] DiscoverError),
DiscoverUnsupported { language_id: LanguageId },
```

Remove from `engine/crates/wax-core/Cargo.toml`:

```toml
wax-lang-compose = { path = "../wax-lang-compose" }
```

Update existing registry discovery tests to call `install_compose_pack_fixture` before discover.

- [ ] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-core registry_discovery subprocess_discover`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add engine/crates/wax-core/
git commit -m "refactor: run registry discovery through language pack subprocess"
```

---

## Phase 4 — Language pack handlers

### Task 4: Compose pack discover + unified stdio loop

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/lib.rs`
- Modify: `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs`
- Modify: `engine/crates/wax-lang-compose/tests/stdio_cli.rs`

- [ ] **Step 1: Write the failing test**

Add to `engine/crates/wax-lang-compose/tests/stdio_cli.rs`:

```rust
#[test]
fn stdio_cli_emits_discover_symbols_for_fixture_roots() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/design-system/src/main/kotlin");
    let repo_root = fixture_root.ancestors().nth(4).unwrap();

    let request = json!({
        "type": "discover",
        "api_version": 1,
        "language_id": "compose",
        "repo_root": repo_root.display().to_string(),
        "roots": ["design-system/src/main/kotlin"]
    });

    let output = run_stdio_request(&request);
    let response: WirePackResponse = serde_json::from_str(output.trim()).unwrap();

    match response {
        WirePackResponse::DiscoverSymbols { symbols, .. } => {
            assert!(symbols.contains(&"PrimaryButton".to_owned()));
            assert!(!symbols.contains(&"PrivateButton".to_owned()));
        }
        other => panic!("expected discover_symbols response, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-lang-compose stdio_cli_emits_discover_symbols_for_fixture_roots`
Expected: FAIL

- [ ] **Step 3: Implement Compose discover handler**

Add to `engine/crates/wax-lang-compose/src/lib.rs`:

```rust
impl ComposeLanguage {
    pub fn discover(&self, request: &DiscoverRequest) -> Result<DiscoverSymbolsResult, ComposeDiscoverError> {
        let compose_language_id =
            LanguageId::try_from("compose").expect("hardcoded compose id must be valid");

        if request.language_id != compose_language_id {
            return Err(ComposeDiscoverError::InvalidLanguageId(
                request.language_id.to_string(),
            ));
        }

        let repo_root = Path::new(&request.repo_root);
        let absolute_roots = request
            .roots
            .iter()
            .map(|root| repo_root.join(root))
            .collect::<Vec<_>>();

        let symbols = discover_registry_symbols(&absolute_roots)?;

        Ok(DiscoverSymbolsResult {
            symbols,
            diagnostics: Vec::new(),
        })
    }
}
```

Update `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs` stdio loop:

```rust
let request: WirePackRequest = serde_json::from_str(&line)?;

match request {
    WirePackRequest::Scan { .. } => { /* existing scan path */ }
    WirePackRequest::Discover { api_version, language_id, repo_root, roots } => {
        // validate api_version, build DiscoverRequest, call compose.discover(),
        // respond with WirePackResponse::DiscoverSymbols or Error
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-lang-compose stdio_cli`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add engine/crates/wax-lang-compose/
git commit -m "feat: add registry discover handler to wax-lang-compose stdio"
```

---

### Task 5: Basic and React stdio routing with unsupported discover

**Files:**
- Modify: `engine/crates/wax-lang-basic/src/bin/wax-lang-basic.rs`
- Modify: `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs`
- Modify: `engine/crates/wax-lang-basic/src/bin/wax-lang-basic.rs` tests section
- Modify: `engine/crates/wax-lang-react/tests/stdio_cli.rs`

- [ ] **Step 1: Write the failing tests**

For basic:

```rust
#[test]
fn discover_request_returns_discover_unsupported() {
    let input = Cursor::new(
        "{\"type\":\"discover\",\"api_version\":1,\"language_id\":\"basic\",\"repo_root\":\"/tmp/repo\",\"roots\":[\"src\"]}\n",
    );
    let mut output = Vec::new();
    run_stdio_with_reader(input, &mut output).unwrap();

    let response: WirePackResponse = serde_json::from_str(std::str::from_utf8(&output).unwrap().trim()).unwrap();
    match response {
        WirePackResponse::Error { code, .. } => {
            assert_eq!(code, WireErrorCode::DiscoverUnsupported);
        }
        other => panic!("expected error response, got {other:?}"),
    }
}
```

Mirror for react in `engine/crates/wax-lang-react/tests/stdio_cli.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd engine && cargo test -p wax-lang-basic discover_request_returns_discover_unsupported && cargo test -p wax-lang-react discover_request_returns_discover_unsupported`
Expected: FAIL

- [ ] **Step 3: Update stdio handlers**

Change both pack binaries to deserialize `WirePackRequest` instead of `WireScanRequest`. On `Discover` variant, return:

```rust
WirePackResponse::Error {
    api_version: WIRE_API_VERSION,
    language_id,
    code: WireErrorCode::DiscoverUnsupported,
    message: format!("{language_id} does not support registry discovery yet"),
    diagnostics: Vec::new(),
}
```

Ensure existing scan tests still pass unchanged.

- [ ] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-lang-basic && cargo test -p wax-lang-react stdio_cli`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add engine/crates/wax-lang-basic/ engine/crates/wax-lang-react/
git commit -m "feat: route discover requests in basic and react stdio handlers"
```

---

## Phase 5 — CLI integration and docs

### Task 6: Update CLI registry discover tests for installed pack requirement

**Files:**
- Modify: `engine/crates/wax-cli/tests/registry_discover_command.rs`

- [ ] **Step 1: Add test harness helper**

Add `setup_compose_discover_repo(repo: &Path)` that:

1. Copies compose fixture into repo
2. Writes `.wax/wax.config.json` with compose roots (when test needs config roots)
3. Writes `.wax/wax.lock.json` pinning compose version
4. Installs compose pack into isolated `WAX_STATE_DIR` pointing at `env!("CARGO_BIN_EXE_wax-lang-compose")`

Pattern: reuse global-state isolation from `engine/crates/wax-cli/tests/scan_command.rs`.

- [ ] **Step 2: Update each CLI test to call setup helper**

Replace tests that assume in-process compose works without install.

- [ ] **Step 3: Run CLI tests**

Run: `cd engine && cargo test -p wax-cli registry_discover`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add engine/crates/wax-cli/tests/registry_discover_command.rs
git commit -m "test: install compose pack in registry discover CLI tests"
```

---

### Task 7: Documentation and roadmap

**Files:**
- Create: `docs/adr/2026-06-10-generic-registry-discovery-protocol.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Modify: `docs/plans/README.md`
- Modify: `docs/adr/2026-06-04-registry-discovery.md` (add superseded note for in-process exception)

- [ ] **Step 1: Write ADR addendum**

Record:

- Discover uses subprocess wire protocol (no in-process pack deps in core)
- Discover requires installed pack (behavior change from v1 in-process shortcut)
- React discover deferred; returns `discover_unsupported`

- [ ] **Step 2: Document wire messages in language packs spec**

Add section under wire protocol:

```json
{"type":"discover","api_version":1,"language_id":"compose","repo_root":"/abs/repo","roots":["design-system/src/main/kotlin"]}
```

Success:

```json
{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton"],"diagnostics":[]}
```

- [ ] **Step 3: Add plan to roadmap**

In `docs/plans/README.md`, add order 7 row for this plan with `in-progress` status.

- [ ] **Step 4: Commit**

```bash
git add docs/
git commit -m "docs: add ADR and spec for subprocess registry discovery"
```

---

### Task 8: Full verification

**Files:** (none — verification only)

- [ ] **Step 1: Format and lint**

Run:

```bash
cd engine
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: clean

- [ ] **Step 2: Full test suite**

Run:

```bash
cd engine
cargo test --workspace
```

Expected: all tests PASS

- [ ] **Step 3: Manual smoke test**

```bash
cd engine && cargo build -p wax-cli -p wax-lang-compose
wax language install compose   # if not already installed
wax registry discover --language compose --root path/to/kotlin --dry-run
```

Expected: valid registry JSON on stdout, no core dependency on compose crate

- [ ] **Step 4: Commit any fmt fixes**

```bash
git add -A
git commit -m "chore: verify subprocess registry discovery workspace checks"
```

---

## Out of scope (follow-up plans)

- React symbol discovery heuristics (`wax-lang-react/src/discover.rs`)
- Shared stdio loop helper in `wax-lang-api` (only extract if Task 5 duplication hurts review)
- Auto-install policy for discover (could mirror scan later)
- Wire API version bump to 2 (not needed if discover is additive within v1)

## Self-review

| Requirement | Task |
|-------------|------|
| Subprocess discover protocol | Task 1, 2 |
| Remove core → compose dep | Task 3 |
| Compose discover implementation | Task 4 |
| Unsupported discover for other packs | Task 5 |
| CLI behavior preserved | Task 6 |
| Document behavior change | Task 7 |
| Workspace verification | Task 8 |

No placeholders remain. Type names consistent: `DiscoverRequest`, `WirePackRequest`, `WirePackResponse`, `DiscoverSymbols`, `DiscoverUnsupported`.

---

**Plan complete and saved to `docs/plans/2026-06-10-generic-registry-discovery-protocol.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
