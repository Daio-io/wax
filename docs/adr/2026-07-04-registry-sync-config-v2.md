# ADR: Registry sync and config v2

**Status:** Accepted (implemented)
**Date:** 2026-07-04
**Type:** Addendum (config cutover and registry sync)
**Related:** [Design spec](../specs/2026-07-04-registry-sync-config-design.md) · [Archived implementation plan](../plans/archive/2026-07-04-registry-sync-config-plan.md)

## Context

Wax alpha shipped registry discovery, interactive init, and scan with a `.waxrc` language array, optional top-level `wax.lock.json`, and `design_system_registry` pack config. The handoff from design-system repos to app repos still required manual registry copies and config edits. Because Wax remained pre–broad-public-use alpha, the product could make a clean cut to config v2, remembered design systems, ephemeral no-config scans, and explicit app sync.

## Decision

1. **Config v2 only** — Repo config uses `.wax/wax.config.json` with `schema_version: 2`, a `languages` object keyed by language id, and optional `design_systems` for DS publication. Lockfiles live only at `.wax/wax.lock.json`. Remove support for `.waxrc`, top-level `wax.lock.json`, and `design_system_registry`.
2. **Remembered design systems** — Global state stores design-system ids, display names, repo roots, and last-seen config paths. `wax registry discover --design-system` writes DS publication config and updates memory. `wax registry list|show|update|delete` manage memory only.
3. **Init and ephemeral scan** — `wax init` selects remembered registries and writes app config with `registry.source` and optional `registry.upstream`. TTY `wax scan` without config runs ephemeral selections using `.wax/cache/` and `.wax/out/` only; non-TTY scan without config fails with a `wax init` hint.
4. **`wax sync`** — Refreshes app registry inputs from remembered upstreams, copies local DS registry updates or switches to `published_source`, and refreshes `.wax/wax.lock.json`. `wax scan` attempts the same refresh best-effort before scanning and warns on failure.
5. **Pack-index naming** — User-facing language-pack index flags use `--pack-index`; environment variable is `WAX_PACK_INDEX`.

## Implementation summary

All 4 tasks shipped:

| Task | What shipped |
|------|----------------|
| Config v2 cutover | v2 parser and schema, legacy file discovery removal, lockfile paths (#196) |
| Remembered design systems | Global state, registry memory helper, discover flags, list/show/update/delete (#197) |
| Init and ephemeral scan | Remembered registry init, no-config TTY scan, `--pack-index` rename (#198) |
| Sync and docs | `wax-core::sync`, `wax sync`, scan-time best-effort sync, README and spec updates (#199, #200) |

## Consequences

### Positive

- App repos refresh registry inputs from remembered design systems without manual copying.
- First local scan works without committed config; CI paths stay explicit through `wax init`.
- Config, lockfile, and registry layout are easier to explain with a single `.wax/` model.

### Negative / trade-offs

- No migration from legacy `.waxrc` or top-level lockfiles; alpha users must re-init or rewrite config.
- Ephemeral scans require a TTY; headless no-config scan is intentionally unsupported.
- Best-effort scan sync can leave stale registry inputs until the user runs `wax sync`.

## References

- [Registry sync and config v2 design spec](../specs/2026-07-04-registry-sync-config-design.md)
- [Archived implementation plan](../plans/archive/2026-07-04-registry-sync-config-plan.md)
- [Registry sources and wax layout ADR](./2026-06-02-registry-sources-and-wax-layout.md)
- [Registry discovery ADR](./2026-06-04-registry-discovery.md)
- [Interactive init ADR](./2026-06-13-interactive-init.md)
