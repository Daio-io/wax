# ADR: Registry discovery and skill-assisted sync

**Status:** Accepted (implemented)  
**Date:** 2026-06-04  
**Type:** Addendum (authoring-time registry workflow)  
**Related:** [Design spec](../plans/archive/2026-06-04-registry-discovery-design.md) · [Archived implementation plan](../plans/archive/2026-06-04-registry-discovery-plan.md)

## Context

After centralized registry layout shipped, teams still needed a deterministic way to bootstrap `wax.registry.json` from source. The component tracker design calls for registry authoring before scan/validate consume locked registry content. AI-assisted review should help maintainers curate discovered symbols without making AI part of scan or validate runtime.

## Decision

1. **`wax registry discover` CLI** — deterministic discovery command with `--dry-run`, `--force`, and optional `--root`; stdout stays JSON-clean in dry-run mode.
2. **Core orchestration in `wax-core`** — root resolution from Wax config, schema v1 registry JSON generation, atomic writes, overwrite refusal unless forced.
3. **Compose-first discovery** — `wax-lang-compose` discovers likely public top-level component symbols via tree-sitter inspection. **Superseded (2026-06-10):** the in-process authoring exception is removed; discover now uses the subprocess wire protocol and requires an installed pack. See [generic registry discovery protocol ADR](./2026-06-10-generic-registry-discovery-protocol.md).
4. **Skill-assisted sync** — `wax-registry-sync` Agent Skill wraps the CLI with review, diffing, validation, and lock-refresh guidance for AI-assisted workflows.
5. **Post-write guidance** — CLI prints validate/lock-refresh next steps after successful writes.

## Implementation summary

All 7 tasks shipped:

| Task | What shipped |
|------|----------------|
| Compose discovery | `discover.rs` with fixtures for public/private/duplicate symbols |
| Core orchestration | `registry_discovery.rs` with dry-run, safe writes, force overwrite |
| CLI wiring | `wax registry discover` in `wax-cli` with stdout/stderr contracts |
| Root resolution | Config-derived roots when `--root` omitted |
| Guidance | Post-write validate and lock refresh messaging |
| Agent skill | `.agents/skills/wax-registry-sync/SKILL.md` project skill |
| Verification | Full engine fmt/test/clippy, plan checkbox completion |

## Consequences

### Positive

- Teams can bootstrap registries from Compose source without hand-editing every symbol.
- Discovery is deterministic and scriptable; AI assists review, not runtime scans.
- Fits the centralized `.wax/` layout and digest-locked registry model.

### Negative / trade-offs

- v1 discovery is Compose-only; React/Swift discovery is future work.
- In-process discovery was an intentional exception to the subprocess pack protocol for authoring only; superseded by subprocess discover in the [2026-06-10 ADR](./2026-06-10-generic-registry-discovery-protocol.md).

## References

- [Archived design spec](../plans/archive/2026-06-04-registry-discovery-design.md)
- [Archived implementation plan](../plans/archive/2026-06-04-registry-discovery-plan.md)
- [Registry layout ADR](./2026-06-02-registry-sources-and-wax-layout.md)
