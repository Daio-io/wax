# Rust Engine and Language Packs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PR boundary:** Treat each checked **Task** as one implementation PR. Complete all steps inside a task, run its verification commands, commit the task, and open a PR before starting the next task. Phase checkpoints gate batches of task PRs; do not combine multiple tasks into one PR unless the human explicitly approves it.

**Goal:** Implement the production `wax` Rust engine with downloadable **language packs**, **`.waxrc`** configuration, global install lifecycle, and subprocess IPC—ready for review before broad foundation coding.

**Architecture:** A single **engine** orchestrates `scan`; each **language pack** is a downloaded native binary that returns normalized `ScanFacts` over **one JSON object per direction** on stdio (NDJSON multi-message deferred to daemon mode). Repo config enables languages; global `~/.wax/langs/` stores artifacts; `wax.lock.json` pins CI when used. **Plugins** (kernel hooks) are explicitly out of scope for this plan.

**Tech Stack:** Rust edition 2024, `wax-contract` / `wax-lang-api`, tree-sitter (Compose), SWC (React), serde JSON config, clap CLI, GitHub Releases + static registry manifest

**Spec (review first):** [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md)

---

## Decision rationale

Phase 0 compared TS-core and Go-core prototypes using source fixtures, golden outputs, and benchmark-oriented spikes. The provisional TS+TS direction had the lowest install friction, but it made the long-term multi-language boundary blurrier: every new ecosystem risked pulling parser/runtime concerns into the same package and making the analysis contract harder to keep stable.

This plan chooses a Rust engine with downloadable native language packs because it gives `wax` a small, deterministic kernel for scanning, merging, adoption metrics, and report output while keeping parser-heavy ecosystem work isolated behind a typed `ScanFacts` + stdio protocol boundary. Prebuilt `wax` and `wax-lang-*` artifacts preserve the “no local Rust toolchain” user experience, while `.waxrc`, `wax.lock.json`, and global pack installs give teams a path from easy local scans to reproducible CI.

## Prerequisites

- [ ] Spec [2026-05-16-language-packs-and-distribution.md](../specs/2026-05-16-language-packs-and-distribution.md) reviewed and open questions resolved (or defaults recorded in ADR addendum).
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

---

## Phase 1 — Contract, config, and wire protocol freeze

**Execution checkpoint:** Do not start Phase 2+ implementation until Tasks 1–4 land together and are reviewed. These tasks freeze the shared data contract (`ScanFacts`), repo/global config shape, lockfile semantics, and wire request/response envelope that every later task depends on.

### - [ ] Task 1: Freeze `ScanFacts` JSON schema

**Files:**
- Create: `engine/crates/wax-contract/Cargo.toml`
- Create: `engine/crates/wax-contract/src/lib.rs`
- Create: `engine/crates/wax-contract/schemas/scan-facts.schema.json`
- Test: `engine/crates/wax-contract/tests/schema_roundtrip.rs`

- [ ] **Step 1: Document field meanings in spec**

Ensure [language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) matches `LanguageMetadata` + `ScanFacts.language` (not `plugin`).

- [ ] **Step 2: Add serde roundtrip test**

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

- [ ] **Step 3: Run test**

Run: `cd engine && cargo test -p wax-contract`
Expected: PASS

- [ ] **Step 4: Commit** (when user requests commits)

```bash
git add engine/crates/wax-contract docs/specs/2026-05-16-language-packs-and-distribution.md
git commit -m "docs: freeze language pack scan facts contract"
```

### - [ ] Task 2: `.waxrc` parser and validation

**Files:**
- Create: `engine/crates/wax-core/Cargo.toml`
- Create: `engine/crates/wax-core/src/config.rs`
- Create: `engine/crates/wax-core/src/config/waxrc.rs`
- Test: `engine/crates/wax-core/tests/waxrc_load.rs`
- Fixture: `engine/fixtures/config/minimal.waxrc`

- [ ] **Step 1: Define Rust types**

```rust
#[derive(Debug, Deserialize)]
pub struct WaxRc {
    pub schema_version: u32,
    pub languages: Vec<LanguageEntry>,
}

#[derive(Debug, Deserialize)]
pub struct LanguageEntry {
    pub id: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

- [ ] **Step 2: Write failing test for minimal config**

```rust
#[test]
fn loads_minimal_waxrc() {
    let rc = load_waxrc("engine/fixtures/config/minimal.waxrc").unwrap();
    assert_eq!(rc.languages.len(), 1);
    assert_eq!(rc.languages[0].id, "compose");
}
```

- [ ] **Step 3: Implement `load_waxrc(path)` with clear errors**

Reject unknown `schema_version` with actionable message.

- [ ] **Step 4: Run test** — `cd engine && cargo test -p wax-core waxrc`

- [ ] **Step 5: Commit** (when requested)

### - [ ] Task 3: `wax.lock.json` parser

**Files:**
- Create: `engine/crates/wax-core/src/config/lockfile.rs`
- Test: `engine/crates/wax-core/tests/lockfile_load.rs`
- Fixture: `engine/fixtures/config/minimal.wax.lock.json`

- [ ] **Step 1: Types for lockfile** (`engine_api_version`, `languages: BTreeMap<String, LockedLanguage>`)
- [ ] **Step 2: Test load + version pin**
- [ ] **Step 3: `doctor` helper: compare `.waxrc` enabled ids vs lock keys**

### - [ ] Task 4: Wire protocol types (v1)

**Files:**
- Create: `engine/crates/wax-lang-api/Cargo.toml`
- Create: `engine/crates/wax-lang-api/src/protocol.rs`
- Create: `engine/crates/wax-lang-api/src/lib.rs`

- [ ] **Step 1: Align `WireScanRequest` with spec** (`repo_root`, `snapshot_id`, `config` — no `mode` in v1)
- [ ] **Step 2: `WireScanResponse` — untagged `ScanFacts` success vs `type: "error"` failure**
- [ ] **Step 3: Unit test roundtrip request JSON matches spec example**

- [ ] **Step 4: Review checkpoint**

Confirm Tasks 1–4 are consistent with each other before starting Phase 2+ work:

- `ScanFacts.schema_version` is enforced by `scan_facts_from_json`.
- `.waxrc` uses `design_system_registry` and keeps per-language config opaque to the engine.
- `wax.lock.json` records `api_version`, `source`, `resolved.target`, `resolved.url`, `resolved.sha256`, `wax_version`, and `locked_at`.
- `WireScanRequest` contains `type`, `api_version`, `language_id`, `repo_root`, `snapshot_id`, and `config`; it does not contain `mode`.
- `WireScanResponse` supports bare `ScanFacts` success and structured `type: "error"` failure.

---

## Phase 2 — Subprocess adapter and first pack entrypoints

Build on the frozen Phase 1 contracts. This phase proves that the engine can invoke an external language-pack binary and that first-party packs can speak the v1 stdio protocol.

### - [ ] Task 5: Subprocess `LanguageExtractor` implementation

**Files:**
- Create: `engine/crates/wax-core/src/subprocess_lang.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`

- [ ] **Step 1: Spawn `manifest.command`, write one `WireScanRequest::Scan` JSON to stdin, read stdout**
- [ ] **Step 2: Parse `WireScanResponse` or `ScanFacts`; map timeout/cancel to `LanguageError::Timeout` / `Cancelled`**
- [ ] **Step 3: Integration test with mock binary** (shell script that echoes canned JSON)

Run: `cd engine && cargo test -p wax-core subprocess`

### - [ ] Task 6: `wax-lang-compose` stdio entrypoint

**Files:**
- Create: `engine/crates/wax-lang-compose/Cargo.toml` with `[[bin]] name = "wax-lang-compose"`
- Create: `engine/crates/wax-lang-compose/src/lib.rs`
- Create: `engine/crates/wax-lang-compose/src/bin/wax-lang-compose.rs`

- [ ] **Step 1: Read stdin lines until `Scan` message**
- [ ] **Step 2: Call `ComposeLanguage::scan`, write `ScanFacts` as one line to stdout**
- [ ] **Step 3: Manual test**

```bash
cd engine
cargo build -p wax-lang-compose
echo '{"type":"scan","api_version":1,...}' | ./target/debug/wax-lang-compose --stdio
```

### - [ ] Task 6b: `wax-lang-react` stdio entrypoint skeleton

**Files:**
- Create: `engine/crates/wax-lang-react/Cargo.toml`
- Create: `engine/crates/wax-lang-react/src/lib.rs`
- Create: `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs`
- Modify: `engine/Cargo.toml`

- [ ] **Step 1: Add a crate skeleton**

Add `wax-lang-react` to the workspace with dependencies on `wax-contract` and `wax-lang-api`.

- [ ] **Step 2: Implement a stub `ReactLanguage`**

Return `ScanFacts` with:

- `language.id = "react"`
- `status = ScanStatus::Partial`
- empty components and usage sites
- a diagnostic explaining React extraction is scaffolded but not implemented

- [ ] **Step 3: Add `wax-lang-react --stdio`**

Read one `WireScanRequest::Scan` JSON object from stdin, call the stub language, and write one JSON object response to stdout.

- [ ] **Step 4: Run a manual stdio smoke test**

```bash
cd engine
cargo build -p wax-lang-react
echo '{"type":"scan","api_version":1,"language_id":"react","repo_root":"/tmp/repo","snapshot_id":"test","config":{}}' \
  | ./target/debug/wax-lang-react --stdio
```

Expected: one valid `ScanFacts` JSON object with `language.id = "react"` and `snapshot_id = "test"`.

### - [ ] Task 6c: Protocol conformance tests

**Files:**
- Test: `engine/crates/wax-lang-api/tests/wire_protocol.rs`
- Test: `engine/crates/wax-core/tests/subprocess_protocol.rs`

- [ ] **Step 1: Add wire request fixture test**

Test that the spec request example deserializes into `WireScanRequest::Scan` and serializes back with the same field names (`repo_root`, not `fixture_root`; no `mode`).

- [ ] **Step 2: Add wire error fixture test**

Test that the spec error response deserializes into `WireScanResponse::Error` with `code = "registry_not_found"` and an empty diagnostics array.

- [ ] **Step 3: Add subprocess adapter conformance test**

Use the mock binary from Task 5 to assert:

- success stdout is parsed through `scan_facts_from_json`
- structured `type: "error"` stdout maps to a pack failure
- oversized stdout triggers `response_too_large`
- timeout maps to `LanguageError::Timeout`

- [ ] **Step 4: Run protocol tests**

Run: `cd engine && cargo test -p wax-lang-api wire_protocol && cargo test -p wax-core subprocess_protocol`
Expected: PASS

---

## Phase 3 — Global install and registry

### - [ ] Task 7: Global paths and state

**Files:**
- Create: `engine/crates/wax-core/src/paths.rs`
- Create: `engine/crates/wax-core/src/global_state.rs`

- [ ] **Step 1: `wax_home() -> ~/.wax` with `WAX_HOME` override**
- [ ] **Step 2: `lang_install_dir(id, version) -> ~/.wax/langs/<id>/<version>`**
- [ ] **Step 3: Load/save `state.json`**

### - [ ] Task 8: Official registry client (read-only v1)

**Files:**
- Create: `engine/crates/wax-core/src/registry.rs`
- Fixture: `engine/fixtures/registry/official-manifest.json`

- [ ] **Step 1: Parse manifest entry** (id, version, api_version, targets map with url + sha256)
- [ ] **Step 2: `install_language(id, version, target_triple)` — download, verify sha256, unpack, write manifest.json**
- [ ] **Step 3: Test with `file://` fixture URL** (no network in unit tests)
- [ ] **Step 4: Harden install edge cases**

Add tests that cover:

- sha mismatch refuses install and leaves no active manifest.
- archive entries cannot write outside the install temp dir (`../` path traversal).
- partial installs are written to a temp dir and atomically promoted only after verification.
- installed binaries are executable on Unix.
- lockfile-pinned installs refuse digest drift from the pack index.

### - [ ] Task 9: CLI `wax language install|list|uninstall|update|doctor`

**Files:**
- Create: `engine/crates/wax-cli/Cargo.toml`
- Create: `engine/crates/wax-cli/src/main.rs`
- Create: `engine/crates/wax-cli/src/commands/language.rs`
- Create: `engine/crates/wax-cli/src/commands/init.rs`

- [ ] **Step 1: clap subcommand tree `language {install,list,uninstall,update,doctor}`**
- [ ] **Step 2: Wire install to registry + global state**
- [ ] **Step 3: `doctor` prints: enabled in `.waxrc`, installed version, lock pin, missing binary**

### - [ ] Task 10: `wax init` onboarding

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Create: `engine/fixtures/config/example.waxrc`

- [ ] **Step 1: Interactive prompts (or `--yes` defaults): select language ids**
- [ ] **Step 2: Write `.waxrc`; write `wax.lock.json` only for `--lock` / CI template mode**
- [ ] **Step 3: Call `language install` for selected ids**
- [ ] **Step 4: Optional registry scaffold** (copy example `registry.json` if missing)
- [ ] **Step 5: Keep v1 onboarding boring**

Implement `wax init --yes --language compose` before interactive prompts. The first version should be scriptable, deterministic, and easy to test:

```bash
wax init --yes --language compose --lock
```

Expected:

- writes `.waxrc`
- installs selected packs unless `--no-install` is passed
- writes `wax.lock.json` only when `--lock` is present
- does not require a TTY

---

## Phase 4 — `wax scan` product path

### - [ ] Task 11: Engine resolves enabled languages from `.waxrc`

**Files:**
- Modify: `engine/crates/wax-core/src/lib.rs`

- [ ] **Step 1: `Engine::scan_repo(repo_root)` loads `.waxrc`, filters `enabled: true`**
- [ ] **Step 2: For each id, resolve subprocess adapter from global manifest**
- [ ] **Step 3: Auto-install if missing** (unless `--no-auto-install`)
- [ ] **Step 4: Parallel scan per spec `engine.scan_concurrency` (default 2)**
- [ ] **Step 5: Write `MergedScan` to `.wax/out/scan-merged.json` and per-language files**

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
- [ ] **Step 2: Link Phase 0 evidence and open questions from spec**
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

- [ ] **Step 1: Record v1 decision (sha256 + HTTPS; signing deferred)**
- [ ] **Step 2: Document lockfile vs auto-install precedence**
- [ ] **Step 3: Add ADR addendum pointer in Task 14**

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
| `.waxrc` | Task 2, 10, 11 |
| `wax.lock.json` | Task 3, 10 |
| Global `~/.wax/langs/` | Task 7, 8 |
| Wire protocol (v1 JSON) | Task 4, 5, 6, 6b, 6c |
| No pack-to-pack IPC | Spec invariants; Task 11 engine-only merge |
| CLI install/update/doctor | Task 9 |
| Onboarding `wax init` | Task 10 |
| Prebuilt distribution | Task 16 |
| Compose + React first-party | Tasks 6, 6b, 12 |
| `ScanFacts` / `LanguageMetadata` | Task 1, production crates |

## Review checklist for humans

Before implementation starts, confirm:

1. Open questions in [language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) (JSON vs YAML, Swift parser, response cap, signing v1.1).
2. ADR process: addendum vs superseding foundation ADR.
3. Monorepo layout: start fresh in `engine/crates/`; keep `rust-prototype/` read-only as reference material.
4. CI policy: `wax scan --no-auto-install` + committed `wax.lock.json` (see spec: lockfile required for that CI mode).
5. Pack binary naming is fixed as `wax-lang-<id>` across crates, manifests, and release artifacts.

---

## Execution handoff

**Plan saved to:**

- Spec: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Plan: `docs/plans/2026-05-16-rust-engine-language-packs-plan.md`

**Two execution options:**

1. **Subagent-driven (recommended)** — one task per subagent, one PR per task, review between task PRs
2. **Inline** — execute one task at a time in-session, still committing and opening one PR per task

Which approach do you want after spec review?
