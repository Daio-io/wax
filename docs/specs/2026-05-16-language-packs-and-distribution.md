# Language Packs, Configuration, and Distribution

**Status:** Draft for review  
**Date:** 2026-05-16  
**Related:** [Component tracker design](./2026-05-13-component-tracker-design.md), [Architecture evaluation plan](../plans/2026-05-14-architecture-evaluation-plan.md), [Foundation ADR](../adr/2026-05-14-foundation-architecture-decision.md), [Rust prototype](../../rust-prototype/README.md)

## Summary

`wax` is a **Rust analysis engine** with optional **language packs** (Compose, React, Swift, …) that discover source, parse, and emit normalized **scan facts**. The **kernel** orchestrates `scan`, merges facts, and owns reporting semantics (wrappers, adoption, drift, static export).

**Plugins** (reserved for a later phase) are **small kernel hooks**—export formatters, custom rules, fact transforms—not full language pipelines.

End users install a **`wax` binary** and download language packs globally. Each repository uses **`.waxrc`** to enable languages and hold per-language config. Language packs **do not communicate with each other**; only the engine talks to each pack.

## Terminology

| Term | Meaning |
|------|---------|
| **Engine / kernel** | `wax` binary: orchestration, merge, graph, metrics, static site export |
| **Language pack** | Installable unit for one stack (`compose`, `react`, `swift`): discover → parse → extract → `ScanFacts` |
| **Language id** | Stable string key used in `.waxrc`, CLI, and global install paths |
| **`scan`** | CLI command that runs all **enabled** language packs and produces merged artifacts |
| **Plugin** (future) | Optional kernel extension; not used for language extraction in v1 |

## Architecture

```text
  .waxrc (repo)              ~/.wax/ (global)
  languages: enabled         langs/<id>/<version>/binary + manifest.json
       │                              │
       └──────────┬───────────────────┘
                  ▼
           ┌─────────────┐
           │ wax engine  │
           └──────┬──────┘
      ┌────────────┼────────────┐
      ▼            ▼            ▼
  wax-lang-*   wax-lang-*   wax-lang-*
  (subprocess) (subprocess) (subprocess)
```

### Invariants

1. Language packs emit **facts only**; the kernel emits **reports**.
2. Language packs **MUST NOT** call other language packs.
3. One **stable wire protocol** (NDJSON over stdio) for all language packs, in-process or subprocess.
4. **Enabled** in `.waxrc` is separate from **installed** globally; `wax scan` may auto-install when enabled and missing (overridable for CI).

## Configuration

### `.waxrc` (repository, committed)

Primary project config. Format: **JSON** (v1); YAML may be added later if needed.

```json
{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "registry": "design-system/registry.json",
      "roots": ["app/src/main/kotlin", "feature/**/src/**/kotlin"]
    },
    {
      "id": "react",
      "enabled": true,
      "registry": "packages/ui/registry.json",
      "roots": ["apps/web/src", "packages/ui/src"]
    }
  ]
}
```

Per-language keys (e.g. `registry`, `roots`) are validated by that language pack’s config schema.

### `wax.lock.json` (repository, committed)

Pins installed language pack versions and protocol compatibility for reproducible CI:

```json
{
  "schema_version": 1,
  "engine_api_version": 1,
  "languages": {
    "compose": { "version": "0.4.2", "sha256": "…" },
    "react": { "version": "0.3.1", "sha256": "…" }
  }
}
```

### Global state

`~/.wax/state.json` — installed language packs and paths:

```json
{
  "languages": {
    "compose": {
      "version": "0.4.2",
      "install_path": "/Users/me/.wax/langs/compose/0.4.2"
    }
  }
}
```

### Language pack manifest (per install)

`~/.wax/langs/<id>/<version>/manifest.json`:

```json
{
  "id": "compose",
  "version": "0.4.2",
  "api_version": 1,
  "command": ["wax-lang-compose", "--stdio"],
  "ecosystem": "jetpack-compose",
  "parser": "tree-sitter-kotlin@0.3.8"
}
```

## Wire protocol (engine ↔ language pack)

Transport: **newline-delimited JSON** on stdin/stdout. No language pack listens on network ports.

### Request (engine → pack)

```json
{
  "type": "scan",
  "api_version": 1,
  "language_id": "compose",
  "repo_root": "/abs/path/to/repo",
  "mode": "cold-process-warm-fs",
  "config": { "registry": "design-system/registry.json", "roots": ["…"] },
  "snapshot_id": null
}
```

### Response (pack → engine)

Single-line JSON matching `ScanFacts` in `wax-contract` (see `rust-prototype/crates/wax-contract`):

- `language`: `LanguageMetadata` (id, version, ecosystem, parser)
- `usage_sites`, `local_components`, `design_system_components`, `metrics`, `counts`, `diagnostics`

Optional daemon mode (later): `initialize` / `scan` / `shutdown` messages on the same stream to amortize process startup.

## CLI surface (v1)

| Command | Purpose |
|---------|---------|
| `wax init` | Onboard: write `.waxrc`, optional `wax.lock.json`, scaffold registries |
| `wax languages list` | Built-in + installed language ids |
| `wax language install <id>[@version]` | Download to `~/.wax/langs/` |
| `wax language uninstall <id>` | Remove global install |
| `wax language update [<id>] [--all]` | Upgrade; update lockfile |
| `wax language doctor` | Enabled vs installed vs lock skew |
| `wax scan` | Run enabled languages; merge; write artifacts under `.wax/` |
| `wax validate` | Registry + config validation (kernel + per-language) |

Flags:

- `wax scan --no-auto-install` — fail if enabled language missing (CI)
- `WAX_PLUGIN_REGISTRY` / `WAX_LANG_REGISTRY` — mirror URL for air-gapped installs

## Distribution

### End users

- Install **`wax`** from GitHub Releases / Homebrew / installer script.
- **No Rust toolchain** required when using prebuilt artifacts.
- Language packs: downloaded per id + platform triple from official registry.

### Contributors

- Rust stable + C toolchain (tree-sitter / SWC native deps).
- Build engine and language packs from `rust-prototype/` workspace (transitional) then product monorepo layout.

### First-party language packs (v1 targets)

| id | Parser | Notes |
|----|--------|-------|
| `compose` | tree-sitter-kotlin | First product language; aligns with Phase 0 fixtures |
| `react` | SWC | TSX/JSX component and usage extraction |
| `swift` | TBD | Planned; grammar choice is a review gate |

### Monolithic vs modular CLI

Some tools ship one npm package with a single prebuilt native addon (parser + CLI bundled). Wax ships a **slim engine** plus **optional language packs** so monorepos enable only what they need. Optional future: npm meta-package that downloads `wax` + language binaries without requiring Rust on the consumer machine.

## Contract types (Rust)

| Crate | Role |
|-------|------|
| `wax-contract` | `ScanFacts`, `LanguageMetadata`, `MergedScan` |
| `wax-lang-api` | `LanguageExtractor`, `ScanRequest`, `LanguageError` |
| `wax-core` | Engine registry, merge |
| `wax-lang-<id>` | First-party implementations |
| `wax-cli` | User-facing binary (`wax`) |

Subprocess adapters implement the same JSON messages as in-process `LanguageExtractor`.

## Relationship to Phase 0 and ADR

- Phase 0 proved artifact shape and parsers; ADR provisionally favored TS+TS for foundation.
- This spec describes the **Rust engine + language pack** direction from prototype option D and product review (self-hosted static reports, multi-language, SWC/tree-sitter per pack).
- **ADR update** is a separate review item once this spec is approved (do not silently supersede ADR).

## Design spec alignment (follow-up)

[Component tracker design](./2026-05-13-component-tracker-design.md) uses “ecosystem plugins” for extractors. After this spec is approved, update that document to **language pack** for extractors and reserve **plugin** for kernel hooks.

## Non-goals (this spec)

- Third-party language pack registry marketplace
- WASM language packs (noted as future option)
- SaaS login or hosted-only reports
- Full static site export design (referenced; detailed in a later spec)

## Open questions for review

1. **`.waxrc` format:** JSON-only v1, or YAML/TOML from day one?
2. **Auto-install on `scan`:** default on for local dev, off in CI, or always explicit `wax language install`?
3. **Lockfile:** required in all repos or optional until team opts in?
4. **Binary naming:** `wax-lang-compose` vs `wax-language-compose`?
5. **Swift parser:** tree-sitter-swift vs other; separate spike before promising id in registry?
