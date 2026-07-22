# ADR: Token inference and reporting

**Status:** Accepted (implemented)
**Date:** 2026-07-19
**Type:** Addendum (schema-v3 inference over the existing token fact family)
**Related:** [Design spec](../specs/2026-07-19-token-inference-reporting-design.md) · [Archived implementation plan](../plans/archive/2026-07-19-token-inference-reporting-plan.md) · [Token scanning ADR](./2026-07-03-token-scanning.md)

## Context

Token scanning (order 13) retained hard-coded styling observations and known token references but treated every hard-coded observation as equivalent debt in `token_reference_ratio`. That ratio could not distinguish a `padding: 4px` literal that plainly duplicates a registered spacing token from a fixed component dimension such as `width: 200px` that has no evidence of being token-worthy. Registries also had no canonical source-facing value to compare against, so no design supported deterministic replacement suggestions.

## Decision

1. **Pack-owned context, core-owned inference** — Parser-backed packs (`compose`, `react`, `swift`) emit raw `HardcodedStyleSite` facts with a typed `StyleContext` (padding, margin, gap, width, height, size, radius, color, typography, elevation, unknown). Packs identify syntax and usage role only; they do not decide debt or rank replacements. `wax-core` performs one deterministic merged-scan inference pass — normalization, matching, confidence, evidence, and counts — after per-language facts are collected and before the merged scan is written.
2. **One optional canonical value per registry token** — `DesignSystemToken.value` is an optional source-facing string, one per language registry. A present value is validated as non-empty; an absent value is valid and means Wax cannot yet use that token for value-based inference.
3. **Four separate classifications, never combined into a score** — Every raw hard-coded observation gets exactly one `exact`, `near`, `unmatched`, or `unassessed` row. Exact and near counts remain separate from each other and from unmatched/unassessed; there is no weighted debt, health, maturity, or compliance score. A fixed unmatched dimension such as `200px` is informational evidence, not migration debt.
4. **Light context-driven confidence, not a suppression rule** — Confidence (`very_high`, `high`, `medium`, `low`) is adjusted by usage context by at most one level and falls by one more level (floor `low`) when multiple suggestions tie. Context never changes classification, suppresses a match, or removes an observation. Tied suggestions all stay visible.
5. **Deterministic, configurable numeric tolerance defaulting to `2`** — `.wax/wax.config.json` gains `token_inference.numeric_tolerance`, a finite non-negative number applied to compatible numeric scalar categories only. The default is `2`; `0` disables near matching while preserving exact matching. Colors, shadows, and composite typography values always require an exact normalized match.
6. **Schema v3 removes `token_reference_ratio`** — The scan contract advances from schema version 2 to 3. `Metrics.token_reference_ratio` is removed rather than reinterpreted, because it treated every hard-coded observation as equivalent debt regardless of registry evidence. `wax-contract` validates a bijection between raw hard-coded sites and inference rows so an inference report can never be silently incomplete.
7. **Registry value writes are reviewed, never automatic** — `wax-registry-discover` expands from component discovery into general reviewed registry maintenance. Every canonical-value proposal carries source evidence, a structured diff, and requires explicit approval before writing. The skill preserves ids, keys, aliases, categories, metadata, and existing values outside the approved diff, and it never deletes components or tokens automatically. AI-derived values affect deterministic metrics only after a user approves and persists them.

## 2026-07-22 addendum: partial canonical coverage

Category is a semantic compatibility filter, not a completeness gate. Wax still compares a spacing observation only with same-language spacing tokens, but each token with a usable canonical value participates independently. Missing or unsupported sibling values do not block exact, near, or unmatched classification; an observation is `unassessed` only when its observed value cannot be normalized or no same-category token has a usable canonical value.

Reporting states `assessed_observation_count` out of `hardcoded_observation_count`. The raw hard-coded total and its category groups are inventory, never debt; exact and near remain the only migration-candidate classes and remain separate.

## Implementation summary

All 6 tasks shipped:

| Task | What shipped | PR |
|------|----------------|-----|
| Design | Token inference and reporting design and implementation plan | [#230](https://github.com/Daio-io/wax/pull/230) |
| 1. Contract v3 | `SCHEMA_VERSION = 3`, `StyleContext`, inference types, optional `DesignSystemToken.value`, linkage/count validation, `token_reference_ratio` removal | [#231](https://github.com/Daio-io/wax/pull/231) |
| 2. Core inference | `TokenInferenceConfig`, repo-local `numeric_tolerance` (default `2`), conservative per-language normalizers, deterministic classification and confidence, `build_token_inference` | [#233](https://github.com/Daio-io/wax/pull/233) |
| 3. Pack context parity | Compose, React, and Swift style-context mapping with equivalent outcomes for shared concepts and ecosystem-capability differences (for example Compose margin) | [#235](https://github.com/Daio-io/wax/pull/235) |
| 4. Reporting | CLI confirmed/possible/unmatched/unassessed summary and ranked findings, wax-scan extractor and HTML sections joined to raw sites by `(language, site_id)` | [#236](https://github.com/Daio-io/wax/pull/236) |
| 5. Registry maintenance | `wax-registry-discover` token-value maintenance workflow with evidence, structured diff, explicit approval, and no automatic deletion | [#237](https://github.com/Daio-io/wax/pull/237) |
| 6. Documentation and closeout | README, specs, this ADR, plan archive, and full-workspace verification | [#238](https://github.com/Daio-io/wax/pull/238) |

## Consequences

### Positive

- Reports distinguish confirmed and possible migration candidates from informational unmatched observations and from registry-metadata gaps, instead of collapsing them into one ratio.
- Deterministic, typed evidence lets CLI, JSON, terminal skill output, and HTML consume identical classifications without drift.
- Optional canonical values keep existing registries valid; teams adopt value-based inference incrementally.
- Reviewed registry writes keep AI-assisted maintenance auditable and reversible before it can affect metrics.

### Negative / trade-offs

- Near matching is limited to compatible numeric scalar values in this version; colors, shadows, and composite typography values require exact normalized matches.
- A registry with no canonical values produces an expected first-run all-`unassessed` state until reviewed values are added and a fresh scan runs.
- One repo-local tolerance applies to all compatible numeric categories; category-specific overrides are deferred until real scans show a global tolerance is inadequate.
- Theme, mode, density, and platform variants of a token value remain unresolved; the contract supports only one canonical value per language registry.

## References

- [Token inference and reporting design spec](../specs/2026-07-19-token-inference-reporting-design.md)
- [Archived implementation plan](../plans/archive/2026-07-19-token-inference-reporting-plan.md)
- [Token scanning design spec](../specs/2026-07-03-token-scanning-design.md) and [ADR](./2026-07-03-token-scanning.md)
- [Registry sync and config v2 design](../specs/2026-07-04-registry-sync-config-design.md)
