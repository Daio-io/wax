# Registry Sync and Config v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the alpha registry/config flow with config v2, remembered design systems, no-config local scans, and explicit app sync.

**Architecture:** `wax-core` owns config v2 parsing, registry source resolution, lockfile updates, and global design-system memory. `wax-cli` owns prompt flows and command orchestration for registry discovery, init, scan, sync, and registry memory management. Language packs continue to receive a resolved repo-relative registry path and do not fetch remote sources.

**Tech Stack:** Rust workspace under `engine/`, clap, dialoguer, serde/serde_json, existing wax lockfile and registry source helpers.

## Global Constraints

- No compatibility path for `.waxrc`, top-level `wax.lock.json`, or `design_system_registry`.
- Only `.wax/wax.config.json` is supported for repo config.
- Only `.wax/wax.lock.json` is supported for repo locks.
- Repo config uses `schema_version: 2`.
- `languages` is an object keyed by language id.
- `enabled` is removed; a language key exists only when enabled.
- `design_systems` is the defining section for design-system configs; app-only configs omit it unless they also publish registries.
- Global state remembers design-system locations but does not store registry JSON snapshots.
- `wax scan` attempts best-effort sync when upstream metadata exists; sync failures warn and scan continues with the current configured registry source.
- Ephemeral no-config scans must not write config, lockfile, or committed registry snapshots under `.wax/registries/`.
- Ephemeral no-config scans may use `.wax/cache/` for materialized registry inputs and `.wax/out/` for scan output.
- User-facing language-pack index naming should use `pack index`, not design-system `registry`.

---

## File Structure

- `engine/crates/wax-core/src/config/waxrc.rs`: replace v1 array parsing with v2 object parsing and design-system publication parsing.
- `engine/crates/wax-core/src/config/repo_files.rs`: remove legacy file discovery and expose only `.wax/wax.config.json` and `.wax/wax.lock.json`.
- `engine/crates/wax-core/src/global_state.rs`: add remembered design-system entries.
- `engine/crates/wax-core/src/registry_source.rs`: keep source resolution, update callers for v2 config fields.
- `engine/crates/wax-core/src/registry_memory.rs`: new helper for reading, writing, and resolving remembered design systems.
- `engine/crates/wax-core/src/sync.rs`: new app sync orchestration that updates registry sources/copies and refreshes lock entries.
- `engine/crates/wax-cli/src/commands/init.rs`: update init prompts and writes for config v2 and remembered registries.
- `engine/crates/wax-cli/src/commands/registry.rs`: extend discover and add list/show/update/delete memory commands.
- `engine/crates/wax-cli/src/commands/scan.rs`: add no-config ephemeral TTY scan path and best-effort sync preflight.
- `engine/crates/wax-cli/src/commands/sync.rs`: new `wax sync` command wrapper.
- `engine/crates/wax-cli/src/main.rs`: add `Sync`, registry subcommands, and `--pack-index` flags.
- `engine/crates/wax-contract/schemas/waxrc.schema.json`: replace config schema with v2.
- `README.md` and relevant docs/specs: update examples and command names.

## Task 1: Config v2 and Lockfile Cutover

**Files:**

- Modify: `engine/crates/wax-core/src/config/waxrc.rs`
- Modify: `engine/crates/wax-core/src/config/repo_files.rs`
- Modify: `engine/crates/wax-core/src/config/lockfile.rs`
- Modify: `engine/crates/wax-contract/schemas/waxrc.schema.json`
- Modify tests under `engine/crates/wax-cli/tests/validate_command.rs` and `engine/crates/wax-core/src/**`

**Interfaces:**

- Produces: `WaxRc { schema_version, engine, adoption, languages, design_systems }`
- Produces: `LanguageEntry { id, roots, registry_source, extra }` derived from the v2 language map key
- Produces: `DesignSystemConfig { name, registries }`
- Consumes later: config loaders return only `.wax/wax.config.json` and `.wax/wax.lock.json`

- [ ] **Step 1: Add failing config v2 parser tests**

Add tests that parse this shape:

```json
{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/acme/react.json",
        "upstream": "acme/react"
      }
    }
  },
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json",
          "published_source": "https://cdn.example.com/acme/react.registry.json"
        }
      }
    }
  }
}
```

Run:

```bash
cd engine
cargo test -p wax-core config_v2
```

Expected: FAIL because v2 parsing is not implemented.

- [ ] **Step 2: Implement config v2 structs and parsing**

Replace v1 language-array parsing with an object keyed by `LanguageId`. Keep pack-specific extra config flattened inside each language entry. Preserve existing `engine` and `adoption` defaults.

Run:

```bash
cd engine
cargo test -p wax-core config_v2
```

Expected: PASS.

- [ ] **Step 3: Remove legacy repo file discovery**

Change repo file discovery so commands only select:

```text
.wax/wax.config.json
.wax/wax.lock.json
```

Remove warning variants for ignored `.waxrc` and top-level `wax.lock.json`.

Run:

```bash
cd engine
cargo test -p wax-core repo_files
```

Expected: PASS with only preferred file paths.

- [ ] **Step 4: Remove `design_system_registry` support**

Remove config parsing, schema support, validation warnings, and language-pack compatibility for `design_system_registry`. Engine-rewritten pack config should use only `registry`.

Run:

```bash
cd engine
cargo test -p wax-core validate
cargo test -p wax-lang-basic registry
cargo test -p wax-lang-compose registry
cargo test -p wax-lang-react registry
cargo test -p wax-lang-swift registry
```

Expected: PASS after fixtures use `registry`.

- [ ] **Step 5: Update schema and example config**

Update `engine/crates/wax-contract/schemas/waxrc.schema.json` and `engine/fixtures/config/example.waxrc` to schema v2. Keep the fixture filename for now only if renaming causes broad unrelated test churn; otherwise rename it in this task.

Run:

```bash
cd engine
cargo test -p wax-cli init_command validate_command
```

Expected: PASS with v2 config snapshots.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/wax-core engine/crates/wax-contract engine/crates/wax-cli engine/crates/wax-lang-basic engine/crates/wax-lang-compose engine/crates/wax-lang-react engine/crates/wax-lang-swift engine/fixtures
git commit -m "feat: cut over to wax config v2"
```

## Task 2: Remembered Design Systems and Registry Discover

**Files:**

- Modify: `engine/crates/wax-core/src/global_state.rs`
- Create: `engine/crates/wax-core/src/registry_memory.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Modify: `engine/crates/wax-cli/src/commands/registry.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`
- Add or update registry command tests

**Interfaces:**

- Produces: `RememberedDesignSystem { name: String, repo_root: PathBuf, last_seen_config: PathBuf }`
- Produces: `remember_design_system(state_path, id, name, repo_root) -> Result<(), RegistryMemoryError>`
- Produces: registry subcommands `list`, `show <id>`, `update <id> --repo-root <path>`, `delete <id>`
- Consumes: Task 1 config v2 `design_systems` shape

- [ ] **Step 1: Add failing global state tests**

Add tests for loading and saving:

```json
{
  "installed_languages": {},
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "repo_root": "/tmp/acme-ds",
      "last_seen_config": ".wax/wax.config.json"
    }
  }
}
```

Run:

```bash
cd engine
cargo test -p wax-core global_state_design_systems
```

Expected: FAIL because the field does not exist.

- [ ] **Step 2: Extend global state**

Add `design_systems` with `#[serde(default)]`, validated design-system ids, and path data. Keep existing installed-language behavior unchanged.

Run:

```bash
cd engine
cargo test -p wax-core global_state_design_systems
```

Expected: PASS.

- [ ] **Step 3: Add registry memory helper**

Create `registry_memory.rs` to centralize list/show/update/delete behavior and validation. The helper should not copy registry JSON.

Run:

```bash
cd engine
cargo test -p wax-core registry_memory
```

Expected: PASS.

- [ ] **Step 4: Extend `wax registry discover`**

Add flags:

```text
--design-system <id>
--name <display-name>
```

When present, discovery writes `.wax/registries/<language>.json`, ensures `design_systems.<id>.registries.<language>.source`, and remembers the design system globally.

Run:

```bash
cd engine
cargo test -p wax-cli registry_discover_design_system
```

Expected: PASS.

- [ ] **Step 5: Add memory management commands**

Implement:

```bash
wax registry list
wax registry show acme
wax registry update acme --repo-root ../acme-ds
wax registry delete acme
```

Run:

```bash
cd engine
cargo test -p wax-cli registry_memory_commands
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/wax-core engine/crates/wax-cli
git commit -m "feat: remember discovered design systems"
```

## Task 3: Init and Ephemeral No-Config Scan

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`
- Add or update `engine/crates/wax-cli/tests/init_interactive.rs`
- Add `engine/crates/wax-cli/tests/scan_ephemeral.rs`

**Interfaces:**

- Consumes: Task 2 registry memory list and resolve helpers
- Produces: init-selected app config with `registry.source` and `registry.upstream`
- Produces: scan-only ephemeral selections that do not write repo files

- [ ] **Step 1: Add failing `wax init` tests for remembered registry selection**

Test that interactive selections can produce:

```json
{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/acme/react.json",
        "upstream": "acme/react"
      }
    }
  }
}
```

Run:

```bash
cd engine
cargo test -p wax-cli --test init_interactive
```

Expected: FAIL because init still writes the old language array and old registry paths.

- [ ] **Step 2: Update init writes**

Use remembered design systems in the prompt flow. Copy local DS registry sources into `.wax/registries/<design-system>/<language>.json` unless the DS declares `published_source`.

Run:

```bash
cd engine
cargo test -p wax-cli --test init_interactive
cargo test -p wax-cli --test init_command
```

Expected: PASS.

- [ ] **Step 3: Add failing no-config scan tests**

Test that TTY-mode scan selections run without writing:

```text
.wax/wax.config.json
.wax/wax.lock.json
.wax/registries/
```

The test may allow `.wax/cache/` and `.wax/out/`.

Test that non-TTY no-config scan fails and suggests `wax init`.

Run:

```bash
cd engine
cargo test -p wax-cli --test scan_ephemeral
```

Expected: FAIL because scan currently requires repo config.

- [ ] **Step 4: Implement ephemeral scan selections**

Refactor scan orchestration enough to accept in-memory language selections when no config exists and stdin is a TTY. Do not write repo files.

Run:

```bash
cd engine
cargo test -p wax-cli --test scan_ephemeral
```

Expected: PASS.

- [ ] **Step 5: Rename pack-index flags**

Rename user-facing `--registry` pack-index flags to `--pack-index` and environment variable `WAX_LANG_INDEX` to `WAX_PACK_INDEX`. Update help text and tests.

Run:

```bash
cd engine
cargo test -p wax-cli language
```

Expected: PASS with updated flag names.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/wax-cli engine/crates/wax-core engine/fixtures
git commit -m "feat: support remembered registry init and ephemeral scans"
```

## Task 4: Wax Sync, Docs, and Final Verification

**Files:**

- Create: `engine/crates/wax-core/src/sync.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Create: `engine/crates/wax-cli/src/commands/sync.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`
- Add `engine/crates/wax-cli/tests/sync_command.rs`
- Modify: `README.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Modify: `docs/specs/2026-06-02-registry-sources-and-wax-layout-design.md`
- Modify: `docs/specs/2026-06-13-interactive-init-design.md`

**Interfaces:**

- Consumes: `registry.upstream` values from config v2
- Consumes: remembered design-system repo roots from global state
- Produces: `wax sync` command
- Produces: updated app registry source or copied app-local registry JSON
- Produces: refreshed `.wax/wax.lock.json`

- [ ] **Step 1: Add failing sync tests**

Add tests for:

- copying local DS registry changes into `.wax/registries/acme/react.json`
- switching app `registry.source` to `published_source`
- failing when `acme/react` cannot be resolved in global memory

Run:

```bash
cd engine
cargo test -p wax-cli --test sync_command
```

Expected: FAIL because `wax sync` does not exist.

- [ ] **Step 2: Implement core sync orchestration**

Create `wax-core::sync` to resolve upstreams, read DS config, copy local registries, update hosted sources, and refresh registry lock entries.

Run:

```bash
cd engine
cargo test -p wax-core sync
```

Expected: PASS.

- [ ] **Step 3: Add CLI command**

Add:

```bash
wax sync
```

The command operates on the current repo and prints each updated language registry.

Run:

```bash
cd engine
cargo test -p wax-cli --test sync_command
```

Expected: PASS.

- [ ] **Step 4: Add best-effort scan sync**

When config contains at least one `registry.upstream`, `wax scan` attempts the same sync refresh as `wax sync` before scanning. If sync fails, scan continues and prints:

```text
warning: registry sync failed for acme/react; scanning with current registry source. Run `wax sync` for details.
```

Run:

```bash
cd engine
cargo test -p wax-cli scan_command
cargo test -p wax-cli --test sync_command
```

Expected: PASS, including coverage that a scan-time sync failure does not fail the scan.

- [ ] **Step 5: Update docs**

Update README and specs so examples use:

```bash
wax registry discover --design-system acme --name "Acme Design System" --language react --root src
wax init
wax sync
wax scan
```

Remove references to `.waxrc`, top-level `wax.lock.json`, and `design_system_registry`.

- [ ] **Step 6: Final verification**

Run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add engine README.md docs/specs
git commit -m "feat: sync app registries from remembered design systems"
```

## Self-Review

- Spec coverage: covered config v2, hard legacy removal, remembered design systems, no-config scan, init, sync, management commands, pack-index naming, and docs.
- Placeholder scan: no task contains TBD/TODO/fill-in placeholders.
- Task sizing: four implementation PRs; each task has focused tests and a commit boundary.
- Residual risk: Task 3 may be the largest because no-config scan requires an in-memory config path through scan orchestration. If implementation reveals that as too broad, split ephemeral scan into its own PR after init.
