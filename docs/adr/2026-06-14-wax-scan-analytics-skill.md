# ADR: Wax scan analytics skill

**Status:** Accepted (implemented)
**Date:** 2026-06-14
**Type:** Addendum (agent skill for adoption analytics)
**Related:** [Design spec](../specs/2026-06-14-wax-scan-design.md) · [Archived implementation plan](../plans/archive/2026-06-14-wax-scan-plan.md)

## Context

Wax ships deterministic `wax validate` and `wax scan` commands that produce `scan-merged.json`, but teams need actionable design-system adoption analytics without changing engine runtime. The component tracker design calls for adoption reporting; post-alpha UX may later promote summaries into `wax-core`, but alpha needed a project-scoped Agent Skill that orchestrates existing CLI output and adds labeled narrative for gaps.

## Decision

1. **`wax-scan` Agent Skill** — Project skill at `skills/wax-scan/SKILL.md` that validates config, always runs a fresh scan, extracts deterministic metrics, and produces terminal or HTML analytics reports.
2. **Skill-local extractor** — `extract-insights.sh` reads `scan-merged.json` and emits versioned insights JSON with adoption rollups, symbol frequency, fragmentation candidates, `limits[]` for unsupported metrics, and optional `--baseline` deltas.
3. **Self-contained HTML dashboard** — `templates/report.html` with embedded CSS, severity badges, inline SVG charts, and `html-escape.sh` for safe substitution of scan-derived text.
4. **No engine changes** — Skill orchestrates existing CLI; AI narrative is authoring-time only with explicit confidence labels and data-gap callouts.
5. **Fixture-based verification** — Maintainer scripts at `scripts/test-wax-scan-*.sh` and `scripts/render-wax-scan-fixture-report.sh` (not shipped with the skill install).

## Implementation summary

All 5 tasks shipped:

| Task | What shipped |
|------|----------------|
| Skill scaffold | `SKILL.md`, `reference.md`, workflow guardrails, embedded analytics spec |
| Deterministic extractor | `extract-insights.sh`; `scripts/test-wax-scan-extract-insights.sh` |
| HTML dashboard | `report.html`, placeholder contract; `scripts/render-wax-scan-fixture-report.sh` |
| Documentation | README AI skills entry, roadmap row in `docs/plans/README.md` |
| Integration smoke | `scripts/test-wax-scan-integration-smoke.sh` on compose smoke fixture; plan archived |

## Consequences

### Positive

- Teams get adoption analytics from existing scan output without waiting for post-alpha engine summaries.
- Deterministic metrics are scriptable and testable; AI fills gaps with labeled confidence.
- HTML reports work offline with no external assets.

### Negative / trade-offs

- Extractor duplicates logic that post-alpha UX may later move into `wax-core` as `json-summary`.
- Integration smoke requires `wax` on `PATH` and a populated registry (seeded from fixture after `wax init`).
- Richer charts await `graph-data` and expanded `ScanFacts` fields.

## References

- [Wax scan analytics design spec](../specs/2026-06-14-wax-scan-design.md)
- [Archived implementation plan](../plans/archive/2026-06-14-wax-scan-plan.md)
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md)
- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md)
