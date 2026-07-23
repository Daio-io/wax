# ADR: Adoption Metrics v2

**Status:** Accepted (implemented)
**Date:** 2026-06-20
**Type:** Breaking contract change (alpha)
**Related:** [Design spec](../specs/2026-06-20-adoption-metrics-v2-design.md) · [Archived implementation plan](../plans/archive/2026-06-20-adoption-metrics-v2-plan.md) · [Wax scan analytics ADR](./2026-06-14-wax-scan-analytics-skill.md)

## Context

Wax v1 scan output reported `adoption_coverage_ratio` from registry-resolved usage sites only. Wrapper-heavy applications could show 100% adoption while screens called local abstractions that internally used design-system primitives. Teams needed honest invocation-level facts, explicit counters, and reporting that separates UI invocation adoption from registry resolution.

## Decision

1. **Schema v2 cutover** — `scan-merged.json` and per-language `ScanFacts` use `schema_version: 2` with no v1 compatibility aliases in engine output.
2. **Facts first** — Language packs emit all detected UI invocations in `usage_sites[]` with `match_status` of `resolved`, `local`, `candidate`, or `unresolved`. Local definitions remain inventory in `local_components[]`.
3. **Engine-owned derived fields** — `wax-core` recomputes `counts`, `metrics`, and `symbol_usage_summary[]` from validated pack facts. Packs must not hand-author ratios.
4. **Primary metrics** — `invocation_adoption_ratio` (resolved ÷ adoption-eligible invocations; candidates excluded) and `registry_resolution_ratio` (resolved ÷ all raw invocations).
5. **Parent attribution** — Parser-backed packs attach optional `parent` scope metadata on usage sites; merged output includes parent-scope counters and symbol rollups subject to `parent_scope_limit` in scan config.
6. **Reporting alignment** — `wax scan` terminal summary, `wax-scan` extractor (`schema_version: 2` insights), HTML template labels, and fixtures use v2 terminology and counters.

## Implementation summary

| Area | What shipped |
|------|----------------|
| Contract | v2 types, validation, JSON schemas, `symbol_usage_summary[]`, grouped `counts` |
| Engine | Merge aggregation, `repo_summary`, derived metrics recompute, subprocess protocol v2 |
| Language packs | `basic` registry-only v2; `compose`, `react`, `swift` local/unresolved invocations and parent attribution |
| CLI & analytics | v2 scan summary labels; `extract-insights.sh` v2; updated fixtures and skill docs |
| Docs | ADR, changelog, README scan notes, plan completion |

## Consequences

### Positive

- Adoption reports reflect application-level UI invocations, not only registry-resolved primitives.
- Raw counters and symbol summaries support multiple reporting views without rescanning source.
- Parent-scope attribution highlights screens and wrappers that still call local UI.

### Negative / trade-offs

- Alpha breaking change: consumers of `adoption_coverage_ratio` and flat v1 `counts` must migrate to v2 fields.
- Baseline trend comparison requires v2 `scan-merged.json`; v1 baselines emit an explicit compatibility gap.
- Full graph analysis and product tagging remain future work; v2 stops at parent-scope summaries.

## References

- [Adoption Metrics v2 design spec](../specs/2026-06-20-adoption-metrics-v2-design.md)
- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md)
- [Wax scan analytics design spec](../specs/2026-06-14-wax-scan-design.md)
