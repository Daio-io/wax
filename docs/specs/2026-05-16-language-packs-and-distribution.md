# Language Packs, Configuration, and Distribution

**Status:** Draft for review  
**Date:** 2026-05-16  
**Related:** [Component tracker design](./2026-05-13-component-tracker-design.md), [Rust prototype](../../rust-prototype/README.md)

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
| **Design system registry** | Repo-local file listing canonical DS components (per language config) |
| **Pack index** | Remote manifest listing downloadable language pack artifacts (`WAX_LANG_INDEX`) |
| **`scan`** | CLI command that runs all **enabled** language packs and produces merged artifacts |
| **Plugin** (future) | Optional kernel extension; not used for language extraction in v1 |

Avoid overloading **registry**: in `.waxrc`, use `design_system_registry` for the in-repo DS file path; reserve **pack index** for the remote install source.

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
3. v1 wire format: **one JSON object on stdin, one JSON object on stdout** (upgrade to NDJSON multi-message when daemon mode lands).
4. **Enabled** in `.waxrc` is separate from **installed** globally; `wax scan` may auto-install when enabled and missing (overridable for CI).

## Versioning matrix

| Field | Where | Bumps when |
|-------|--------|------------|
| `schema_version` | `.waxrc`, `wax.lock.json`, `ScanFacts`, `MergedScan` | Repo config or fact JSON shape changes |
| `engine_api_version` | `wax.lock.json` | Engine orchestration / CLI contract changes |
| `api_version` | Pack manifest, wire `scan` request | Engine ↔ pack message shape changes |
| `LanguageMetadata.version` | `ScanFacts` | Language pack release only |

Rules:

- Engine **MUST** reject wire `api_version` newer than it supports.
- Pack **MUST** refuse (structured error) when `request.api_version` > `manifest.api_version`.
- `ScanFacts.schema_version` **MUST** match `SCHEMA_VERSION` constant; engine validates on ingest.

## Configuration

### `.waxrc` (repository, committed)

Primary project config. Format: **JSON** (v1).

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
      "design_system_registry": "design-system/registry.json",
      "roots": ["app/src/main/kotlin"]
    },
    {
      "id": "react",
      "enabled": true,
      "design_system_registry": "packages/ui/registry.json",
      "roots": ["apps/web/src"]
    }
  ]
}
```

`engine.scan_concurrency` defaults to `2`; override via CLI `wax scan --concurrency=N`. Packs run in separate processes and should not assume exclusive host access. v1 does **not** pass concurrency into the wire request (isolation is by process boundary); revisit if in-process packs need shared resource hints.

Per-language keys beyond `id` / `enabled` are validated by that language pack’s config schema.

### `wax.lock.json` (repository, committed for CI)

Pins resolved artifacts for reproducible CI. **Required when using `wax scan --no-auto-install` in CI**; optional for local-only workflows until teams opt in.

```json
{
  "schema_version": 1,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "locked_at": "2026-05-16T12:00:00Z",
  "languages": {
    "compose": {
      "version": "0.4.2",
      "api_version": 1,
      "source": "https://packs.wax.dev/index.json",
      "resolved": {
        "target": "aarch64-apple-darwin",
        "url": "https://releases.wax.dev/compose/0.4.2/aarch64-apple-darwin.tar.gz",
        "sha256": "…"
      }
    }
  }
}
```

- **`api_version` per language** — verified before spawn.
- **`resolved`** — host triple, url, and sha256 for the machine that produced the lock (CI must match triple or use a matrix).
- **`source`** — pack index URL or mirror id for audit.
- **`wax_version`** — engine that wrote the lock; `doctor` warns on skew.
- **`locked_at`** — when the lock was produced; optional audit field.

When a lockfile exists, auto-install **MUST** install exactly the pinned `version` + `resolved.sha256`; refuse if the index now serves a different digest for that version.

### Global state

`~/.wax/state.json` — installed language packs and paths (not committed).

### Language pack manifest (per install)

`~/.wax/langs/<id>/<version>/manifest.json`:

```json
{
  "id": "compose",
  "version": "0.4.2",
  "api_version": 1,
  "command": ["./wax-lang-compose", "--stdio"],
  "ecosystem": "jetpack-compose",
  "parser": "tree-sitter-kotlin@0.3.8"
}
```

**Command resolution:** `command[0]` is resolved relative to the manifest directory when not absolute. Absolute paths in `command` are rejected in v1. On Windows (non-goal for v1), engines would try `.exe` suffix—see Non-goals.

## Wire protocol (engine ↔ language pack) — v1

Transport: **stdio, binary-safe length not required for v1** — one UTF-8 JSON object written to pack stdin, one JSON object read from pack stdout. **Stderr** is unstructured pack logs; engine may tee to `~/.wax/logs/<scan_id>/<language_id>.stderr`.

Future **daemon mode** will use NDJSON (`initialize` / `scan` / `progress` / `shutdown`) on the same fd pair.

### Request (engine → pack)

```json
{
  "type": "scan",
  "api_version": 1,
  "language_id": "compose",
  "repo_root": "/abs/path/to/repo",
  "snapshot_id": "scan-20260516-abc123",
  "config": {
    "design_system_registry": "design-system/registry.json",
    "roots": ["app/src/main/kotlin"]
  }
}
```

- **`snapshot_id`:** assigned by the engine before spawn; pack **MUST** echo the same value in `ScanFacts.snapshot_id`.
- **`config`:** opaque to the engine; validated by the pack.

### Success response (pack → engine)

Single JSON object: `ScanFacts` (`wax-contract`). Field `type` is omitted on success (or `"type": "scan_facts"` if we add a tag later).

### Error response (pack → engine)

Non-zero exit is a last resort. Prefer a structured line on stdout:

```json
{
  "type": "error",
  "api_version": 1,
  "language_id": "compose",
  "code": "registry_not_found",
  "message": "design_system_registry path missing",
  "diagnostics": []
}
```

| `code` (v1) | Meaning |
|-------------|---------|
| `api_version_unsupported` | Request `api_version` too new |
| `config_invalid` | Pack rejected `config` |
| `scan_failed` | Unrecoverable extraction failure |

### Engine responsibilities

| Topic | v1 policy |
|-------|-----------|
| **Timeout** | Default 10 minutes per language pack; `WAX_SCAN_TIMEOUT_SECS` override |
| **Cancellation** | SIGTERM, 5s grace, then SIGKILL on Ctrl-C or parent cancel |
| **Max stdout size** | Soft cap 64 MiB per response; engine aborts with `response_too_large` |
| **Version mismatch** | No best-effort across `api_version`; refuse before spawn |

## Pack distribution trust model (v1)

| Topic | v1 decision |
|-------|-------------|
| **Trust root** | Default pack index URL baked into engine; override only via `WAX_LANG_INDEX` |
| **Integrity** | sha256 of artifact bytes; index entry supplies expected hash (integrity boundary = HTTPS + index you trust) |
| **Authenticity** | HTTPS to index/releases only; **code signing deferred to v1.1** (document explicitly) |
| **Lockfile** | When present, pins digest; auto-install must not upgrade silently |
| **Sandbox** | **No sandbox** — subprocess runs as the user; document in security notes |
| **Mirrors** | `WAX_LANG_INDEX` may point at corporate mirror; `doctor` prints effective index URL |

Auto-install default: **on** for local `wax scan`; CI **MUST** use `wax scan --no-auto-install` with a committed `wax.lock.json`.

## CLI surface (v1)

All language lifecycle commands use the **`wax language`** group (singular):

| Command | Purpose |
|---------|---------|
| `wax init` | Onboard: write `.waxrc`, optional `wax.lock.json`, scaffold DS registries |
| `wax language list` | Installed language ids (all packs are downloaded; none ship inside `wax`) |
| `wax language install <id>[@version]` | Download to `~/.wax/langs/` |
| `wax language uninstall <id>` | Remove global install |
| `wax language update [<id>] [--all]` | Upgrade; update lockfile |
| `wax language doctor` | Global install vs lock vs `.waxrc` enabled set |
| `wax scan` | Run enabled languages; merge; write artifacts under `.wax/` |
| `wax validate` | **Repo-only:** `.waxrc` + DS registry files consistent (no `~/.wax/` access) |

**`validate` vs `doctor`:** `validate` is fast, local, CI-friendly. `doctor` checks global install state and lock skew.

Flags:

- `wax scan --no-auto-install` — fail if enabled language missing (CI)
- `wax scan --concurrency=N` — override `.waxrc` `engine.scan_concurrency`
- `WAX_LANG_INDEX` — pack index URL for air-gapped / mirror installs

## Distribution

### End users

- Install **`wax`** from GitHub Releases / Homebrew / installer script.
- **No Rust toolchain** required when using prebuilt artifacts.
- Language packs: downloaded per id + platform triple from the pack index.

### First-party language packs (v1 targets)

| id | Parser | Notes |
|----|--------|-------|
| `compose` | tree-sitter-kotlin | First product language |
| `react` | SWC | TSX/JSX extraction |
| `swift` | TBD | Parser spike before registry listing |

### Monolithic vs modular CLI

Some tools ship one package with a single prebuilt native addon. Wax ships a **slim engine** plus **optional language packs** so monorepos enable only what they need.

## Contract types (Rust)

| Crate | Role |
|-------|------|
| `wax-contract` | `ScanFacts`, enums, `MergedScan` |
| `wax-lang-api` | `LanguageExtractor` (in-process), `protocol` (wire) |
| `wax-core` | Engine (future) |
| `wax-lang-<id>` | Pack binaries (future) |
| `wax-cli` | User-facing binary (future) |

**In-process:** engine calls `LanguageExtractor::scan(ScanRequest)` after validating config.  
**Subprocess:** engine uses `protocol::WireScanRequest` / `WireScanResponse` — same fields as the JSON above.

## Background: architecture evaluation (not in repo)

Phase 0 compared TS-core and Go-core spikes (fixtures, goldens, benchmarks). Provisional conclusion was TS+TS for lowest install friction; this spec proposes **Rust engine + downloadable language packs** for multi-language product goals. A formal ADR addendum is planned after this spec is approved.

## Design spec alignment (follow-up)

[Component tracker design](./2026-05-13-component-tracker-design.md) still says “ecosystem plugins” for extractors. After approval, rename to **language pack** and reserve **plugin** for kernel hooks.

## Non-goals (this spec)

- Windows language packs in v1 (macOS/Linux triples only)
- Third-party pack marketplace
- WASM packs
- SaaS login
- Code signing (deferred v1.1)
- Full static site export design

## Open questions for review

1. **`.waxrc` format:** JSON-only v1, or YAML/TOML from day one?
2. **Lockfile:** remain optional locally but required in documented CI template?
3. **Binary naming:** `wax-lang-compose` vs `wax-language-compose`?
4. **Swift parser:** tree-sitter-swift vs other?
5. **64 MiB response cap:** too low for huge monorepos?
6. **Signing:** minisign vs cosign vs sigstore for v1.1?
