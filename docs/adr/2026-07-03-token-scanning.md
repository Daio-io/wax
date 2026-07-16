# ADR: Token scanning

**Status:** Accepted (implemented)
**Date:** 2026-07-03
**Type:** Addendum (additive token fact family)
**Related:** [Design spec](../specs/2026-07-03-token-scanning-design.md) · [Archived implementation plan](../plans/archive/2026-07-03-token-scanning-plan.md)

## Context

Wax already scanned component invocations across language packs and reported adoption metrics from those facts. Teams also needed design-token evidence: which known tokens appear in source, and where parser-backed packs can conservatively flag hard-coded styling that bypasses tokens. Modeling tokens as UI invocations would overload `usage_sites[]` and break Adoption Metrics v2's facts-first extensibility model.

## Decision

1. **Separate token fact family** — Emit `design_system_tokens[]`, `token_sites[]`, and `hardcoded_style_sites[]` alongside existing component facts. Do not model tokens as component invocations.
2. **Exact registry matching** — Match token `key` and `aliases` exactly. No regex, fuzzy value matching, unit normalization, theme-mode matching, or suggested replacements in v1.
3. **Pack roles** — Basic text scanner emits token references only. Parser-backed packs (Compose, React, Swift) also emit hard-coded styling candidates only in styling contexts, and reuse `ParentScope` when parent attribution is available.
4. **Core owns derived metrics** — Language packs emit raw facts; `wax-core` recomputes token counts, category summaries, and `token_reference_ratio`. CLI `wax scan` prints factual token counts and the ratio.
5. **Additive compatibility** — Registries without `tokens` (or with an empty array) remain valid and produce empty token facts. Existing component registry entries, usage facts, and invocation metrics keep their meanings.

## Implementation summary

All 8 tasks shipped:

| Task | What shipped |
|------|----------------|
| Shared contract | Token types, schema, validation, recomputation, merge helpers (#207) |
| Shared registry helpers | Exact key/alias parser and matcher in `wax-lang-api` (#208) |
| Basic pack | Token reference scanning; no hard-coded candidates (#210) |
| Core and CLI metrics | Merged token summaries, ratio, CLI summary lines (#211) |
| Compose pack | Token references, Compose hard-coded candidates, parent attribution (#212) |
| React pack | Token references, JSX/CSS-in-JS candidates, parent attribution (#213) |
| Swift pack | Token references, SwiftUI candidates, parent attribution (#215) |
| Docs closeout | Design cross-links, plan checkboxes, archive, and this ADR (#216) |

## Consequences

### Positive

- Token adoption and styling drift evidence sit beside component adoption without changing invocation semantics.
- Exact matching keeps scan results deterministic and registry-authored.
- Parser-backed packs share the same token fact contract while staying conservative on hard-coded candidates.

### Negative / trade-offs

- v1 reporting is CLI-only; wax-scan HTML templates and branded report updates remain follow-on work.
- `wax validate` does not yet enforce token registry authoring rules (duplicate ids, empty keys, bad categories).
- Registry discovery / skill-assisted population of `tokens[]` is out of scope; token registries are authored or synced explicitly.

## References

- [Token scanning design spec](../specs/2026-07-03-token-scanning-design.md)
- [Archived implementation plan](../plans/archive/2026-07-03-token-scanning-plan.md)
- [Adoption Metrics v2 design](../specs/2026-06-20-adoption-metrics-v2-design.md)
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md)
