# Token Scanning Design

**Status:** Draft approved for implementation planning  
**Date:** 2026-07-03  
**Audience:** Wax engine, contract, language-pack, and reporting implementers  
**Related:** `docs/specs/2026-06-20-adoption-metrics-v2-design.md`, `docs/specs/2026-05-13-component-tracker-design.md`

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

- Suggested replacement tokens for hard-coded values.
- Fuzzy value matching, unit normalization, theme-mode matching, or cross-platform token equivalence.
- Regex or pattern-based token matching in registries.
- A token `style_context` field.
- Hard-coded styling candidates from the basic text scanner.
- Runtime telemetry or production instrumentation.

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
- React: JSX style props, CSS-in-JS object literals, clearly styled component props, and source expressions containing known token keys or aliases.
- SwiftUI: foreground/background color APIs, padding and spacing APIs, font sizing, corner radius, shadows, and source expressions containing known token keys or aliases.

### Basic Pack

The basic pack should emit only token references by exact string matching token `key` and `aliases` from its registry.

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

## Reporting

Initial CLI and report surfaces should be careful and factual:

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
5. Update CLI/reporting surfaces and docs.

This design is intended to become a follow-on implementation plan after Adoption Metrics v2 is in a stable state or explicitly allows the token fact family as the next active work item.
