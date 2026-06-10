# ADR: Registry sources and centralized `.wax/` layout

**Status:** Accepted (implemented)  
**Date:** 2026-06-02  
**Type:** Addendum (repo-local config and registry locking)  
**Related:** [Design spec](../specs/2026-06-02-registry-sources-and-wax-layout-design.md) · [Archived implementation plan](../plans/archive/2026-06-02-registry-sources-and-wax-layout.md)

## Context

Early alpha used repo-root `.waxrc` and `wax.lock.json` with a single local `design-system/registry.json` path. Teams needed:

- A centralized `.wax/` directory for config, locks, cache, and scan output.
- Optional local or hosted registry sources per language.
- Digest-locked registry content for deterministic `validate` and `scan`.

## Decision

1. **Centralized layout** — prefer `.wax/wax.config.json`, `.wax/wax.lock.json`, and `.wax/wax.registry.json`; continue reading legacy `.waxrc` / `wax.lock.json` with warnings during migration.
2. **Registry sources** — per-language `registry` config accepts repo-relative paths or remote URLs; external sources materialize into `.wax/cache/registries/` before scan.
3. **Lockfile registry digests** — `wax.lock.json` records sha256 per language registry; `validate` and `scan` check lock alignment; `wax language update` refreshes registry locks.
4. **Language pack contract** — `registry` is canonical; `design_system_registry` remains a deprecated alias for the migration window.
5. **Init** — `wax init` writes the centralized layout and updates `.gitignore` for `.wax/cache/` and `.wax/out/`.

## Implementation summary

All 11 tasks shipped in `wax-core`, `wax-cli`, and language packs:

| Task | What shipped |
|------|----------------|
| Repo file discovery | `repo_files.rs` with preferred/legacy path resolution and warnings |
| Registry config parsing | Typed per-language `registry` on `WaxRc` with `extra` passthrough |
| Registry source resolution | Fetch/read, JSON validation, sha256 digest, cache materialization, config rewrite to local path |
| Lockfile digests | Registry lock entries keyed by language id |
| Validate integration | Layout warnings, deprecated alias warnings, lock digest checks |
| Scan integration | Registry materialization before jobs, digest verification, pack config rewrite |
| Language pack alias | `registry` canonical in `wax-lang-basic` and `wax-lang-compose` |
| Init layout | `.wax/wax.*.json` scaffold, gitignore updates |
| Language commands | `language update` / `doctor` use discovered repo files |
| Schemas and docs | Updated `.waxrc` schema, README onboarding, spec cross-links |
| Verification | Full engine test/clippy pass, plan checkbox completion |

## Consequences

### Positive

- Repos have one obvious `.wax/` home for config, locks, cache, and output.
- Remote registry URLs work in CI with digest-locked reproducibility.
- Legacy configs keep working during migration.

### Negative / trade-offs

- Dual-path discovery adds complexity until legacy files are removed.
- Remote registry fetch requires network at scan/validate time unless cached.

## References

- [Registry sources design spec](../specs/2026-06-02-registry-sources-and-wax-layout-design.md)
- [Archived implementation plan](../plans/archive/2026-06-02-registry-sources-and-wax-layout.md)
- [Language packs spec](../specs/2026-05-16-language-packs-and-distribution.md)
