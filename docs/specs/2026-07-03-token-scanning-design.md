# Token Scanning Design

**Status:** Accepted (implemented); superseded in part by the inference addendum below
**Date:** 2026-07-03  
**Audience:** Wax engine, contract, language-pack, and reporting implementers  
**Related:** `docs/specs/2026-06-20-adoption-metrics-v2-design.md`, `docs/specs/2026-05-13-component-tracker-design.md`, `docs/specs/2026-07-04-registry-sync-config-design.md`, [`docs/specs/2026-07-19-token-inference-reporting-design.md`](./2026-07-19-token-inference-reporting-design.md)
**Implementation plan:** [`docs/plans/archive/2026-07-03-token-scanning-plan.md`](../plans/archive/2026-07-03-token-scanning-plan.md) · [ADR](../adr/2026-07-03-token-scanning.md)

> **v1 non-goals revisited by the inference addendum:** This v1 design intentionally deferred suggested replacement tokens, a token `style_context` field, and automated registry value population (see Non-Goals below). The [token inference and reporting design](./2026-07-19-token-inference-reporting-design.md) and its [ADR](../adr/2026-07-19-token-inference-reporting.md) deliver typed `StyleContext` and deterministic exact/near/unmatched/unassessed classification against optional canonical registry `value`. They partially address registry population through a reviewed `wax-registry-discover` maintenance workflow that never writes automatically. This document is kept as-is for v1 history; it is not rewritten. The retired `token_reference_ratio` metric described below no longer appears in scan output as of schema v3.

## Summary

Wax should add token scanning as an additive fact family alongside component invocation facts. The feature should detect known design token references across all language packs and detect hard-coded styling candidates in parser-backed language packs where syntax gives enough context to keep the signal useful.

The design does not try to map hard-coded values to replacement tokens. That remains future work after Wax has reliable raw token and hard-coded styling evidence.

## Goals

- Add per-language token definitions to existing registry files.
- Emit token facts separately from component `usage_sites`.
- Support exact token key and alias matching across all current language packs.
- Emit hard-coded styling candidates only from parser-backed packs and only in styling contexts.
- Reuse existing parent attribution when a pack can identify the enclosing component, composable, or view.
- Let `wax-core` derive token counters and summaries from raw facts.
- Keep existing component invocation contracts unchanged.

## Non-Goals

- Suggested replacement tokens for hard-coded values. **Delivered by the inference addendum:** schema-v3 `token_inference` rows now suggest exact/near replacement tokens; see [Token inference and reporting](./2026-07-19-token-inference-reporting-design.md).
- Fuzzy value matching, unit normalization, theme-mode matching, or cross-platform token equivalence. **Partially delivered:** the inference addendum adds conservative, language-aware numeric normalization and near matching within a configurable tolerance; theme-mode and cross-platform equivalence remain out of scope.
- Regex or pattern-based token matching in registries. Still out of scope; token `key`/`aliases` matching remains exact.
- A token `style_context` field. **Delivered by the inference addendum:** raw `HardcodedStyleSite.context` (a typed `StyleContext`) ships in schema v3.
- Hard-coded styling candidates from the basic text scanner. Still out of scope; `basic` continues to emit token references only.
- Runtime telemetry or production instrumentation. Still out of scope.
- Automated token discovery or population of registry `tokens[]` (for example via `wax-registry-discover`); token registries are authored or synced explicitly in v1. **Partially addressed by the inference addendum:** `wax-registry-discover` expands into a reviewed token-value maintenance workflow with source evidence and explicit approval, but it never writes automatically.

## Approach

Use an additive token fact family:

- `design_system_tokens[]` for configured token definitions loaded by the language pack.
- `token_sites[]` for known token references found in source.
- `hardcoded_style_sites[]` for hard-coded styling candidates found in parser-backed scans.
- Token-specific count groups and summaries derived by `wax-core`.

This keeps token evidence separate from UI invocation evidence. Component `usage_sites[]` continues to mean UI component invocation, while token facts describe styling reference and drift evidence.

## Compatibility

This is additive to the product contract: existing component registry entries, component usage facts, and invocation metrics keep their current meanings. Existing registries with no `tokens` key remain valid and produce empty token facts.

Because `wax-contract` intentionally validates strict JSON shapes, old binaries should not be expected to deserialize scan output from newer binaries that include token fields. The implementation should still minimize disruption by making token fields default-empty in Rust structs and schemas, and by avoiding required token configuration for existing users.

## Alternatives Considered

### Token References Only

This would add token registry definitions and `token_sites[]` across all packs, but defer hard-coded candidates. It is smaller, but it delays the primary drift signal: places where source bypasses tokens.

### Tokens Inside `usage_sites[]`

This would reuse the existing component invocation array for token references. It avoids a top-level field, but it overloads `usage_sites[]` with non-invocation semantics and complicates adoption metrics.

The separate additive fact family is preferred because it follows Adoption Metrics v2's facts-first extensibility model.

## Registry Shape

Tokens live in each per-language registry because token definitions and source references differ by ecosystem.

Each token definition has:

- `id`: stable token id within the language registry.
- `key`: exact source-facing token key to match.
- `category`: token category.
- `aliases`: optional exact source-facing aliases.

Example:

```json
{
  "schema_version": 1,
  "components": [
    { "symbol": "Button" }
  ],
  "tokens": [
    {
      "id": "color.primary",
      "key": "Theme.colors.primary",
      "category": "color",
      "aliases": ["AppColors.Primary"]
    },
    {
      "id": "space.medium",
      "key": "Spacing.Medium",
      "category": "spacing"
    }
  ]
}
```

Token categories are:

- `color`
- `spacing`
- `typography`
- `radius`
- `elevation`
- `unknown`

`unknown` means the registry has a known token reference but Wax does not know its design category. It is allowed for token definitions and token references. Hard-coded candidates should use a concrete category when the styling context identifies one, with `unknown` only for deliberately included styling contexts that cannot be categorized.

## Contract Shape

Add typed contract structs rather than generic JSON extension blobs.

`DesignSystemToken`:

- `id`
- `key`
- `category`
- `aliases`

`TokenSite`:

- `id`
- `location`
- `token_id`
- `key`
- `category`
- `parent`

`HardcodedStyleSite`:

- `id`
- `location`
- `value`
- `category`
- `parent`

`TokenCategory`:

- `color`
- `spacing`
- `typography`
- `radius`
- `elevation`
- `unknown`

`TokenSite.token_id` must refer to an entry in `design_system_tokens[]`. `TokenSite.key` is the matched `key` or one of the matched token's `aliases`. `HardcodedStyleSite` does not carry a token id because Wax is not recommending replacement tokens in this version.

The existing `ParentScope` type should be reused for token facts when parent attribution is available.

## Scanner Behavior

### Parser-Backed Packs

Compose, React, and Swift should emit:

- token references from exact key or alias matches where the parser can locate the reference
- hard-coded styling candidates from styling-related syntax contexts
- parent attribution when the enclosing UI scope is known

The scanners should be conservative. It is better to miss a hard-coded candidate than to emit noisy facts for ordinary literals.

Candidate examples:

- Compose: color constructors, `Modifier.padding`, size APIs, shape or radius APIs, typography APIs, and elevation-like APIs.
- React: JSX inline `style={{ ... }}` object literals (v1). CSS-in-JS style objects and styled-component props are follow-on work after the inline-style baseline is stable.
- SwiftUI: foreground/background color APIs, padding and spacing APIs, font sizing, corner radius, shadows, and source expressions containing known token keys or aliases.

### Basic Pack

The basic pack should emit only token references by exact string matching token `key` and `aliases` from its registry.

Matching uses naive substring search per line. False positives inside longer identifiers (for example a key `primary` matching `primaryAction`) are a known v1 limitation and acceptable for the basic scanner's conservative role.

It should not emit hard-coded styling candidates. Without language context, values like `8`, `primary`, or `#fff` are too ambiguous and would create noisy reports.

## Aggregation And Metrics

`wax-core` owns derived token counts and summaries.

Initial count groups should cover:

- configured token count
- used token count
- token reference site count
- hard-coded style candidate count
- token reference counts by category
- hard-coded candidate counts by category
- parent scope count with token references
- parent scope count with hard-coded candidates

The primary convenience ratio should be:

```text
token_reference_ratio =
  token_reference_site_count /
  (token_reference_site_count + hardcoded_style_candidate_count)
```

The ratio is `null` when the denominator is zero.

Reports must label this as "token reference ratio", not "token compliance", because Wax is not proving that hard-coded values have correct replacement tokens.

> **Retired in schema v3:** `token_reference_ratio` treated every hard-coded observation as equivalent debt regardless of registry evidence. The inference addendum removes it from the contract and headline reporting in favor of separate exact/near/unmatched/unassessed counts; see [Token inference and reporting](./2026-07-19-token-inference-reporting-design.md).

## Validation

Contract validation should enforce:

- `TokenSite.token_id` references a known `DesignSystemToken.id`.
- `TokenSite.key` matches the token's `key` or one of its `aliases`.
- token categories are one of the supported enum values.
- hard-coded style sites have a non-empty `value`.
- hard-coded style sites have a category.
- hard-coded style sites do not carry token ids.
- token-derived counts and ratios match raw facts.
- parent attribution uses the existing `ParentScope` shape.

Registry validation should allow registries with no `tokens` key or an empty `tokens` array so existing component registries remain valid.

`wax validate` should reject malformed token entries when a registry defines `tokens[]` (duplicate ids, empty keys or aliases, unsupported categories). Detailed registry token validation is follow-on work and is not required for the initial token scanning implementation plan.

## Reporting

v1 reporting is CLI-only via `wax scan` terminal summary. The wax-scan HTML skill, baseline fixtures, and branded report templates are follow-on work after the contract, engine, language-pack, and CLI surfaces ship.

Initial CLI output should be careful and factual:

- show token reference counts
- show hard-coded styling candidate counts
- show counts by category
- show parent scopes with the most hard-coded candidates when attribution is available
- show the token reference ratio only with clear labeling

Reports should not imply correctness or compliance.

## Testing

Contract tests should cover:

- schema round-trip for token definitions, token sites, hard-coded style sites, counts, summaries, and ratio validation
- invalid token site references
- invalid matched keys
- hard-coded candidates that incorrectly carry token ids
- registries with no `tokens` key

Language-pack tests should cover:

- exact token key matching
- alias matching
- category preservation
- parent attribution
- parser-backed hard-coded candidates in styling contexts
- false-positive boundaries for ordinary literals
- basic pack token references only

Engine tests should cover:

- per-language token aggregation
- merged scan token aggregation
- token reference ratio math
- deterministic ordering for token summaries and report output

## Delivery Notes

The implementation should follow the active roadmap discipline:

1. Extend `wax-contract` and schemas.
2. Update registry loaders and fixtures.
3. Update `wax-core` aggregation.
4. Update language packs, keeping parser-backed semantics aligned.
5. Update CLI summary output and docs.

Token Scanning is roadmap order 13. Registry Sync and Config v2 (order 12) is complete on `main`. Do not start token scanning implementation until Adoption Metrics v2 (order 11) is complete or the maintainer explicitly promotes this plan to active work.
