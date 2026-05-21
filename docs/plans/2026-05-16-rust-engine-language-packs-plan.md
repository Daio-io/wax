# Rust Engine and Language Packs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PR boundary:** Treat each checked **Task** as one implementation PR. Complete all steps inside a task, run its verification commands, commit the task, and open a PR before starting the next task. Phase checkpoints gate batches of task PRs; do not combine multiple tasks into one PR unless the human explicitly approves it.

**Goal:** Implement the production `wax` Rust engine with downloadable **language packs**, **`.waxrc`** configuration, global install lifecycle, and subprocess IPC—ready for review before broad foundation coding.

**Architecture:** A single **engine** orchestrates `scan`; each **language pack** is a downloaded native binary that returns normalized `ScanFacts` over **one JSON object per direction** on stdio (NDJSON multi-message deferred to daemon mode). Repo config enables languages; global `~/.wax/langs/` stores artifacts; `wax.lock.json` pins CI when used. **Plugins** (kernel hooks) are explicitly out of scope for this plan.

**Tech Stack:** Rust edition 2024, `wax-contract` / `wax-lang-api`, tree-sitter (Compose), SWC (React), serde JSON config, clap CLI, GitHub Releases + static registry manifest

**Spec (review first):** [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md)

---

## Verification note

Verification commands in this plan describe the future production `engine/` workspace. This PR still contains `rust-prototype/` as read-only reference material until Task 18 removes it.

## Decision rationale

Phase 0 compared TS-core and Go-core prototypes using source fixtures, golden outputs, and benchmark-oriented spikes. The provisional TS+TS direction had the lowest install friction, but it made the long-term multi-language boundary blurrier: every new ecosystem risked pulling parser/runtime concerns into the same package and making the analysis contract harder to keep stable.

This plan chooses a Rust engine with downloadable native language packs because it gives `wax` a small, deterministic kernel for scanning, merging, adoption metrics, and report output while keeping parser-heavy ecosystem work isolated behind a typed `ScanFacts` + stdio protocol boundary. Prebuilt `wax` and `wax-lang-*` artifacts preserve the “no local Rust toolchain” user experience, while `.waxrc`, `wax.lock.json`, and global pack installs give teams a path from easy local scans to reproducible CI.

## Prerequisites

- [ ] Spec [2026-05-16-language-packs-and-distribution.md](../specs/2026-05-16-language-packs-and-distribution.md) reviewed and decisions recorded in the ADR addendum.
- [ ] `rust-prototype/` remains read-only reference material. Do not evolve it into production code.
- [ ] Phase 0 spike artifacts (if used for compose goldens) live on a separate branch or PR—not required for the fresh production workspace.

## Execution model

- One task = one branch, one focused commit series, one PR.
- Task PRs should include the task number in the title, for example `Task 4: Wire protocol types (v1)`.
- A task is complete only when its listed verification command passes and the PR description links back to this plan.
- Phase checkpoints are review gates across multiple task PRs. For example, Phase 1 is not complete until Task 1, Task 2, Task 3, and Task 4 PRs are all merged or otherwise approved together.
- Keep task PRs narrow. If implementation reveals a missing prerequisite, stop and open a small plan/spec follow-up instead of silently expanding the task.

## File structure (fresh production layout)

Start a new Rust workspace under `engine/` for production. Use `rust-prototype/` only to understand prior API sketches; copy/adapt code only when it still matches the approved spec and the task PR makes that choice explicit.

```text
engine/
  Cargo.toml             # Rust workspace
  Cargo.lock             # committed for reproducible tool/CI builds
  crates/
    wax-contract/        # ScanFacts, LanguageMetadata, MergedScan, schema_version
    wax-lang-api/        # LanguageExtractor, ScanRequest, protocol types
    wax-core/            # Engine, merge, .waxrc loading
    wax-cli/             # user-facing `wax` binary
    wax-lang-compose/    # Compose language pack (library + bin target for subprocess)
    wax-lang-react/      # React language pack
  fixtures/
    config/
    registry/
docs/
  specs/2026-05-16-language-packs-and-distribution.md
  plans/2026-05-16-rust-engine-language-packs-plan.md
.waxrc                   # example in docs or fixtures only
```

Generated / local (gitignored):

```text
~/.wax/langs/<id>/<version>/
~/.wax/state.json
.wax/                    # scan output, cache (repo-local per AGENTS.md)
```

## Prototype patterns not to copy

When Phase 1 starts in `engine/`, do not blindly copy spike code from `rust-prototype/`:

- Do not use `#[serde(untagged)]` for success/error response envelopes; use a `type` discriminator (`scan_facts` / `error`).
- Do not return `String` errors from public contract helpers; use `thiserror` enums that preserve error context.
- Do not store timestamps as plain `String`; use typed timestamps with explicit JSON serialization.
- Do not re-export the entire `wax-contract` crate from `wax-lang-api`; expose only the types needed by the API.
- Do not duplicate `file` / `line` fields; use a shared `SourceLocation { file, line, column: Option<u32> }`.
- Do not hand-edit `engine/Cargo.lock`; generate it from a real `cd engine && cargo build`.
- Do not ship undocumented public contract types; `wax-contract` starts with `#![deny(missing_docs)]`.

---

## Phase 1 — Contract, config, and wire protocol freeze

**Execution checkpoint:** Do not start Phase 2+ implementation until Tasks 1–4 land together and are reviewed. These tasks freeze the shared data contract (`ScanFacts`), repo/global config shape, lockfile semantics, and wire request/response envelope that every later task depends on.

### - [x] Task 1: Freeze `ScanFacts` JSON schema

**Files:**
- Create: `engine/crates/wax-contract/Cargo.toml`
- Create: `engine/crates/wax-contract/src/lib.rs`
- Create: `engine/crates/wax-contract/schemas/scan-facts.schema.json`
- Test: `engine/crates/wax-contract/tests/schema_roundtrip.rs`

- [x] **Step 1: Document field meanings in spec**

Ensure [language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) matches `LanguageMetadata` + `ScanFacts.language` (not `plugin`).

- [x] **Step 2: Define production contract guardrails**

Implement the contract crate with:

- `#![deny(missing_docs)]`
- `LanguageId(String)` newtype with lowercase slug validation
- `SourceLocation { file, line, column: Option<u32> }`
- `LanguageMetadata.parser_name` and `LanguageMetadata.parser_version` as separate fields
- typed public errors using `thiserror`
- typed timestamps with RFC 3339 serialization
- `adoption_coverage_ratio = resolved_count / usage_site_count`, excluding candidates

- [x] **Step 3: Add serde roundtrip test**

```rust
#[test]
fn scan_facts_roundtrip() {
    let mut facts = minimal_facts(); // literal in tests/schema_roundtrip.rs
    facts.recompute_counts();
    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(facts.language.id, back.language.id);
}
```

- [x] **Step 4: Run test**

Run: `cd engine && cargo test -p wax-contract`
Expected: PASS

- [x] **Step 5: Commit** (when user requests commits)

```bash
git add engine/crates/wax-contract docs/specs/2026-05-16-language-packs-and-distribution.md
git commit -m "docs: freeze language pack scan facts contract"
```

### - [x] Task 2: `.waxrc` parser and validation

**Files:**
- Create: `engine/crates/wax-core/Cargo.toml`
- Create: `engine/crates/wax-core/src/config.rs`
- Create: `engine/crates/wax-core/src/config/waxrc.rs`
- Test: `engine/crates/wax-core/tests/waxrc_load.rs`
- Fixture: `engine/fixtures/config/minimal.waxrc`

- [x] **Step 1: Define Rust types**

```rust
#[derive(Debug, Deserialize)]
pub struct WaxRc {
    pub schema_version: u32,
    pub languages: Vec<LanguageEntry>,
}

#[derive(Debug, Deserialize)]
pub struct LanguageEntry {
    pub id: LanguageId,
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

- [x] **Step 2: Write failing test for minimal config**

```rust
#[test]
fn loads_minimal_waxrc() {
    let rc = load_waxrc("engine/fixtures/config/minimal.waxrc").unwrap();
    assert_eq!(rc.languages.len(), 1);
    assert_eq!(rc.languages[0].id, "compose");
}
```

- [x] **Step 3: Implement `load_waxrc(path)` with clear errors**

Reject unknown `schema_version` with actionable message.

- [x] **Step 4: Run test** — `cd engine && cargo test -p wax-core waxrc`

- [x] **Step 5: Commit** (when requested)

### - [x] Task 3: `wax.lock.json` parser

**Files:**
- Create: `engine/crates/wax-core/src/config/lockfile.rs`
- Test: `engine/crates/wax-core/tests/lockfile_load.rs`
- Fixture: `engine/fixtures/config/minimal.wax.lock.json`

- [x] **Step 1: Types for lockfile** (`engine_api_version`, `languages: BTreeMap<LanguageId, LockedLanguage>`)
- [x] **Step 2: Test load + version pin**
- [x] **Step 3: Reserve signature slot**

Add `resolved.signature: Option<SignatureRef>` to the lockfile shape. v1 writes `null`; v1.1 can fill this with Sigstore/cosign bundle metadata without another lockfile shape change.

- [x] **Step 4: `doctor` helper: compare `.waxrc` enabled ids vs lock keys**

### - [x] Task 4: Wire protocol types (v1)

**Files:**
- Create: `engine/crates/wax-lang-api/Cargo.toml`
- Create: `engine/crates/wax-lang-api/src/protocol.rs`
- Create: `engine/crates/wax-lang-api/src/lib.rs`

- [x] **Step 1: Align request types with spec**

Both in-process `ScanRequest` and wire `WireScanRequest` contain the same fields: `type`, `api_version`, `language_id`, `repo_root`, `snapshot_id`, and `config`. There is no `mode` field in v1.

- [x] **Step 2: Use tagged response envelopes**

`WireScanResponse` uses a `type` discriminator with `scan_facts` and `error` variants. Do not use untagged serde response parsing.

- [x] **Step 3: Define complete error code enum**

Include at least: `api_version_unsupported`, `config_invalid`, `registry_not_found`, `parser_init_failed`, `timeout`, `scan_failed`, `internal_error`.

- [x] **Step 4: Add wire fixture tests**

Add request, success, and error fixture tests in `engine/crates/wax-lang-api/tests/wire_protocol.rs`:

- request fixture roundtrips with `repo_root`, `language_id`, `api_version`, `snapshot_id`, and `config`
- in-process `ScanRequest -> JSON -> WireScanRequest::Scan -> JSON -> ScanRequest` conformance test fails if request fields drift
- success fixture requires `type: "scan_facts"`
- error fixture deserializes `registry_not_found`
- malformed/untagged response fails

- [x] **Step 5: Run wire protocol tests**

Run: `cd engine && cargo test -p wax-lang-api wire_protocol`
Expected: PASS

- [x] **Step 6: Review checkpoint**

Confirm Tasks 1–4 are consistent with each other before starting Phase 2+ work:

- `ScanFacts.schema_version` is enforced by `scan_facts_from_json`.
- `.waxrc` uses `design_system_registry` and keeps per-language config opaque to the engine.
- `wax.lock.json` records `api_version`, `source`, `resolved.target`, `resolved.url`, `resolved.sha256`, `wax_version`, and `locked_at`.
- `WireScanRequest` and in-process `ScanRequest` share fields.
- `WireScanResponse` supports tagged `type: "scan_facts"` success and tagged `type: "error"` failure.

---

## Phase 2 — Subprocess adapter and first pack entrypoints

Build on the frozen Phase 1 contracts. This phase proves that the engine can invoke an external language-pack binary and that first-party packs can speak the v1 stdio protocol.

### - [x] Task 5: Subprocess `LanguageExtractor` implementation

**Files:**
- Create: `engine/crates/wax-core/src/subprocess_lang.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`

- [x] **Step 1: Spawn `manifest.command`, write one `WireScanRequest::Scan` JSON to stdin, read stdout**
- [x] **Step 2: Parse tagged `WireScanResponse`; map timeout/cancel to `LanguageError::Timeout` / `Cancelled`**
- [x] **Step 3: Integration test with mock binary** (shell script that echoes canned JSON)

Run: `cd engine && cargo test -p wax-core subprocess`

### - [x] Task 6: `wax-lang-compose` stdio entrypoint

**Files:**
- Create: `engine/crates/wax-lang-compose/Cargo.toml` with `[[bin]] name = "wax-lang-compose"`
- Create: `engine/crates/wax-lang-compose/src/lib.rs`
- Create: `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs`

- [x] **Step 1: Read stdin lines until `Scan` message**
- [x] **Step 2: Call `ComposeLanguage::scan`, write a tagged `scan_facts` response as one JSON object to stdout**
- [x] **Step 3: Manual test**

```bash
cd engine
cargo build -p wax-lang-compose
echo '{"type":"scan","api_version":1,...}' | ./target/debug/wax-lang-compose --stdio
```

### - [x] Task 6b: `wax-lang-react` stdio entrypoint skeleton

**Files:**
- Create: `engine/crates/wax-lang-react/Cargo.toml`
- Create: `engine/crates/wax-lang-react/src/lib.rs`
- Create: `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs`
- Modify: `engine/Cargo.toml`

- [x] **Step 1: Add a crate skeleton**

Add `wax-lang-react` to the workspace with dependencies on `wax-contract` and `wax-lang-api`.

- [x] **Step 2: Implement a stub `ReactLanguage`**

Return `ScanFacts` with:

- `language.id = "react"`
- `status = ScanStatus::Partial`
- empty components and usage sites
- a diagnostic explaining React extraction is scaffolded but not implemented

- [x] **Step 3: Add `wax-lang-react --stdio`**

Read one `WireScanRequest::Scan` JSON object from stdin, call the stub language, and write one tagged `scan_facts` response to stdout.

- [x] **Step 4: Run a manual stdio smoke test**

```bash
cd engine
cargo build -p wax-lang-react
echo '{"type":"scan","api_version":1,"language_id":"react","repo_root":"/tmp/repo","snapshot_id":"test","config":{}}' \
  | ./target/debug/wax-lang-react --stdio
```

Expected: one valid `scan_facts` response with `language.id = "react"` and `snapshot_id = "test"`.

### - [x] Task 6c: Subprocess protocol conformance tests

**Files:**
- Test: `engine/crates/wax-core/tests/subprocess_protocol.rs`

- [x] **Step 1: Add subprocess adapter conformance test**

Use the mock binary from Task 5 to assert:

- success stdout is parsed as tagged `scan_facts` and validates the embedded `ScanFacts`
- structured `type: "error"` stdout maps to a pack failure
- large stdout is streamed or spooled safely without a fixed protocol cap
- timeout maps to `LanguageError::Timeout`

- [x] **Step 2: Run subprocess protocol tests**

Run: `cd engine && cargo test -p wax-core subprocess_protocol`
Expected: PASS

---

## Phase 3 — Global install and registry

### - [x] Task 7: Global paths and state

**Files:**
- Create: `engine/crates/wax-core/src/paths.rs`
- Create: `engine/crates/wax-core/src/global_state.rs`

- [x] **Step 1: `wax_home() -> ~/.wax` with `WAX_HOME` override**
- [x] **Step 2: `lang_install_dir(id, version) -> ~/.wax/langs/<id>/<version>`**
- [x] **Step 3: Load/save `state.json`**

### - [x] Task 7b: Auto-install policy

**Files:**
- Create: `engine/crates/wax-core/src/auto_install.rs`
- Test: `engine/crates/wax-core/tests/auto_install_policy.rs`

- [x] **Step 1: Define policy inputs**

Create a small pure policy API that takes `.waxrc` enabled ids, lockfile entries, installed manifests, CLI mode (`allow_auto_install`), and pack-index metadata.

- [x] **Step 2: Test required-lock behavior**

Assert enabled language packs require `wax.lock.json`; `--no-auto-install` fails when an enabled pack is missing locally; auto-install chooses the exact lockfile version/digest when allowed.

- [x] **Step 3: Test drift behavior**

Assert digest drift between lockfile and pack index refuses install, even when auto-install is enabled.

Run: `cd engine && cargo test -p wax-core auto_install_policy`
Expected: PASS

### - [x] Task 8a: Pack index and manifest client

**Files:**
- Create: `engine/crates/wax-core/src/registry.rs`
- Fixture: `engine/fixtures/registry/official-manifest.json`

- [x] **Step 1: Parse manifest entry** (id, version, api_version, targets map with url + sha256)
- [x] **Step 2: Fetch pack index from `file://` fixture URL** (no network in unit tests)
- [x] **Step 3: Select target artifact for host triple**
- [x] **Step 4: Run tests**

Run: `cd engine && cargo test -p wax-core registry`
Expected: PASS

### - [ ] Task 8b: Secure language install

**Files:**
- Create: `engine/crates/wax-core/src/install.rs`
- Test: `engine/crates/wax-core/tests/install_language.rs`

- [ ] **Step 1: Implement `install_language(id, version, target_triple)`**

Download artifact, verify sha256, unpack to a temp dir, write manifest, then atomically promote to `~/.wax/langs/<id>/<version>`.

- [ ] **Step 2: Harden install edge cases**

Add tests that cover:

- sha mismatch refuses install and leaves no active manifest.
- archive entries cannot write outside the install temp dir (`../` path traversal).
- partial installs are written to a temp dir and atomically promoted only after verification.
- installed binaries are executable on Unix.
- lockfile-pinned installs refuse digest drift from the pack index.

- [ ] **Step 3: Run install tests**

Run: `cd engine && cargo test -p wax-core install_language`
Expected: PASS

### - [ ] Task 9: CLI `wax language list|install|uninstall|update|doctor`

**Files:**
- Create: `engine/crates/wax-cli/Cargo.toml`
- Create: `engine/crates/wax-cli/src/main.rs`
- Create: `engine/crates/wax-cli/src/commands/language.rs`
- Create: `engine/crates/wax-cli/src/commands/init.rs`

- [ ] **Step 1: clap subcommand tree `language {list,install,uninstall,update,doctor}`**
- [ ] **Step 2: Wire install to registry + global state**
- [ ] **Step 3: `doctor` prints: enabled in `.waxrc`, installed version, lock pin, missing binary**

### - [ ] Task 10: `wax init` onboarding

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Create: `engine/fixtures/config/example.waxrc`

- [ ] **Step 1: Implement scriptable selection first**
- [ ] **Step 2: Write `.waxrc` and `wax.lock.json` after resolving selected pack artifacts**
- [ ] **Step 3: Call `language install` for selected ids**
- [ ] **Step 4: Optional registry scaffold** (copy example `registry.json` if missing)
- [ ] **Step 5: Defer interactive prompts until the non-TTY path is stable**

Implement `wax init --yes --language compose` before interactive prompts. The first version should be scriptable, deterministic, and easy to test:

```bash
wax init --yes --language compose
```

Expected:

- writes `.waxrc`
- installs selected packs unless `--no-install` is passed
- writes `wax.lock.json` with resolved pack versions and digests
- does not require a TTY

---

## Phase 4 — `wax scan` product path

### - [ ] Task 11a: Engine resolves enabled languages and spawns packs

**Files:**
- Modify: `engine/crates/wax-core/src/lib.rs`

- [ ] **Step 1: `Engine::scan_repo(repo_root)` loads `.waxrc`, filters `enabled: true`**
- [ ] **Step 2: For each id, resolve subprocess adapter from global manifest**
- [ ] **Step 3: Apply `auto_install::policy()` from Task 7b**
- [ ] **Step 4: Run enabled packs serially**

Run: `cd engine && cargo test -p wax-core scan_resolve`
Expected: PASS

### - [ ] Task 11b: Engine scan concurrency

**Files:**
- Modify: `engine/crates/wax-core/src/lib.rs`
- Test: `engine/crates/wax-core/tests/scan_concurrency.rs`

- [ ] **Step 1: Apply `engine.scan_concurrency` default and CLI override**
- [ ] **Step 2: Run enabled packs in parallel with bounded concurrency**
- [ ] **Step 3: Preserve deterministic merged output ordering by `LanguageId`**

Run: `cd engine && cargo test -p wax-core scan_concurrency`
Expected: PASS

### - [ ] Task 11c: Engine scan output writing

**Files:**
- Modify: `engine/crates/wax-core/src/lib.rs`
- Test: `engine/crates/wax-core/tests/scan_output.rs`

- [ ] **Step 1: Write per-language scan files under `.wax/out/languages/`**
- [ ] **Step 2: Write `MergedScan` to `.wax/out/scan-merged.json`**
- [ ] **Step 3: Use atomic writes so interrupted scans do not leave corrupt JSON**

Run: `cd engine && cargo test -p wax-core scan_output`
Expected: PASS

### - [ ] Task 12: Compose correctness gate (after `wax-lang-compose` exists)

**Files:**
- Test: `engine/crates/wax-lang-compose/tests/golden_small.rs`
- Data: committed golden JSON under `engine/crates/wax-lang-compose/tests/fixtures/` (do not depend on `prototypes/` paths)

- [ ] **Step 1: Add small Kotlin fixture + golden file in the compose crate**
- [ ] **Step 2: Assert usage_site_count and resolved_count**
- [ ] **Step 3: Document any intentional drift in spec**

### - [ ] Task 13: Create production `wax` binary target

**Files:**
- Modify: `engine/crates/wax-cli/Cargo.toml`
- Modify: `README.md`

- [ ] **Step 1: Ensure `[[bin]] name = "wax"`**
- [ ] **Step 2: Update docs to point at the production workspace, not `rust-prototype/`**

---

## Phase 5 — Distribution and docs (review, not full CI in one pass)

### - [ ] Task 14: ADR addendum for Rust foundation

**Files:**
- Create: `docs/adr/2026-05-16-rust-engine-language-packs.md`

- [ ] **Step 1: State decision to adopt Rust engine + language packs (pending spec approval)**
- [ ] **Step 2: Link Phase 0 evidence and decisions from spec**
- [ ] **Step 3: Explicitly defer kernel **plugins** to future ADR**

### - [ ] Task 15: Update component tracker design terminology

**Files:**
- Modify: `docs/specs/2026-05-13-component-tracker-design.md` (surgical edits)

- [ ] **Step 1: Replace “ecosystem plugin” (extractor sense) with **language pack****
- [ ] **Step 2: Add glossary note: **plugin** = future kernel hook**
- [ ] **Step 3: Point to [2026-05-16-language-packs-and-distribution.md](../specs/2026-05-16-language-packs-and-distribution.md)**

### - [ ] Task 16: Release sketch (document only)

**Files:**
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md` § Distribution

- [ ] **Step 1: Document cargo-dist or GitHub Actions matrix** (darwin-arm64, darwin-x64, linux-x64-gnu, linux-arm64-gnu)
- [ ] **Step 2: Separate artifacts: `wax`, `wax-lang-compose`, `wax-lang-react` per triple**
- [ ] **Step 3: Note npm wrapper as optional Phase 5b (not blocking v1)**

### - [ ] Task 17: Pack distribution threat model

**Files:**
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md` § Pack distribution trust model

- [ ] **Step 1: Record v1 decision (sha256 + HTTPS; Sigstore/cosign planned for v1.1)**
- [ ] **Step 2: Document lockfile vs auto-install precedence**
- [ ] **Step 3: Record Sigstore/cosign as the planned v1.1 signing direction**
- [ ] **Step 4: Add ADR addendum pointer in Task 14**

### - [ ] Task 18: Remove `rust-prototype/` reference workspace

**Files:**
- Delete: `rust-prototype/`
- Modify: `README.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`

- [ ] **Step 1: Confirm production replacement exists**

Before deleting the prototype, verify the production `engine/` workspace has landed and covers the useful reference material:

```bash
cd engine
cargo test -p wax-contract
cargo test -p wax-lang-api
```

Expected: PASS

- [ ] **Step 2: Remove `rust-prototype/`**

Delete the entire `rust-prototype/` directory after Tasks 1–4 have established the production contract/config/wire crates.

- [ ] **Step 3: Update references**

Remove README/spec links that point to `rust-prototype/`; replace them with `engine/` links where useful.

- [ ] **Step 4: Run cleanup checks**

```bash
rg -n "rust-prototype|Rust prototype" README.md docs engine
cd engine && cargo test
```

Expected: no stale production docs references to `rust-prototype`; tests PASS.

---

## Deferred (separate plans)

- Static site export (`wax export`)
- Swift language pack (parser spike required)
- WASM language packs
- Kernel **plugins** (export hooks, custom rules)
- Backend API and web UI from component tracker design
- npm meta-installer package

---

## Self-review (spec coverage)

| Spec requirement | Plan task |
|------------------|-----------|
| Terminology language vs plugin | Spec doc + Task 15 |
| `.waxrc` | Task 2, 10, 11a |
| `wax.lock.json` | Task 3, 10 |
| Global `~/.wax/langs/` | Task 7, 8a, 8b |
| Auto-install policy | Task 7b, 11a |
| Wire protocol (v1 JSON) | Task 4, 5, 6, 6b, 6c |
| No pack-to-pack IPC | Spec invariants; Task 11a engine-only merge |
| CLI install/update/doctor | Task 9 |
| Onboarding `wax init` | Task 10 |
| Prebuilt distribution | Task 16 |
| Compose + React first-party | Tasks 6, 6b, 12 |
| `ScanFacts` / `LanguageMetadata` | Task 1, production crates |
| Scan execution and output | Tasks 11a, 11b, 11c |
| Prototype cleanup | Task 18 |

## Review checklist for humans

Before implementation starts, confirm:

1. Decisions in [language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) are recorded: JSON-only `.waxrc`, required lockfile, Swift deferred, no fixed response cap, Sigstore/cosign v1.1 signing direction.
2. ADR process: addendum vs superseding foundation ADR.
3. Monorepo layout: start fresh in `engine/crates/`; keep `rust-prototype/` read-only as reference material.
4. CI policy: `wax scan --no-auto-install` + committed `wax.lock.json`.
5. Pack binary naming is fixed as `wax-lang-<id>` across crates, manifests, and release artifacts.
6. Task 15 intentionally carries old component-tracker “plugin” terminology cleanup; mention this in the PR description so reviewers do not confuse old design text with the new language-pack direction.

---

## Execution handoff

**Plan saved to:**

- Spec: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Plan: `docs/plans/2026-05-16-rust-engine-language-packs-plan.md`

**Two execution options:**

1. **Subagent-driven (recommended)** — one task per subagent, one PR per task, review between task PRs
2. **Inline** — execute one task at a time in-session, still committing and opening one PR per task

Which approach do you want after spec review?
