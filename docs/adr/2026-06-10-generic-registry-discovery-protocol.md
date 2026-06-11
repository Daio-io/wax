# ADR: Generic registry discovery protocol

**Status:** Accepted (implemented)  
**Date:** 2026-06-10  
**Type:** Addendum (registry discovery wire protocol and per-language output)  
**Related:** [Implementation plan](../plans/archive/2026-06-10-generic-registry-discovery-protocol.md) · [Registry discovery ADR](./2026-06-04-registry-discovery.md) · [Language packs spec](../specs/2026-05-16-language-packs-and-distribution.md)

## Context

The v1 registry discovery workflow shipped with an intentional exception: `wax-core` linked `wax-lang-compose` in-process for `wax registry discover`. That shortcut avoided pack install requirements for authoring but contradicted the subprocess language-pack model used by `wax scan`, blocked React and other packs from adding discover heuristics behind the same contract, and wrote a single shared `.wax/wax.registry.json` that collided when multiple languages were discovered in one repository.

## Decision

1. **Subprocess discover wire protocol** — extend the v1 stdio JSON protocol with `discover` / `discover_symbols` messages on `WirePackRequest` / `WirePackResponse`. The engine spawns the installed language pack (same lockfile + global install resolution as scan) and reads symbol names plus diagnostics from stdout.
2. **No in-process pack dependencies in core** — remove `wax-lang-compose` from `wax-core`. Discover and scan share pack resolution; compose implements discover in its stdio binary via existing `discover_registry_symbols` logic.
3. **Installed pack required** — discover fails with a clear error when the locked pack is not installed locally (for example: `registry discovery requires language pack compose to be installed; run wax language install compose`). There is no in-process fallback.
4. **Per-language registry files** — each `wax registry discover --language <id>` writes only that language's registry file. When the language entry has no configured `registry`, the default path is `.wax/<language-id>.registry.json`. Multi-language discover does not merge files or require `--force` across languages; duplicate symbols across files are allowed.
5. **Config and lockfile patch on write** — when discover creates the default per-language path, it patches the loaded wax config to set `"registry": ".wax/<id>.registry.json"` on that language entry and updates `wax.lock.json` `registries[<id>]` with `{ source, sha256 }`. Dry-run prints JSON to stdout without writing or patching.
6. **Unsupported discover for other packs** — `wax-lang-basic` and `wax-lang-react` route discover requests and return `discover_unsupported` until they add heuristics. React symbol discovery remains follow-up work.

## Implementation summary

All 10 tasks shipped:

| Task | What shipped |
|------|----------------|
| Wire protocol | `DiscoverRequest`, `WirePackRequest`, `WirePackResponse`, `DiscoverUnsupported` in `wax-lang-api` |
| Subprocess runner | `SubprocessLanguageDiscoverer` in `wax-core` |
| Per-language paths | `default_registry_path_for_language` helper |
| Core orchestration | Subprocess discover, per-language writes, config/lock patch, external source rejection |
| Compose pack | `discover()` handler and unified stdio loop |
| Basic and React | Discover routing with `discover_unsupported` |
| CLI | Per-language output messages and integration tests |
| Init | Per-language registry scaffold on `wax init` |
| Documentation | This ADR, language packs spec discover section, superseded notes |
| Verification | Full engine fmt, clippy, and test suite |

## Consequences

### Positive

- Discover follows the same subprocess pack model as scan; core stays pack-agnostic.
- Multi-stack repositories can discover compose and react independently without registry collisions.
- Language packs can add discover heuristics behind one wire contract.
- Init, discover, and scan agree on per-language registry paths and lockfile entries.

### Negative / trade-offs

- Discover now requires a globally installed pack (behavior change from v1 in-process shortcut).
- React discover initially shipped deferred, then gained conservative parser-backed symbol discovery in `wax-lang-react`; Basic remains intentionally unsupported.
- `WireScanRequest` / `WireScanResponse` remain scan-only; pack stdio loops use the superset pack enums (minor duplication until a shared loop helper is extracted).

## References

- [Archived implementation plan](../plans/archive/2026-06-10-generic-registry-discovery-protocol.md)
- [Registry discovery ADR](./2026-06-04-registry-discovery.md) (v1 authoring; in-process exception superseded by this ADR)
- [Language packs spec](../specs/2026-05-16-language-packs-and-distribution.md)
- [Registry discovery design (historical)](../plans/archive/2026-06-04-registry-discovery-design.md)
