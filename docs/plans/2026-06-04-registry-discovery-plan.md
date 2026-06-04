# Registry Discovery and Skill-Assisted Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic design-system registry discovery plus a proper Agent Skill workflow for AI-assisted registry review and sync.

**Architecture:** `wax-cli` exposes `wax registry discover`; `wax-core` owns command orchestration, root resolution, registry JSON generation, and safe writes; language-specific discovery starts with Compose and emits deterministic component symbols. Registry discovery is an authoring-time exception to the current subprocess language-pack scan path: implementation may call in-process discovery code from `wax-lang-compose` for v1, but scan and validate continue using the distributed subprocess protocol. A separate `wax-registry-sync` skill wraps the CLI with review, diffing, validation, and lock refresh guidance without making AI part of scan or validate runtime.

**Tech Stack:** Rust 2024, clap, serde JSON, existing `wax-core`, `wax-cli`, and `wax-lang-compose` crates, tree-sitter-backed Compose source inspection, Agent Skill `SKILL.md`.

---

## Reference Spec

- Design spec: `docs/plans/2026-06-04-registry-discovery-design.md`
- Roadmap source: `docs/plans/README.md`
- Existing registry layout spec: `docs/specs/2026-06-02-registry-sources-and-wax-layout-design.md`
- Product scope spec: `docs/specs/2026-05-13-component-tracker-design.md`

## File Structure

- Modify `engine/crates/wax-cli/src/main.rs`
  - Add the `registry` command group and `discover` subcommand wiring.
- Create `engine/crates/wax-cli/src/commands/registry.rs`
  - Parse `wax registry discover` flags.
  - Call `wax-core` discovery orchestration.
  - Keep stdout JSON-clean for `--dry-run`.
- Create `engine/crates/wax-core/src/registry_discovery.rs`
  - Resolve roots from `--root` or Wax config.
  - Generate schema v1 registry JSON from discovered symbols.
  - Refuse overwrites unless `force` is true.
  - Write registry files atomically.
- Modify `engine/crates/wax-core/src/lib.rs`
  - Export registry discovery types and functions.
- Create `engine/crates/wax-lang-compose/src/discover.rs`
  - Find likely public top-level Compose component symbols under supplied roots.
- Modify `engine/crates/wax-lang-compose/src/lib.rs`
  - Export Compose discovery.
- Create `engine/crates/wax-core/tests/registry_discovery.rs`
  - Test root resolution, dry-run generation, overwrite refusal, and forced replacement.
- Create `engine/crates/wax-cli/tests/registry_discover_command.rs`
  - Test user-facing CLI behavior and stdout/stderr contracts.
- Create `engine/crates/wax-lang-compose/tests/registry_discover.rs`
  - Test Compose symbol extraction from fixtures.
- Create `engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/Components.kt`
  - Fixture with public, private, internal, duplicate, and helper composables.
- Create `engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/DuplicateComponents.kt`
  - Fixture with a duplicate public symbol under another source file.
- Create `.agents/skills/wax-registry-sync/SKILL.md`
  - Project-scoped Agent Skill for AI-assisted registry review and sync.
- Modify `README.md`
  - Add registry discovery quick-start and skill-assisted sync mention when implementation tasks land.
- Modify `docs/plans/README.md`
  - Keep registry discovery marked as the order 4 active plan.

## Phase 1 - Deterministic Compose Discovery

### - [x] Task 1: Add Compose symbol discovery

**Files:**
- Create: `engine/crates/wax-lang-compose/src/discover.rs`
- Modify: `engine/crates/wax-lang-compose/src/lib.rs`
- Create: `engine/crates/wax-lang-compose/tests/registry_discover.rs`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/Components.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/DuplicateComponents.kt`

- [x] **Step 1: Add fixture source for likely DS components**

Create `engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/Components.kt` with public composables, skipped helpers, and non-public composables:

```kotlin
package com.example.ds

import androidx.compose.runtime.Composable

@Composable
fun PrimaryButton() {}

@Composable
public fun SecondaryButton() {}

@Composable
internal fun InternalButton() {}

@Composable
private fun PrivateButton() {}

@Composable
fun helperText() {}

fun NotComposable() {}
```

Create `engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/DuplicateComponents.kt` with a duplicate symbol to verify stable de-duplication:

```kotlin
package com.example.ds.duplicates

import androidx.compose.runtime.Composable

@Composable
fun PrimaryButton() {}
```

- [x] **Step 2: Write failing discovery tests**

Create `engine/crates/wax-lang-compose/tests/registry_discover.rs`:

```rust
use std::path::PathBuf;
use wax_lang_compose::discover::discover_registry_symbols;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover/design-system/src/main/kotlin")
}

#[test]
fn discovers_public_top_level_composables() {
    let symbols = discover_registry_symbols(&[fixture_root()]).expect("discover symbols");

    assert_eq!(symbols, vec!["PrimaryButton", "SecondaryButton"]);
}

#[test]
fn missing_root_fails() {
    let missing = fixture_root().join("missing");

    let err = discover_registry_symbols(&[missing]).expect_err("missing root should fail");

    assert!(err.to_string().contains("discovery root does not exist"));
}
```

- [x] **Step 3: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-lang-compose --test registry_discover
```

Expected: FAIL with unresolved module or function `wax_lang_compose::discover`.

- [x] **Step 4: Implement minimal Compose discovery**

Create `engine/crates/wax-lang-compose/src/discover.rs` with deterministic tree-sitter-backed scanning for `.kt` files. It must return sorted unique symbols and skip `private`, `internal`, and lowercase helper names.

Modify `engine/crates/wax-lang-compose/src/lib.rs`:

```rust
pub mod discover;
```

- [x] **Step 5: Run focused Compose tests**

Run:

```bash
cd engine
cargo test -p wax-lang-compose --test registry_discover
```

Expected: PASS.

- [x] **Step 6: Commit Task 1**

```bash
git add engine/crates/wax-lang-compose/src/discover.rs \
  engine/crates/wax-lang-compose/src/lib.rs \
  engine/crates/wax-lang-compose/tests/registry_discover.rs \
  engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/Components.kt \
  engine/crates/wax-lang-compose/tests/fixtures/discover/design-system/src/main/kotlin/DuplicateComponents.kt
git commit -m "feat: discover compose registry symbols"
```

### - [x] Task 2: Add core registry discovery orchestration

**Architecture note:** Normal scan execution still uses installed language packs through the subprocess protocol in `engine/crates/wax-core/src/subprocess_lang.rs`. Registry discovery is authoring-time source inspection, so v1 may call `wax-lang-compose` discovery code in process to avoid inventing a new wire protocol before multiple language packs need it. If future language packs need out-of-process registry discovery, add an explicit discovery request to the language-pack protocol in a later plan rather than overloading scan requests.

**Files:**
- Create: `engine/crates/wax-core/src/registry_discovery.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Modify: `engine/crates/wax-core/Cargo.toml` if a dependency is needed for atomic temp writes
- Create: `engine/crates/wax-core/tests/registry_discovery.rs`

- [x] **Step 1: Write failing core tests for dry-run generation and writes**

Create `engine/crates/wax-core/tests/registry_discovery.rs` with tests for:

- generated registry JSON contains `schema_version: 1`
- generated ids use `ds.<kebab-case-symbol>`
- output components are sorted
- duplicate symbols collapse to one component
- default writes target `.wax/wax.registry.json`
- existing registry refuses overwrite
- `force` replaces an existing registry

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-core --test registry_discovery
```

Expected: FAIL with unresolved registry discovery types.

- [x] **Step 3: Implement core types and registry JSON generation**

Create `RegistryDiscoverOptions`, `RegistryDiscoverResult`, and a function such as `discover_registry(options)`. Keep the public API small:

```rust
pub struct RegistryDiscoverOptions<'a> {
    pub repo_root: &'a Path,
    pub language_id: &'a str,
    pub roots: Vec<PathBuf>,
    pub dry_run: bool,
    pub force: bool,
}
```

The implementation should generate a serde JSON value or typed struct for:

```json
{
  "schema_version": 1,
  "components": []
}
```

- [x] **Step 4: Implement safe write behavior**

Write `.wax/wax.registry.json` only when `dry_run` is false. If the file exists and `force` is false, return an error that includes `--force` and `--dry-run`. Ensure the parent `.wax/` directory is created before writing.

- [x] **Step 5: Run focused core tests**

Run:

```bash
cd engine
cargo test -p wax-core --test registry_discovery
```

Expected: PASS.

- [x] **Step 6: Commit Task 2**

```bash
git add engine/crates/wax-core/src/registry_discovery.rs \
  engine/crates/wax-core/src/lib.rs \
  engine/crates/wax-core/Cargo.toml \
  engine/crates/wax-core/tests/registry_discovery.rs
git commit -m "feat: add registry discovery orchestration"
```

### - [x] Task 3: Wire `wax registry discover`

**Files:**
- Modify: `engine/crates/wax-cli/src/main.rs`
- Create: `engine/crates/wax-cli/src/commands/registry.rs`
- Create: `engine/crates/wax-cli/tests/registry_discover_command.rs`

- [x] **Step 1: Write failing CLI tests**

Add tests for:

- `wax registry discover --language compose --root <fixture> --dry-run` prints valid JSON to stdout.
- `--dry-run` writes summaries and warnings to stderr, not stdout.
- default write creates `.wax/wax.registry.json`.
- a second write fails without `--force`.
- `--force` replaces the registry.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cd engine
cargo test -p wax-cli --test registry_discover_command
```

Expected: FAIL because the `registry` command does not exist.

- [x] **Step 3: Add clap command wiring**

Add a `registry` command group with a `discover` subcommand:

```text
wax registry discover --language <id> [--root <path>...] [--dry-run] [--force]
```

`--root` should be repeatable so users can target multiple DS source roots explicitly.

- [x] **Step 4: Preserve stdout and stderr contracts**

For `--dry-run`, print only registry JSON to stdout. Print summary, warnings, and skipped counts to stderr. For write mode, print human summary to stdout or stderr following existing CLI conventions.

Write mode should not print JSON to stdout. It should print a concise human summary, including the output path, false-positive warning, and next-step commands. If existing CLI conventions are ambiguous, use stdout for successful summaries and stderr for warnings/errors.

- [x] **Step 5: Run focused CLI tests**

Run:

```bash
cd engine
cargo test -p wax-cli --test registry_discover_command
```

Expected: PASS.

- [x] **Step 6: Commit Task 3**

```bash
git add engine/crates/wax-cli/src/main.rs \
  engine/crates/wax-cli/src/commands/registry.rs \
  engine/crates/wax-cli/tests/registry_discover_command.rs
git commit -m "feat: add registry discover command"
```

## Phase 2 - Root Resolution and Validation

### - [ ] Task 4: Resolve roots from Wax config when `--root` is omitted

**Files:**
- Modify: `engine/crates/wax-core/src/registry_discovery.rs`
- Modify: `engine/crates/wax-core/tests/registry_discovery.rs`
- Modify: `engine/crates/wax-cli/tests/registry_discover_command.rs`

- [ ] **Step 1: Add failing tests for config roots**

Test that a repo with `.wax/wax.config.json` and enabled `compose` roots can run:

```bash
wax registry discover --language compose --dry-run
```

without passing `--root`.

- [ ] **Step 2: Add failing test for missing roots**

Test that missing config roots fail with an error containing:

```text
pass --root path/to/design-system
```

- [ ] **Step 3: Implement root resolution**

When `roots` is empty, load repo files with existing config discovery helpers, find the enabled language matching `language_id`, and use its `roots` array. Validate each resolved root stays within the repo and exists. Because config roots are scan targets, emit a warning that `--root path/to/design-system` is preferred when the configured roots point at app code.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cd engine
cargo test -p wax-core --test registry_discovery
cargo test -p wax-cli --test registry_discover_command
```

Expected: PASS.

- [ ] **Step 5: Commit Task 4**

```bash
git add engine/crates/wax-core/src/registry_discovery.rs \
  engine/crates/wax-core/tests/registry_discovery.rs \
  engine/crates/wax-cli/tests/registry_discover_command.rs
git commit -m "feat: resolve registry discovery roots from config"
```

### - [ ] Task 5: Add validation and lock refresh guidance

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/registry.rs`
- Modify: `engine/crates/wax-cli/tests/registry_discover_command.rs`
- Modify: `README.md`

- [ ] **Step 1: Add failing test for post-write guidance**

Test that successful write output includes:

```text
Review before committing
wax validate
wax language update
```

- [ ] **Step 2: Implement guidance text**

After writing the registry, print concise next steps. Do not automatically run `wax validate` in v1 unless the implementation plan is revised to support it safely; keep generation fast and predictable.

- [ ] **Step 3: Document command usage**

Update `README.md` with:

```bash
wax registry discover --language compose --dry-run
wax registry discover --language compose
wax language update
wax validate
```

- [ ] **Step 4: Run focused CLI tests**

Run:

```bash
cd engine
cargo test -p wax-cli --test registry_discover_command
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

```bash
git add engine/crates/wax-cli/src/commands/registry.rs \
  engine/crates/wax-cli/tests/registry_discover_command.rs \
  README.md
git commit -m "docs: document registry discovery workflow"
```

## Phase 3 - Agent Skill

### - [ ] Task 6: Add `wax-registry-sync` project skill

**Files:**
- Create: `.agents/skills/wax-registry-sync/SKILL.md`
- Create: `.agents/skills/wax-registry-sync/fixtures/README.md` if fixture notes are useful
- Modify: `README.md`

- [ ] **Step 1: Create the skill file**

Create `.agents/skills/wax-registry-sync/SKILL.md` with YAML frontmatter:

```markdown
---
name: wax-registry-sync
description: Use when updating Wax design-system registries from source packages; runs deterministic registry discovery, reviews candidates, asks about ambiguous exports, writes .wax/wax.registry.json, validates config, and refreshes locks.
---
```

The skill body must instruct agents to:

- inspect `.wax/wax.config.json` or `.waxrc`
- run `wax registry discover --language <id> --dry-run`
- compare with existing `.wax/wax.registry.json`
- ask before using `--force`
- run `wax validate`
- run `wax language update` when registry locks are stale

- [ ] **Step 2: Add a skill workflow smoke review**

Manually inspect the skill for these exact guardrails:

```text
dry-run before write
do not blindly overwrite
show diff or summary before --force
validate after write
refresh locks
```

- [ ] **Step 3: Document installation and use**

Update `README.md` with a short AI-assisted section and a skills ecosystem install example. If the skill is project-local only at first, say so clearly and avoid implying it is already published on skills.sh.

- [ ] **Step 4: Commit Task 6**

```bash
git add .agents/skills/wax-registry-sync/SKILL.md README.md
git commit -m "docs: add wax registry sync skill"
```

## Phase 4 - Full Verification

### - [ ] Task 7: Run full engine verification and update plan checkboxes

**Files:**
- Modify: `docs/plans/2026-06-04-registry-discovery-plan.md`

- [ ] **Step 1: Run formatting**

Run:

```bash
cd engine
cargo fmt --all --check
```

Expected: PASS.

- [ ] **Step 2: Run workspace tests**

Run:

```bash
cd engine
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Run clippy**

Run:

```bash
cd engine
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Tick completed plan checkboxes**

Update this plan so every completed task and step is checked in the same PR that implements it.

- [ ] **Step 5: Commit verification updates**

```bash
git add docs/plans/2026-06-04-registry-discovery-plan.md
git commit -m "chore: complete registry discovery plan"
```
