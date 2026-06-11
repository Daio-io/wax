# Generic Registry Discovery Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** `in-progress` (active plan, order 7)

**Goal:** Decouple `wax registry discover` from the in-process `wax-lang-compose` dependency via a subprocess wire-protocol discover request, and write **per-language registry files** so multi-stack repos can discover compose and react independently with no merge or cross-language collisions.

**Architecture:** Extend `wax-lang-api` with `discover` request/response variants on the existing stdio JSON protocol (one line in, one line out). `wax-core` resolves the installed pack command (lockfile + global state, same as scan), spawns the pack subprocess, receives symbol names + diagnostics, builds flat registry JSON, and writes it to **that language's registry path only**. When the language entry has no `registry` configured, discover defaults to `.wax/<language-id>.registry.json`, patches `.wax/wax.config.json` to point the language at that path, and updates `wax.lock.json` `registries[<language-id>]` with the new source + digest. **`wax init` must scaffold the same model** — per enabled language, not one shared empty `.wax/wax.registry.json`. Scan already resolves registry per language — pack loaders unchanged. Compose implements discover via existing `discover_registry_symbols`; basic and react return `DiscoverUnsupported` until they add heuristics.

**Tech Stack:** Rust 2024, serde JSON, existing `wax-contract`, `wax-lang-api`, `wax-core`, `wax-cli`, language pack stdio binaries.

---

## Reference docs

- [Registry discovery ADR](../adr/2026-06-04-registry-discovery.md) — current Compose-first in-process exception
- [Registry discovery design](./archive/2026-06-04-registry-discovery-design.md) — UX, root selection, false-positive warnings
- [Language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) — wire protocol invariants
- [Rust engine ADR](../adr/2026-05-16-rust-engine-language-packs.md) — pack subprocess model

## Behavior change (document in ADR addendum)

**Subprocess discover:** Today discover works without a globally installed pack because core links `wax-lang-compose` in-process. After this plan, discover **requires the installed language pack** (same resolution as scan: `wax.lock.json` + `~/.wax/langs/`).

Clear error when pack missing:

```text
registry discovery requires language pack `compose` to be installed; run `wax language install compose`
```

**Per-language registry output:** Today discover always writes the shared default `.wax/wax.registry.json` and refuses a second run unless `--force` (which replaces the whole file). After this plan:

| Before | After |
|--------|-------|
| All languages write `.wax/wax.registry.json` | Each `--language` writes **its own** registry file |
| Second language discover conflicts with first | Each language writes its own registry file — no cross-language overwrite |
| `--force` replaces entire shared registry | `--force` replaces **only that language's** registry file |

**Output path resolution:**

Discover writes **local repo-relative registry files only**. It does not fetch or overwrite hosted `registry.source` URLs or `file://` sources.

| Config shape | Write target | Patch config on write? | Patch lock on write? |
|--------------|--------------|------------------------|----------------------|
| No `registry` / `design_system_registry` | `.wax/<language-id>.registry.json` | Yes — add string `"registry": ".wax/<id>.registry.json"` to the language entry | Yes — `registries[<id>]` |
| String `"registry": ".wax/compose.registry.json"` | That repo-relative path | No | Yes |
| Legacy string `"design_system_registry": "design-system/registry.json"` | That repo-relative path | No (keep legacy key; do not rewrite to `registry` in this plan) | Yes |
| Object `"registry": { "source": ".wax/compose.registry.json" }` | `registry.source` when repo-relative | No — preserve object shape | Yes |
| Object `"registry": { "source": "https://…" }` or `file://…` | — | — | Fail with `RegistryExternalSource` — discover cannot write non-repo-relative sources |
| Config loaded from legacy `.waxrc` only | Same rules via `discover_repo_files()` | Yes — patch the file that was loaded (`.waxrc` or `.wax/wax.config.json`) | Yes |

Validation rules (same as scan registry resolution):

- Repo-relative paths only for writable targets; reject `..` segments and absolute paths.
- Language entry must exist and be enabled when falling back to config roots without `--root`.

Examples:

```text
discover --language compose  →  .wax/compose.registry.json  (+ config + lock update when unconfigured)
discover --language react    →  .wax/react.registry.json      (+ config + lock update when unconfigured)
```

Duplicate symbols across language files (`Button` in both) are fine. Scan loads only the registry path resolved for the language being scanned.

**Dry-run:** prints registry JSON to stdout; does not write files or patch config/lock.

**Overwrite:** `OutputExists` applies only to the resolved output path for the requested language. Discovering react does not require `--force` because compose already wrote a different file.

**Init alignment:** `wax init` currently scaffolds one shared `.wax/wax.registry.json` and points every enabled language's lock entry at it. That contradicts per-language discover. This plan updates init to scaffold `.wax/<language-id>.registry.json` per enabled language, set each language's `registry` in config, and lock each file independently. Without this, discover would write files init never prepared users for.

## File structure

| File | Responsibility |
|------|----------------|
| `engine/crates/wax-lang-api/src/protocol.rs` | Add `WirePackRequest`, `WirePackResponse`, `DiscoverRequest`, discover response variant |
| `engine/crates/wax-lang-api/src/lib.rs` | Re-export new protocol types |
| `engine/crates/wax-lang-api/tests/discover_protocol.rs` | Wire fixture roundtrips and rejection tests |
| `engine/crates/wax-core/src/subprocess_discover.rs` | Spawn pack, send discover request, parse response (mirror `subprocess_lang.rs`) |
| `engine/crates/wax-core/src/registry_discovery.rs` | Subprocess discover; per-language output path; config + lockfile patch on write |
| `engine/crates/wax-core/src/config/repo_files.rs` | Add `default_registry_path_for_language(language_id)` helper |
| `engine/crates/wax-core/Cargo.toml` | Remove `wax-lang-compose` dependency |
| `engine/crates/wax-core/tests/subprocess_discover.rs` | Subprocess discover protocol tests with fixture script |
| `engine/crates/wax-core/tests/registry_discovery.rs` | Per-language output, multi-language no-collision, config/lock updates |
| `engine/crates/wax-cli/src/commands/init.rs` | Scaffold per-language registry files + config `registry` fields on init |
| `engine/crates/wax-cli/tests/init_command.rs` | Assert per-language registry paths, not shared default |
| `engine/crates/wax-lang-compose/src/discover.rs` | Unchanged logic; add `discover()` wrapper returning symbols + diagnostics |
| `engine/crates/wax-lang-compose/src/lib.rs` | Export `ComposeLanguage::discover` |
| `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs` | Parse `WirePackRequest`, route scan vs discover |
| `engine/crates/wax-lang-basic/src/bin/wax-lang-basic.rs` | Same routing; discover returns `DiscoverUnsupported` |
| `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs` | Same routing; discover returns `DiscoverUnsupported` |
| `engine/crates/wax-cli/src/commands/registry.rs` | Update success messages to show language-specific output path |
| `engine/crates/wax-cli/tests/registry_discover_command.rs` | Set up lockfile + installed compose pack in test harness |
| `docs/adr/2026-06-10-generic-registry-discovery-protocol.md` | ADR addendum recording subprocess discover + per-language output |
| `docs/specs/2026-05-16-language-packs-and-distribution.md` | Document discover wire messages and per-language output |
| `docs/plans/README.md` | Roadmap entry (order 7) |

---

## Phase 1 — Wire protocol

### Task 1: Add discover wire types

**Files:**
- Modify: `engine/crates/wax-lang-api/src/protocol.rs`
- Modify: `engine/crates/wax-lang-api/src/lib.rs`
- Create: `engine/crates/wax-lang-api/tests/discover_protocol.rs`

- [x] **Step 1: Write the failing test**

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
fn scan_request_still_deserializes_as_wire_scan_request() {
    let fixture = json!({
        "type": "scan",
        "api_version": WIRE_API_VERSION,
        "language_id": "compose",
        "repo_root": "/repo/root",
        "snapshot_id": "snap-123",
        "config": {}
    });

    let request: WireScanRequest = serde_json::from_value(fixture.clone()).unwrap();
    let back = serde_json::to_value(&request).unwrap();

    assert_eq!(back, fixture);
}

#[test]
fn wire_pack_request_scan_variant_roundtrips() {
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

- [x] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-lang-api --test discover_protocol`
Expected: FAIL — `DiscoverRequest`, `WirePackRequest`, `WirePackResponse`, `DiscoverUnsupported` not defined

- [x] **Step 3: Implement protocol types**

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

Keep **`WireScanRequest` / `WireScanResponse` as scan-only enums** (unchanged shape used by `subprocess_lang.rs` and existing pack destructuring). Add **`WirePackRequest` / `WirePackResponse`** as the superset envelopes used by pack stdio loops. Do **not** type-alias scan enums to the pack enums — that would make `let WireScanRequest::Scan { .. } = request` fail because the discover variant is refutable.

```rust
// protocol.rs — scan-only types stay for engine ↔ subprocess scan path
pub enum WireScanRequest { Scan { .. } }
pub enum WireScanResponse { ScanFacts { .. }, Error { .. } }

// pack stdio superset adds discover
pub enum WirePackRequest { Scan { .. }, Discover { .. } }
pub enum WirePackResponse { ScanFacts { .. }, DiscoverSymbols { .. }, Error { .. } }
```

Add conversions:

```rust
impl From<ScanRequest> for WireScanRequest { .. }          // existing
impl From<ScanRequest> for WirePackRequest { .. }          // Scan variant
impl From<WireScanRequest> for WirePackRequest { .. }      // for pack routing
impl From<DiscoverRequest> for WirePackRequest { .. }
impl TryFrom<WirePackRequest> for DiscoverRequest { .. }
```

Task 5–6 updates every pack stdio binary to deserialize `WirePackRequest` and `match` on `Scan` vs `Discover`. Leave `engine/crates/wax-core/src/subprocess_lang.rs` on `WireScanRequest` until a later cleanup unifies serialization.

Export new types from `engine/crates/wax-lang-api/src/lib.rs`.

- [x] **Step 4: Run test to verify it passes**

Run: `cd engine && cargo test -p wax-lang-api --test discover_protocol`
Expected: PASS

- [x] **Step 5: Commit**

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

- [x] **Step 1: Write the failing test**

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

- [x] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-core subprocess_discover_parses_discover_symbols_response`
Expected: FAIL — module/type not found

- [x] **Step 3: Implement subprocess discover**

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

- [x] **Step 4: Run test to verify it passes**

Run: `cd engine && cargo test -p wax-core subprocess_discover`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-core/src/subprocess_discover.rs engine/crates/wax-core/src/lib.rs engine/crates/wax-core/tests/subprocess_discover.rs
git commit -m "feat: add subprocess registry discover runner in wax-core"
```

---

## Phase 3 — Per-language registry output + subprocess discover

### Task 3: Per-language output path helper

**Files:**
- Modify: `engine/crates/wax-core/src/config/repo_files.rs`
- Create: `engine/crates/wax-core/tests/registry_discovery_paths.rs`

- [x] **Step 1: Write the failing test**

Create `engine/crates/wax-core/tests/registry_discovery_paths.rs`:

```rust
use wax_contract::LanguageId;
use wax_core::config::repo_files::default_registry_path_for_language;

#[test]
fn default_registry_path_uses_language_id_slug() {
    let compose = LanguageId::try_from("compose").unwrap();
    let react = LanguageId::try_from("react").unwrap();

    assert_eq!(
        default_registry_path_for_language(&compose),
        ".wax/compose.registry.json"
    );
    assert_eq!(
        default_registry_path_for_language(&react),
        ".wax/react.registry.json"
    );
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-core default_registry_path_uses_language_id_slug`
Expected: FAIL — helper not defined

- [x] **Step 3: Implement helper**

Add to `engine/crates/wax-core/src/config/repo_files.rs`:

```rust
/// Default per-language registry path when a language entry omits `registry`.
pub fn default_registry_path_for_language(language_id: &LanguageId) -> String {
    format!(".wax/{}.registry.json", language_id.as_str())
}
```

Export from `wax_core::config::repo_files` (already public module).

- [x] **Step 4: Run test to verify it passes**

Run: `cd engine && cargo test -p wax-core --test registry_discovery_paths`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-core/src/config/repo_files.rs engine/crates/wax-core/tests/registry_discovery_paths.rs
git commit -m "feat: add per-language default registry path helper"
```

---

### Task 4: Rewire registry discovery (subprocess + per-language writes)

**Files:**
- Modify: `engine/crates/wax-core/src/registry_discovery.rs`
- Modify: `engine/crates/wax-core/Cargo.toml`
- Modify: `engine/crates/wax-core/tests/registry_discovery.rs`

- [x] **Step 1: Write the failing tests**

Add to `engine/crates/wax-core/tests/registry_discovery.rs`:

```rust
#[test]
fn discover_writes_language_specific_default_registry_path() {
    let repo = TestRepo::new("discover-compose-default-path");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    install_compose_pack_fixture();

    let result = discover_with_config_roots(repo.path()).expect("discover should succeed");

    assert_eq!(
        result.output_path,
        repo.path().join(".wax/compose.registry.json")
    );
    assert!(!repo.path().join(".wax/wax.registry.json").exists());

    let config = fs::read_to_string(repo.path().join(".wax/wax.config.json")).unwrap();
    assert!(config.contains(r#""registry": ".wax/compose.registry.json""#));

    let lock = fs::read_to_string(repo.path().join(".wax/wax.lock.json")).unwrap();
    assert!(lock.contains(r#""compose""#));
    assert!(lock.contains(r#""source": ".wax/compose.registry.json""#));
}

#[test]
fn discover_compose_then_react_writes_both_without_force() {
    let repo = TestRepo::new("discover-multi-language");
    write_multi_language_config(repo.path());
    link_compose_fixture_into_repo(repo.path());
    write_multi_language_lockfile(repo.path());
    install_compose_pack_fixture();
    install_react_discover_fixture_pack(); // shell script returning discover_symbols JSON

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "compose",
        roots: vec![repo.path().join("design-system/src/main/kotlin")],
        dry_run: false,
        force: false,
    })
    .expect("compose discover");

    discover_registry(RegistryDiscoverOptions {
        repo_root: repo.path(),
        language_id: "react",
        roots: vec![repo.path().join("design-system/src")],
        dry_run: false,
        force: false,
    })
    .expect("react discover via fixture pack");

    let compose_path = repo.path().join(".wax/compose.registry.json");
    let react_path = repo.path().join(".wax/react.registry.json");
    assert!(compose_path.is_file());
    assert!(react_path.is_file());

    let compose_registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(compose_path).unwrap()).unwrap();
    let react_registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(react_path).unwrap()).unwrap();
    assert!(compose_registry["components"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["symbol"] == "PrimaryButton"));
    assert!(react_registry["components"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["symbol"] == "Button"));
}

#[test]
fn discover_rejects_external_registry_source() {
    let repo = TestRepo::new("discover-external-registry-source");
    write_config_with_registry_object(
        repo.path(),
        "compose",
        r#"{ "source": "https://example.com/registry.json" }"#,
    );
    write_compose_lockfile(repo.path());
    install_compose_pack_fixture();

    let err = discover_with_config_roots(repo.path())
        .expect_err("hosted registry source should not be writable by discover");

    assert!(matches!(
        err,
        RegistryDiscoverError::RegistryExternalSource { .. }
    ));
}

#[test]
fn discover_writes_configured_legacy_design_system_registry_path() {
    let repo = TestRepo::new("discover-legacy-registry-key");
    fs::write(
        repo.path().join(".waxrc"),
        r#"{
  "schema_version": 1,
  "languages": [{
    "id": "compose",
    "enabled": true,
    "design_system_registry": "design-system/registry.json",
    "roots": ["design-system/src/main/kotlin"]
  }]
}"#,
    )
    .unwrap();
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    install_compose_pack_fixture();

    let result = discover_with_config_roots(repo.path()).expect("legacy key discover");

    assert_eq!(
        result.output_path,
        repo.path().join("design-system/registry.json")
    );
}

#[test]
fn discover_without_installed_pack_returns_clear_error() {
    let repo = TestRepo::new("discover-missing-pack");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());

    let err = discover_with_config_roots(repo.path()).expect_err("missing installed pack");

    assert!(matches!(err, RegistryDiscoverError::PackNotInstalled { .. }));
}

#[test]
fn second_discover_for_same_language_fails_without_force() {
    let repo = TestRepo::new("discover-same-language-overwrite");
    write_compose_config_with_roots(repo.path(), &["design-system/src/main/kotlin"]);
    link_compose_fixture_into_repo(repo.path());
    write_compose_lockfile(repo.path());
    install_compose_pack_fixture();

    discover_with_config_roots(repo.path()).expect("first discover");
    let err = discover_with_config_roots(repo.path()).expect_err("second discover");

    assert!(matches!(err, RegistryDiscoverError::OutputExists { .. }));
}
```

Add helpers: `write_compose_lockfile`, `install_compose_pack_fixture`, `write_multi_language_config`, `write_multi_language_lockfile` (adapt from `engine/crates/wax-core/tests/scan_resolve.rs`).

Update existing tests that assert `.wax/wax.registry.json` to assert `.wax/compose.registry.json` instead.

- [x] **Step 2: Run tests to verify they fail**

Run: `cd engine && cargo test -p wax-core registry_discovery`
Expected: FAIL — still writes shared default path / in-process compose

- [x] **Step 3: Implement subprocess dispatch and per-language orchestration**

In `engine/crates/wax-core/src/registry_discovery.rs`:

**Resolve output path** — use `LanguageEntry::registry_source()` from `wax-core/src/config/waxrc.rs` and apply the table in **Behavior change**:

```rust
fn resolve_discover_output_path(
    repo_root: &Path,
    language_id: &LanguageId,
    entry: &LanguageEntry,
) -> Result<PathBuf, RegistryDiscoverError> {
    let repo_relative = match entry.registry_source() {
        Some(source) if is_external_registry_source(&source.source) => {
            return Err(RegistryDiscoverError::RegistryExternalSource {
                language_id: language_id.clone(),
                source: source.source,
            });
        }
        Some(source) => source.source,
        None => default_registry_path_for_language(language_id),
    };

    validate_repo_relative_registry_path(language_id, &repo_relative)?;
    Ok(repo_root.join(repo_relative))
}

fn should_patch_config_registry(entry: &LanguageEntry) -> bool {
    entry.registry_source().is_none()
}
```

Config patch rules:

- Patch only when `should_patch_config_registry` is true.
- Write `"registry": "<repo-relative path>"` (string form) into the language entry; do not remove other keys.
- When the loaded config file is legacy `.waxrc`, patch `.waxrc` in place (do not migrate to `.wax/wax.config.json` in this task).
- When the language already has `registry: { "source": "…" }` and discover writes that local path, preserve the object form.

**Replace `discover_registry` body:**

1. Load wax config + lockfile + global state.
2. Resolve discovery roots (unchanged).
3. Resolve output path for `language_id`.
4. Resolve installed pack command from lockfile + global state.
5. Call subprocess discover for symbols.
6. Build flat registry JSON (`schema_version`, `components[]`) — unchanged shape per file.
7. If `dry_run`, return JSON without writing.
8. If output exists and not `force`, return `OutputExists` for **that path only**.
9. Write registry atomically to output path.
10. If language had no configured `registry`, patch `.wax/wax.config.json` to set `"registry": "<repo-relative path>"`.
11. Update `wax.lock.json` `registries[language_id]` with `{ source, sha256 }` for the written file.

**Subprocess discover** (replace in-process compose):

```rust
fn discover_symbols(
    repo_root: &Path,
    language_id: &LanguageId,
    roots: &[PathBuf],
    pack_command: Vec<String>,
) -> Result<Vec<String>, RegistryDiscoverError> {
    // ... WirePackRequest::Discover via SubprocessLanguageDiscoverer (Task 2)
}
```

Replace `RegistryDiscoverError::Discover { source: ComposeDiscoverError }` with:

```rust
PackNotInstalled { language_id: LanguageId },
DiscoverSubprocess(#[from] DiscoverError),
DiscoverUnsupported { language_id: LanguageId },
RegistryExternalSource { language_id: LanguageId, source: String },
ConfigPatch { .. },
LockfilePatch { .. },
```

Remove from `engine/crates/wax-core/Cargo.toml`:

```toml
wax-lang-compose = { path = "../wax-lang-compose" }
```

Reuse `refresh_registry_locks_in_lockfile` logic from `engine/crates/wax-cli/src/commands/language.rs` — **move or duplicate the minimal lock digest helper into `wax-core`** so discover does not depend on `wax-cli`. Prefer extracting a small `registry_lock_refresh.rs` helper in `wax-core` if the CLI function is not reusable.

- [x] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-core registry_discovery subprocess_discover`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-core/
git commit -m "feat: per-language registry discover via language pack subprocess"
```

---

## Phase 4 — Language pack handlers

### Task 5: Compose pack discover + unified stdio loop

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/lib.rs`
- Modify: `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs`
- Modify: `engine/crates/wax-lang-compose/tests/stdio_cli.rs`

- [x] **Step 1: Write the failing test**

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

- [x] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-lang-compose stdio_cli_emits_discover_symbols_for_fixture_roots`
Expected: FAIL

- [x] **Step 3: Implement Compose discover handler**

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

- [x] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-lang-compose stdio_cli`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-lang-compose/
git commit -m "feat: add registry discover handler to wax-lang-compose stdio"
```

---

### Task 6: Basic and React stdio routing with unsupported discover

**Files:**
- Modify: `engine/crates/wax-lang-basic/src/bin/wax-lang-basic.rs`
- Modify: `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs`
- Modify: `engine/crates/wax-lang-basic/src/bin/wax-lang-basic.rs` tests section
- Modify: `engine/crates/wax-lang-react/tests/stdio_cli.rs`

- [x] **Step 1: Write the failing tests**

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

- [x] **Step 2: Run tests to verify they fail**

Run: `cd engine && cargo test -p wax-lang-basic discover_request_returns_discover_unsupported && cargo test -p wax-lang-react discover_request_returns_discover_unsupported`
Expected: FAIL

- [x] **Step 3: Update stdio handlers**

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

- [x] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-lang-basic && cargo test -p wax-lang-react stdio_cli`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-lang-basic/ engine/crates/wax-lang-react/
git commit -m "feat: route discover requests in basic and react stdio handlers"
```

---

## Phase 5 — CLI integration and docs

### Task 7: Update CLI registry discover tests and messages

**Files:**
- Modify: `engine/crates/wax-cli/tests/registry_discover_command.rs`
- Modify: `engine/crates/wax-cli/src/commands/registry.rs`

- [x] **Step 1: Add test harness helper**

Add `setup_compose_discover_repo(repo: &Path)` that:

1. Copies compose fixture into repo
2. Writes `.wax/wax.config.json` with compose roots (when test needs config roots)
3. Writes `.wax/wax.lock.json` pinning compose version
4. Installs compose pack into isolated `WAX_STATE_DIR` pointing at `env!("CARGO_BIN_EXE_wax-lang-compose")`

Pattern: reuse global-state isolation from `engine/crates/wax-cli/tests/scan_command.rs`.

- [x] **Step 2: Update CLI tests for per-language output paths**

Replace assertions on `.wax/wax.registry.json` with `.wax/compose.registry.json`:

```rust
let registry_path = repo.join(".wax/compose.registry.json");
assert!(registry_path.is_file());
assert!(!repo.join(".wax/wax.registry.json").exists());
```

Update stdout expectations:

```rust
assert!(stdout.contains("Wrote .wax/compose.registry.json"));
```

Add test: second discover for a **different** language id does not require `--force` on the first language's file. Use the Task 4 `discover_compose_then_react_writes_both_without_force` fixture-pack approach (react returns symbols from a test subprocess, not production `wax-lang-react` heuristics).

- [x] **Step 3: Update CLI success messages**

In `engine/crates/wax-cli/src/commands/registry.rs`, use `result.output_path` repo-relative display (already via `display_output_path`) — verify messages say the language-specific path, not hardcoded `.wax/wax.registry.json`.

- [x] **Step 4: Run CLI tests**

Run: `cd engine && cargo test -p wax-cli registry_discover`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add engine/crates/wax-cli/tests/registry_discover_command.rs engine/crates/wax-cli/src/commands/registry.rs
git commit -m "test: align registry discover CLI with per-language output paths"
```

---

---

### Task 8: Align `wax init` with per-language registries

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Modify: `engine/crates/wax-cli/tests/init_command.rs`

- [x] **Step 1: Write the failing test**

Add to `engine/crates/wax-cli/tests/init_command.rs`:

```rust
#[test]
fn init_scaffolds_per_language_registry_files_for_multi_language_repo() {
    let _guard = env_lock();
    let root = TestDir::new("init-per-language-registries");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_init(&repo, &["--language", "compose", "--language", "react", "--no-install"]);

    assert!(repo.join(".wax/compose.registry.json").is_file());
    assert!(repo.join(".wax/react.registry.json").is_file());
    assert!(!repo.join(".wax/wax.registry.json").exists());

    let waxrc = load_waxrc(&repo.join(".wax/wax.config.json")).unwrap();
    let compose = waxrc.languages.iter().find(|l| l.id.as_str() == "compose").unwrap();
    let react = waxrc.languages.iter().find(|l| l.id.as_str() == "react").unwrap();
    assert_eq!(
        compose.registry_source_string(),
        Some(".wax/compose.registry.json")
    );
    assert_eq!(react.registry_source_string(), Some(".wax/react.registry.json"));

    let lock = load_lockfile(&repo.join(".wax/wax.lock.json")).unwrap();
    assert_eq!(
        lock.registries.get(&lang("compose")).unwrap().source,
        ".wax/compose.registry.json"
    );
    assert_eq!(
        lock.registries.get(&lang("react")).unwrap().source,
        ".wax/react.registry.json"
    );
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd engine && cargo test -p wax-cli init_scaffolds_per_language_registry_files_for_multi_language_repo`
Expected: FAIL — init still writes shared `.wax/wax.registry.json`

- [x] **Step 3: Implement per-language init scaffold**

Replace `pending_default_registry_scaffold` (single shared file) with `pending_registry_scaffolds(languages)` returning one entry per enabled language:

```rust
struct PendingRegistryScaffold {
    language_id: LanguageId,
    path: PathBuf,
    sha256: String,
    contents: String,
}

fn pending_registry_scaffolds(
    repo_root: &Path,
    languages: &[LanguageId],
    scaffold_registries: bool,
) -> Vec<PendingRegistryScaffold> {
    if !scaffold_registries {
        return Vec::new();
    }

    languages
        .iter()
        .filter_map(|language_id| {
            let repo_relative = default_registry_path_for_language(language_id);
            let path = repo_root.join(&repo_relative);
            if path.exists() {
                return None;
            }
            let contents = rendered_file_contents(EXAMPLE_DESIGN_SYSTEM_REGISTRY);
            Some(PendingRegistryScaffold {
                language_id: language_id.clone(),
                path,
                sha256: sha256_hex(contents.as_bytes()),
                contents,
            })
        })
        .collect()
}
```

Update `build_waxrc_contents` to emit `"registry": ".wax/<id>.registry.json"` for each language when scaffolding.

Update init lockfile loop: each language gets its own `LockedRegistry { source, sha256 }` from its scaffold file (not one shared digest for all).

Write each scaffold file in the init loop.

- [x] **Step 4: Update existing init tests**

Replace assertions on `.wax/wax.registry.json` with `.wax/compose.registry.json` (single-language init) and add multi-language coverage.

- [x] **Step 5: Run init tests**

Run: `cd engine && cargo test -p wax-cli init_`
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add engine/crates/wax-cli/src/commands/init.rs engine/crates/wax-cli/tests/init_command.rs
git commit -m "feat: scaffold per-language registry files in wax init"
```

---

### Task 9: Documentation and roadmap

**Files:**
- Create: `docs/adr/2026-06-10-generic-registry-discovery-protocol.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Modify: `docs/plans/README.md`
- Modify: `docs/adr/2026-06-04-registry-discovery.md` (add superseded note for in-process exception)

- [ ] **Step 1: Write ADR addendum**

Record:

- Discover uses subprocess wire protocol (no in-process pack deps in core)
- Discover requires installed pack (behavior change from v1 in-process shortcut)
- Discover writes **per-language registry files** (default `.wax/<language-id>.registry.json`); multi-language discover does not merge or collide
- Patches config + lockfile for the discovered language on write
- React discover deferred; returns `discover_unsupported`

- [ ] **Step 2: Document wire messages and per-language discover output in language packs spec**

Add wire protocol section:

```json
{"type":"discover","api_version":1,"language_id":"compose","repo_root":"/abs/repo","roots":["design-system/src/main/kotlin"]}
```

Success:

```json
{"type":"discover_symbols","api_version":1,"language_id":"compose","symbols":["PrimaryButton"],"diagnostics":[]}
```

Add discover output section:

```text
Default output when language has no registry configured: .wax/<language-id>.registry.json
Explicit language registry in config: write to configured path
Multi-language: no merge; duplicate symbols across files are allowed
```

- [ ] **Step 3: Update registry discovery design cross-reference**

Add note to `docs/plans/archive/2026-06-04-registry-discovery-design.md` header that per-language output supersedes the v1 shared-file default (design archive stays historical; ADR addendum is authoritative).

- [ ] **Step 4: Commit**

```bash
git add docs/
git commit -m "docs: add ADR and spec for subprocess registry discovery"
```

---

### Task 10: Full verification

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
./target/debug/wax language install compose   # if not already installed
./target/debug/wax registry discover --language compose --root path/to/kotlin --dry-run
./target/debug/wax registry discover --language compose --root path/to/kotlin   # writes .wax/compose.registry.json
```

Alternatively: `cargo run -p wax-cli -- registry discover ...` (same flags).

Expected: valid registry JSON on stdout; write creates `.wax/compose.registry.json` (not shared default); config and lock updated; no core dependency on compose crate

- [ ] **Step 4: Commit any fmt fixes**

```bash
git add engine/
git commit -m "chore: verify subprocess registry discovery workspace checks"
```

---

## Out of scope (follow-up plans)

- React symbol discovery heuristics (`wax-lang-react/src/discover.rs`) — wire routing returns `discover_unsupported` in this plan
- Shared stdio loop helper in `wax-lang-api` (only extract if Task 6 duplication hurts review)
- Auto-install policy for discover (could mirror scan later)
- Wire API version bump to 2 (not needed if discover is additive within v1)
- Language-keyed sections inside one registry file (separate files per language is sufficient)

## Self-review

| Requirement | Task |
|-------------|------|
| Subprocess discover protocol | Task 1, 2 |
| Per-language registry output path | Task 3, 4 |
| Config + lockfile patch on write | Task 4 |
| Registry source shape matrix (string/object/legacy/external) | Task 4 |
| Multi-language no collision (fixture react pack) | Task 4, 7 |
| Remove core → compose dep | Task 4 |
| Compose discover implementation | Task 5 |
| Unsupported discover for other packs | Task 6 |
| CLI discover messages + tests | Task 7 |
| Init scaffolds per-language registries | Task 8 |
| Document behavior change | Task 9 |
| Workspace verification | Task 10 |

No placeholders remain. Type names consistent: `DiscoverRequest`, `WirePackRequest`, `WirePackResponse`, `WireScanRequest`, `WireScanResponse`, `DiscoverSymbols`, `DiscoverUnsupported`, `RegistryExternalSource`, `default_registry_path_for_language`.
