# Language Packs, Configuration, and Distribution

**Status:** Active spec; partially implemented in alpha rollout (see release plan Task 15 for getting-started/docs)  
**Date:** 2026-05-16  
**Related:** [Component tracker design](./2026-05-13-component-tracker-design.md), [Rust engine workspace](../../engine/)

## Summary

`wax` is a **Rust analysis engine** with optional **language packs** (Compose, React, Swift, …) that discover source, parse, and emit normalized **scan facts**. The **kernel** orchestrates `scan`, merges facts, and owns reporting semantics (wrappers, adoption, drift, static export).

**Plugins** (reserved for a later phase) are **small kernel hooks**—export formatters, custom rules, fact transforms—not full language pipelines.

End users install a **`wax` binary** and download language packs globally. Each repository uses **`.waxrc`** to enable languages and hold per-language config. Language packs **do not communicate with each other**; only the engine talks to each pack.

## Implementation plan roadmap

Plan order, doc/implementation status, gates, and agent rules live in **[`docs/plans/README.md`](../plans/README.md)** only. Do not duplicate that table here.

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

Production Rust code MUST model language ids as a validated `LanguageId` newtype, not raw `String`. Valid ids are lowercase ASCII slugs (`[a-z][a-z0-9-]*`) and the same type is used across `.waxrc`, manifests, lockfiles, wire messages, and `ScanFacts`.

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
 wax-lang-compose  wax-lang-react  wax-lang-* later
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

## Production contract requirements

The production `wax-contract` crate is the stable boundary for language packs and reports. It MUST:

- use `#![deny(missing_docs)]` from the first production PR;
- expose typed error enums instead of returning `String` errors from contract parsing/validation helpers;
- use typed timestamps (`time::OffsetDateTime` or equivalent) for recorded times, with RFC 3339 JSON serialization;
- use `SourceLocation { file, line, column: Option<u32> }` for source references instead of duplicating `file` / `line` fields across fact types;
- model language ids as a validated `LanguageId` newtype and use it across `.waxrc`, manifests, lockfiles, wire messages, and `ScanFacts`;
- split parser metadata into `parser_name` and `parser_version` fields instead of a combined parser string;
- define `adoption_coverage_ratio` as `resolved_count / usage_site_count`, excluding `candidate` matches from the numerator; when `usage_site_count == 0`, the ratio is `null`;
- reserve extension fields only where the engine has a known compatibility need.

### `ScanFacts` contract fields

`ScanFacts.language` is a `LanguageMetadata` object describing the language pack that produced the facts:

| Field | Meaning |
|-------|---------|
| `id` | Validated `LanguageId` slug for the language pack, e.g. `compose` |
| `version` | Language pack release version |
| `ecosystem` | Human-readable ecosystem/stack key, e.g. `jetpack-compose` |
| `parser_name` | Parser implementation name used during extraction |
| `parser_version` | Parser implementation version used during extraction |

`ScanFacts.snapshot_id` is assigned by the engine and echoed by the language pack. `ScanFacts.scanned_at` is an RFC 3339 timestamp serialized from a typed timestamp. Source-bearing facts use `SourceLocation { file, line, column }`; `file` is repository-relative, `line` is one-based, and `column` is optional and one-based when present.

`ScanFacts.metrics.adoption_coverage_ratio` is recomputed from usage facts as `resolved_count / usage_site_count`. Candidate matches are counted separately and are not included in `resolved_count`; when there are no usage sites, the ratio is `null`.

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
      "roots": ["*/src/main/kotlin"]
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
Source `roots` are repo-relative directories. Language packs may also expand path components that are exactly `*`, so Android multi-module repositories can use patterns such as `*/src/main/kotlin`. This is not full glob syntax: `**`, `?`, and mixed wildcard segments such as `app-*` are not expanded. Literal missing roots report `root_not_found`; wildcard roots that match no directories report `root_glob_not_found`.

### `wax.lock.json` (repository, committed)

Pins resolved artifacts for reproducible local and CI scans. **Required for repositories using language packs**; `wax init` writes it after resolving selected pack artifacts.

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
        "sha256": "…",
        "signature": null
      }
    }
  }
}
```

- **`api_version` per language** — verified before spawn.
- **`resolved`** — host triple, url, and sha256 for the machine that produced the lock (CI must match triple or use a matrix).
- **`resolved.signature`** — reserved for Sigstore/cosign metadata in v1.1; `null` in v1.
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
  "parser_name": "tree-sitter-kotlin",
  "parser_version": "0.3.8"
}
```

**Command resolution:** `command[0]` is resolved relative to the manifest directory when not absolute. Absolute paths in `command` are rejected in v1. On Windows (non-goal for v1), engines would try `.exe` suffix—see Non-goals.

## Wire protocol (engine ↔ language pack) — v1

Transport: **stdio, binary-safe length not required for v1** — one UTF-8 JSON object written to pack stdin, one JSON object read from pack stdout. **Stderr** is unstructured pack logs; engine may tee to `~/.wax/logs/<scan_id>/<language_id>.stderr`.

Future **daemon mode** will use NDJSON (`initialize` / `scan` / `progress` / `shutdown`) on the same fd pair.

In-process and subprocess scan request types MUST share the same fields. The engine populates `api_version`, `language_id`, `repo_root`, `snapshot_id`, and `config` before invoking either an in-process `LanguageExtractor` or a subprocess language pack.

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
    "roots": ["*/src/main/kotlin"]
  }
}
```

- **`snapshot_id`:** assigned by the engine before spawn; pack **MUST** echo the same value in `ScanFacts.snapshot_id`.
- **`config`:** opaque to the engine; validated by the pack.

### Success response (pack → engine)

Single tagged JSON object containing `ScanFacts` (`wax-contract`). Abridged example:

```json
{
  "type": "scan_facts",
  "api_version": 1,
  "language_id": "compose",
  "facts": {
    "schema_version": 1,
    "language": {
      "id": "compose",
      "version": "0.4.2",
      "ecosystem": "jetpack-compose",
      "parser_name": "tree-sitter-kotlin",
      "parser_version": "0.3.8"
    },
    "snapshot_id": "scan-20260516-abc123",
    "scanned_at": "2026-05-16T12:00:00Z"
  }
}
```

The response envelope is tagged by `type`; production code MUST NOT use untagged success/error deserialization.

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
| `registry_not_found` | Configured design-system registry path is missing |
| `parser_init_failed` | Parser/runtime could not initialize before scanning |
| `timeout` | Pack exceeded the engine deadline |
| `scan_failed` | Unrecoverable extraction failure |
| `internal_error` | Unexpected pack failure; message should be safe to display |

### Engine responsibilities

| Topic | v1 policy |
|-------|-----------|
| **Timeout** | Default 10 minutes per language pack; `WAX_SCAN_TIMEOUT_SECS` override |
| **Cancellation** | SIGTERM, 5s grace, then SIGKILL on Ctrl-C or parent cancel |
| **Response size** | No fixed response cap in v1; engine must stream or spool stdout safely instead of buffering unbounded data in memory |
| **Version mismatch** | No best-effort across `api_version`; refuse before spawn |

## Pack distribution trust model (v1)

Language packs are **native executables** downloaded from a remote index. v1 assumes the operator trusts the pack index host and TLS to that host; the engine adds digest verification and repository lockfile pins so teams can detect index drift and keep CI reproducible.

### v1 trust boundary

| Topic | v1 decision |
|-------|-------------|
| **Trust root** | Default pack index URL baked into engine; override only via `WAX_LANG_INDEX` |
| **Integrity** | **sha256** of artifact bytes after download; index entry supplies expected hash |
| **Authenticity** | **HTTPS** to index and release URLs in v1 (TLS + host you trust) |
| **Lockfile** | Required for repositories using language packs; pins version + digest + target |
| **Sandbox** | **No sandbox** — pack subprocess runs as the invoking user |
| **Mirrors** | `WAX_LANG_INDEX` may point at a corporate mirror; `wax language doctor` prints the effective index URL |

**What v1 does not verify:** code signing, publisher identity beyond TLS, or runtime isolation. A compromised index or MITM on an untrusted network could serve malicious pack bytes until digest checks fail against a committed lockfile.

### Threats and mitigations (v1)

| Threat | Mitigation in v1 |
|--------|------------------|
| Artifact tampered in transit | HTTPS to index/releases; sha256 verified against index entry at install time |
| Index serves a newer digest for the same version string | Lockfile pins `resolved.sha256`; auto-install and `wax language install` **refuse digest drift** |
| Silent upgrade to a newer pack version on scan | Lockfile pins `version`; auto-install installs the locked version only |
| CI pulls “latest” instead of team-approved packs | CI **MUST** commit `wax.lock.json` and run `wax scan --no-auto-install` |
| Wrong host triple installed | Lockfile `resolved.target` must match install host; policy treats target mismatch as not ready |
| Malicious pack binary at rest | No v1 signature check; operator trusts download source + lockfile audit |

### Lockfile vs auto-install precedence

Repositories that enable language packs **MUST** commit `wax.lock.json`. Auto-install is a convenience for local dev; the lockfile is always authoritative for **which** artifact to fetch.

Evaluation order for each **enabled** language id (engine policy; see `wax-core` auto-install):

1. **Lockfile required** — if the id is enabled in `.waxrc` but missing from `wax.lock.json`, scan fails (no implicit “latest”).
2. **Already satisfied** — if `~/.wax/langs/<id>/<version>/manifest.json` matches the lock (`version`, `api_version`, `resolved.target`, `resolved.sha256`), the pack is ready; no download.
3. **Auto-install disabled** (`wax scan --no-auto-install`) — if the locked artifact is not installed locally, scan fails with a clear missing-install error (CI path).
4. **Pack index lookup** — when auto-install is allowed, fetch index metadata for the locked `version` + `resolved.target`.
5. **Digest drift** — if the index sha256 for that version/target differs from `resolved.sha256`, refuse install/scan even when auto-install is on.
6. **Install plan** — when allowed and digests match, download exactly the locked `version` and verify bytes against `resolved.sha256` (never a newer index version).

| Scenario | `wax.lock.json` | Local install | `--no-auto-install` | Outcome |
|----------|-----------------|---------------|---------------------|---------|
| CI scan | committed pin | optional pre-install | **yes** | fail if pin not installed |
| Local dev scan | committed pin | missing | no (default) | download locked pin if index agrees |
| Index rotated digest for same version | committed pin | any | any | **fail** (digest drift) |
| Enable language without lock entry | absent entry | any | any | **fail** (missing lock) |

Auto-install default: **on** for local `wax scan`. CI **MUST** use `wax scan --no-auto-install` with the committed `wax.lock.json`.

`wax init` writes `.waxrc` and `wax.lock.json` after resolving concrete pack artifacts from the index (same digest rules apply).

### Planned v1.1 signing (Sigstore / cosign)

v1 records `resolved.signature: null` in `wax.lock.json`. **v1.1** will add optional **Sigstore** bundle verification (typically **cosign**-signed release artifacts) without changing the lockfile shape:

- Pack index entries may advertise signature metadata alongside `sha256`.
- `wax language install` / auto-install verify signature when `resolved.signature` is present.
- Unsigned artifacts remain supported for mirrors that only mirror HTTPS + digest.

Direction: first-party releases on GitHub/OCI signed with cosign; engine trusts a configurable Sigstore root or pinned issuer policy. Exact trust policy TBD in the v1.1 task; v1 ships digest + HTTPS only.

## CLI surface (v1)

All language lifecycle commands use the **`wax language`** group (singular):

| Command | Purpose |
|---------|---------|
| `wax init` | Onboard: write `.waxrc`, resolve packs, write `wax.lock.json`, scaffold DS registries |
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

### Prebuilt release matrix (v1 sketch)

v1 ships **prebuilt binaries only** for the engine and first-party language packs. Implementation may use **[cargo-dist](https://github.com/axodotdev/cargo-dist)** (preferred when it fits the monorepo layout) or an equivalent **GitHub Actions release matrix** that cross-compiles each crate target and uploads per-triple archives to GitHub Releases. This section is a distribution sketch; wiring CI is a follow-on task.

**Supported host triples (v1):**

| Shorthand | Rust target triple | Notes |
|-----------|-------------------|--------|
| darwin-arm64 | `aarch64-apple-darwin` | Apple Silicon macOS |
| darwin-x64 | `x86_64-apple-darwin` | Intel macOS |
| linux-x64-gnu | `x86_64-unknown-linux-gnu` | glibc Linux (CI and most dev containers) |
| linux-arm64-gnu | `aarch64-unknown-linux-gnu` | ARM64 Linux (e.g. Graviton CI) |

Windows (`x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`) is recognized by the CLI for local dev but **out of scope** for the v1 prebuilt matrix above.

**Separate artifacts per triple:**

Each supported triple gets its own downloadable archive(s). Do not bundle language packs inside the `wax` binary.

| Binary | Crate / role | Release artifact name (example) |
|--------|--------------|----------------------------------|
| `wax` | `wax-cli` | `wax-<version>-<triple>.tar.gz` |
| `wax-lang-compose` | `wax-lang-compose` | `wax-lang-compose-<version>-<triple>.tar.gz` |
| `wax-lang-react` | `wax-lang-react` | `wax-lang-react-<version>-<triple>.tar.gz` |

Pack index entries and `wax.lock.json` `resolved.target` use the **Rust triple** strings above (same as `wax language install` host detection). First-party pack ids in manifests remain `compose` and `react`; only the on-disk binary names use the `wax-lang-<id>` prefix.

**Release flow (sketch):**

1. Tag `wax` + pack versions; CI builds the matrix for `wax`, `wax-lang-compose`, and `wax-lang-react`.
2. Upload per-triple archives to GitHub Releases (or object storage behind `releases.wax.dev`).
3. Publish/update the pack index (`WAX_LANG_INDEX`) with `targets` maps keyed by triple, `url`, and `sha256` per language id + version.
4. `wax language install` / `wax init` resolve the host triple and download the matching artifact; lockfiles pin the resolved digest.

**Optional Phase 5b — npm wrapper (not blocking v1):**

A future `@wax/cli` (or similar) npm package may download the correct prebuilt `wax` binary for the host triple via `postinstall`. That improves Node-centric onboarding but is **not required** for v1: users can install `wax` from GitHub Releases, Homebrew, or a curl installer script. Defer npm packaging until the release matrix and pack index are stable.

### First-party language packs (v1 targets)

First-party pack binaries use `wax-lang-<id>` names, for example `wax-lang-compose` and `wax-lang-react`. Keep crate names, install manifests, release artifacts, and examples aligned with that convention.

| id | Parser | Notes |
|----|--------|-------|
| `basic` | Text line scanner | Generic fallback for unsupported languages and smoke tests |
| `compose` | tree-sitter-kotlin | First production parser-backed language |
| `react` | SWC | TSX/JSX extraction |
| `swift` | Deferred | Later-phase language pack after a dedicated parser decision |

### Basic fallback scanner

`wax-lang-basic` is the generic text-scanner fallback for languages that do not yet have parser-backed packs. It reads a repo-local `design_system_registry` plus configured `roots`, optionally expands path components that are exactly `*` in roots for multi-module repositories, optionally filters files with `file_extensions` or `include_globs`, scans source text for registry symbols, and emits heuristic resolved usage facts with an informational `basic_text_scan` diagnostic. It does not extract local components or claim ecosystem-specific syntax awareness.

`include_globs` supports only `*suffix` filename patterns (for example `*.src`); full glob syntax such as `src/**/*.kt` is not supported. The line scanner strips `//` comments before matching, so code after `//` inside strings or URLs may be missed.

Use `basic` for unsupported languages, smoke tests, and early adoption estimates only. Parser-backed packs such as `compose` and `react` remain the production path for supported ecosystems.

`engine/crates/wax-lang-basic/tests/fixtures/small/` commits a language-agnostic fixture and golden count summary. `cargo test -p wax-lang-basic` asserts usage counts, alias resolution, comment/string false-positive guards, and one-based source columns against `golden.json`.

### Compose correctness gate and parser path

`wax-lang-compose` commits a small Kotlin fixture set and golden count summary under `engine/crates/wax-lang-compose/tests/fixtures/small/`. `cargo test -p wax-lang-compose` asserts `usage_site_count`, `resolved_count`, `local_component_count`, and `design_system_component_count` against `golden.json`.

**Production parser path (tree-sitter-kotlin):**

- `wax-lang-compose` uses **tree-sitter-kotlin** for AST-based Kotlin parsing. `language.parser_name` is `"tree-sitter-kotlin"`.
- The scanner discovers Kotlin files under configured `roots`, expands path components that are exactly `*` such as `*/src/main/kotlin` for Android multi-module repositories, parses syntax trees, identifies `@Composable` function declarations (local components) and call expressions matching registry symbols (resolved DS usages), and emits repository-relative `SourceLocation` values with one-based line and column numbers.
- Direct calls and alias calls resolve to canonical registry symbols. Qualified (navigation) calls, comments, and string literal content are not counted.
- Parser initialisation failures map to the `parser_init_failed` wire error code rather than panicking.
- Requests without compose scan keys return scaffold facts with the `compose_scaffold` diagnostic.
- `wax-lang-basic` is the explicit text-scanner fallback for unsupported languages; Compose does not use line scanning.

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

**In-process:** engine calls `LanguageExtractor::scan(ScanRequest)` with the same fields as the wire request after validating config.

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
- Code-signature enforcement in v1 (Sigstore/cosign planned for v1.1)
- Full static site export design

## Decisions from review

1. **`.waxrc` format:** JSON-only for v1.
2. **Lockfile:** required for repositories using language packs.
3. **Swift parser:** deferred to a later phase.
4. **Response size:** no fixed cap; engine implementation must handle large responses safely.
5. **Signing:** plan Sigstore/cosign for v1.1, while v1 relies on HTTPS + sha256 + lockfile pins.
