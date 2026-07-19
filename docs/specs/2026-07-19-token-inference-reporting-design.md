# Token Inference and Reporting Design

**Status:** Proposed

**Date:** 2026-07-19

**Audience:** Wax engine, contract, language-pack, registry, and reporting implementers

**Related:** [Token scanning](./2026-07-03-token-scanning-design.md) · [Wax scan analytics](./2026-06-14-wax-scan-design.md) · [Registry sync and config v2](./2026-07-04-registry-sync-config-design.md)

**Implementation plan:** [Token inference and reporting implementation plan](../plans/2026-07-19-token-inference-reporting-plan.md)

## Summary

Wax currently treats every parser-detected hard-coded styling literal as equivalent token debt. That produces noisy reports because a fixed component dimension such as `width: 200px` contributes to the same count and ratio as `padding: 4px`, even when the registry provides no evidence that either value should use a token.

Wax should retain every hard-coded styling observation, add precise usage context, and perform a deterministic second pass that compares observed values with optional canonical token values. Exact matches become confirmed migration candidates, nearby numeric matches become possible migration candidates, unmatched values remain informational, and observations without enough registry metadata remain explicitly unassessed.

Language packs own syntax-aware observation. `wax-core` owns normalization, comparison, confidence, and derived reporting facts. The `wax-registry-discover` skill expands into a reviewed registry-maintenance workflow that can populate and refresh canonical token values when a scan exposes unassessed observations.

## Problem

The v1 token contract records `HardcodedStyleSite.value`, broad token category, location, and parent scope. It does not record whether a value came from padding, gap, width, height, size, or radius. The current `token_reference_ratio` then places every hard-coded observation in its denominator.

This causes five related problems:

1. Reports cannot explain the styling context that caused a literal to be emitted.
2. Hard-coded observations are presented as migration candidates without registry evidence.
3. Registries cannot provide canonical values for deterministic replacement suggestions.
4. Fixed component dimensions are counted as debt even when they do not resemble a registered token.
5. Replacement suggestions cannot communicate match quality or confidence.

## Goals

- Retain all parser-backed hard-coded styling observations as auditable facts.
- Emit typed usage context from Compose, React, and Swift with parity across packs.
- Add one optional canonical source value to each registered token.
- Classify every hard-coded observation as exact, near, unmatched, or unassessed.
- Keep value match quality independent from confidence.
- Use context only as supporting confidence evidence, never as a suppression rule.
- Make numeric near-match tolerance repo-local, deterministic, and configurable with a default of `2`.
- Separate confirmed, possible, informational, and unassessed results in CLI and skill reports.
- Let `wax-registry-discover` propose reviewed registry changes for missing or stale token metadata.

## Non-Goals

- Automatically rewriting application source to use suggested tokens.
- Treating every hard-coded value as token debt.
- A weighted debt, health, maturity, or compliance score.
- Near matching for colors, shadows, or composite typography values in the first version.
- Cross-unit conversion without a deterministic language-specific basis.
- Resolving theme, mode, density, or platform variants of a token value.
- Letting AI-inferred values affect deterministic metrics before a user approves and persists them.
- Automatically deleting registry components or tokens.

## Architecture

The inference pipeline has four stages:

```text
parser-backed language pack
  -> raw hard-coded sites with typed context
per-language registry
  -> tokens with optional canonical values
wax-core merged-scan inference
  -> normalized comparisons and derived classifications
CLI / scan JSON / wax-scan
  -> identical deterministic reporting facts
```

Language packs remain responsible for recognizing styling syntax and identifying the role of the relevant argument or property. They do not decide whether a hard-coded value is debt and do not rank token replacements.

`wax-core` runs inference after it has collected valid per-language facts and before it writes the merged scan. The derived inference family lives at the merged-scan level, separate from raw pack facts. This preserves the existing pack boundary: packs can be validated independently without pretending to own core-derived conclusions.

## Registry Contract

`DesignSystemToken` gains one optional field:

```text
value: Option<String>
```

Example:

```json
{
  "id": "spacing.s",
  "key": "Spacing.s",
  "category": "spacing",
  "value": "4px"
}
```

The value is the canonical source-facing representation for that language registry. Registries remain per-language, so this design does not create cross-platform token equivalence. A missing value is valid and means Wax cannot yet use that token for value-based inference.

The initial contract supports only one canonical value. Theme- or mode-specific values require contextual mode resolution and are deferred.

Registry validation should reject an explicitly present empty value. It should otherwise preserve the source string and leave category- and language-aware normalization to `wax-core`.

## Raw Observation Contract

`HardcodedStyleSite` gains a required `context` field using a typed `StyleContext` enum:

- `padding`
- `margin`
- `gap`
- `width`
- `height`
- `size`
- `radius`
- `color`
- `typography`
- `elevation`
- `unknown`

The existing `category` remains broader and continues to align observations with token categories. For example, width and gap both remain in the `spacing` category while their contexts remain distinct.

Pack mappings should follow ecosystem syntax while producing the same outcomes:

- Compose distinguishes calls such as `padding`, `size`, `width`, `height`, `spacedBy`, and radius APIs.
- React maps individual inline-style property names such as `padding`, `gap`, `width`, `height`, `fontSize`, and `borderRadius`.
- Swift distinguishes modifier names and labeled arguments such as `padding`, `spacing`, `width`, `height`, `size`, and `cornerRadius`.

When a parser recognizes a styling literal but cannot assign a more precise context, it emits `unknown`. It must not guess a context from the numeric value.

## Derived Inference Contract

The merged scan gains a core-owned `token_inference` object containing the applied configuration, summary counts, and one inference row per raw hard-coded observation.

Each inference row contains:

- `language`
- `site_id`
- `classification`: `exact`, `near`, `unmatched`, or `unassessed`
- optional `confidence`: `very_high`, `high`, `medium`, or `low`
- `suggestions[]`
- `evidence[]`

Each suggestion contains:

- `token_id`
- `token_key`
- `canonical_value`
- `match_kind`: `exact` or `near`
- optional numeric `distance`
- normalized unit when numeric

Evidence values are typed and initially include:

- `exact_value`
- `within_numeric_tolerance`
- `clear_usage_context`
- `generic_dimension_context`
- `multiple_equal_matches`
- `missing_canonical_values`
- `incomplete_canonical_coverage`
- `unsupported_canonical_format`
- `incompatible_units`
- `outside_numeric_tolerance`

Raw facts are never removed or rewritten. Derived rows refer to raw facts by stable site id. `wax-contract` validates that every inference row refers to a known hard-coded site in the named language and that suggestions refer to known tokens from the same language and category.

Exact and near rows must have at least one suggestion and a confidence value. Unmatched and unassessed rows must have no suggestions or confidence. Every suggestion's match kind must agree with its row classification.

## Matching Semantics

Inference compares a site only with tokens in the same language and category.

### Normalization

Normalization is conservative and language-aware:

- Numeric values are parsed into a scalar and unit.
- Compose units such as `dp` and `sp` remain distinct.
- React numeric values use deterministic CSS pixel semantics only for properties where React/CSS defines the number as a length. Unitless properties remain unitless.
- Swift numeric layout values remain in Swift's native scalar space.
- Incompatible units are never compared.
- Nonnumeric values receive only safe normalization, such as trimming insignificant whitespace and quotes and normalizing hexadecimal color casing.
- The first version does not infer environment-dependent conversions such as `rem` to `px`.

An unsupported observed or canonical value is not silently coerced.

### Exact and near matching

The classifier applies these rules in order:

1. Normalize every same-category token that has a canonical value.
2. If one or more normalized canonical values exactly match the observation, classify it as `exact` and retain all equal matches.
3. If no exact match exists and any same-category token lacks a usable canonical value, classify the site as `unassessed`. Missing metadata could conceal an exact or closer match.
4. Otherwise, for compatible numeric scalar values, find the smallest absolute distance.
5. If the smallest distance is greater than zero and no greater than the configured tolerance, classify the site as `near` and retain every suggestion tied at that distance.
6. If every same-category token has an assessable value but none qualify, classify the site as `unmatched`.
7. If no same-category token exists, classify the site as `unassessed` because the registry lacks the metadata needed for a value-based conclusion.

Near matching initially applies to numeric scalar values only. Colors, shadows, and composite typography values require an exact normalized match.

Keeping all equally good suggestions avoids an arbitrary token choice. It also makes registry ambiguity visible to maintainers.

## Confidence

Match quality and confidence remain separate fields. Classification states what the values did; confidence describes how strongly Wax should present a replacement suggestion.

Base confidence is adjusted by at most one level for context:

| Classification | Context | Confidence |
|---|---|---|
| Exact | Padding, margin, gap, radius, color, typography, or elevation | `very_high` |
| Exact | Width, height, size, or unknown | `high` |
| Near | Padding, margin, gap, radius, color, typography, or elevation | `medium` |
| Near | Width, height, size, or unknown | `low` |
| Unmatched or unassessed | Any | absent |

If several suggestions are tied, confidence falls by one level, with `low` as the floor. Confidence never controls whether the raw observation is retained, whether a value match exists, or whether a row can be inspected.

This intentionally gives context a light touch. A `width: 2px` exact match remains a confirmed candidate because it may represent a token-worthy divider or spacer; its generic dimension context simply prevents the strongest confidence label.

## Configuration

`.wax/wax.config.json` gains an optional top-level section:

```json
{
  "schema_version": 2,
  "token_inference": {
    "numeric_tolerance": 2
  }
}
```

`numeric_tolerance` accepts a finite non-negative JSON number and defaults to `2`. A value of `0` disables near matching while preserving exact matching. The applied tolerance is written into the merged inference output for reproducibility.

The first version uses one tolerance for all compatible numeric scalar categories. Category-specific overrides are deferred until real scans demonstrate that a global tolerance is inadequate.

CLI help, the config reference, example configs, and report documentation must explain normalization, compatible units, the default, how to disable near matching, and why increasing tolerance creates more possible candidates.

## Counts and Reporting

The core-derived inference summary contains:

- `hardcoded_observation_count`
- `assessed_observation_count`
- `exact_replacement_candidate_count`
- `near_replacement_candidate_count`
- `unmatched_observation_count`
- `unassessed_observation_count`
- candidate counts by confidence
- candidate counts by token category
- candidate counts by style context

The counts must reconcile with the inference rows. Exact and near counts remain separate; Wax does not combine them into a weighted debt score.

Counts are site counts, not suggestion counts. `assessed_observation_count` equals exact plus near plus unmatched observations, and `hardcoded_observation_count` equals assessed plus unassessed observations.

CLI and `wax-scan` reports should present:

1. Known token references.
2. Confirmed migration candidates from exact matches.
3. Possible migration candidates from near matches.
4. Unmatched observations as informational evidence, not debt.
5. Unassessed observations as a registry-metadata gap.
6. Ranked suggestions with source location, usage context, observed value, canonical token and value, distance, confidence, and evidence.

The current `token_reference_ratio` is removed from headline reporting. Because it treats every hard-coded observation as equivalent debt, scan contract v3 should remove the metric rather than silently change its denominator. The replacement contract deliberately exposes factual counts without introducing a new composite ratio.

Fixed dimensions are therefore retained but discounted correctly: they appear as confirmed or possible candidates only when registry values support that conclusion. Otherwise they remain unmatched informational observations or unassessed metadata gaps.

## Registry Maintenance Skill

`wax-registry-discover` remains the user-facing skill name and expands from component discovery into general reviewed registry maintenance. It can be invoked directly or delegated to by `wax-scan` when unassessed observations reveal missing canonical values.

The skill should:

1. Resolve the design-system repository and language roots using existing config and remembered-design-system behavior.
2. Run deterministic component discovery in preview mode first.
3. Inspect token declarations and source assignments.
4. Propose missing token entries, missing canonical values, stale canonical values, and component registry changes.
5. Attach source evidence to every AI-inferred token change.
6. Show a structured diff of component and token additions, changes, and potential removals.
7. Require explicit approval before writing.
8. Preserve ids, keys, aliases, categories, metadata, and existing values unless a replacement is explicitly approved.
9. Never delete components or tokens automatically.
10. Write atomically, run `wax validate`, sync the consuming app when applicable, and offer to re-run the originating scan.

AI-derived values remain authoring suggestions. They become deterministic scan inputs only after the user approves them and they are written to the registry.

## Error Handling

- Registries without `value` remain valid and produce unassessed observations where comparison is impossible.
- Explicit empty canonical values fail registry validation.
- Unsupported canonical formats produce `unassessed` with evidence rather than false unmatched results.
- Incompatible units cannot become exact or near suggestions.
- Invalid or negative tolerance configuration fails config validation before scanning.
- Ambiguous equal matches remain visible and lower confidence.
- Arithmetic and parsing failures must not produce partial suggestions; affected rows become unassessed with typed evidence.
- Inference rows and summary counts use deterministic ordering.

## Compatibility and Versioning

- Existing registry files remain valid because token `value` is optional.
- Existing config files remain valid because `token_inference` is optional and defaults are deterministic.
- Older binaries may reject registries or configs containing the new optional fields because Wax contracts reject unknown fields; new binaries continue to accept old files.
- Parser-backed language packs must add `StyleContext` in parity and ship against the same contract version.
- The scan facts/merged output contract advances from schema version 2 to 3 because hard-coded sites change shape and `token_reference_ratio` is retired.
- The registry and config schema versions remain unchanged because their new fields are optional additions; schemas, fixtures, documentation, and strict validators still require updates.
- Basic continues to emit token references only and therefore emits no hard-coded contexts or inference rows.

## Testing

### Contract and schema

- Round-trip canonical token values, style contexts, inference rows, suggestions, evidence, and counts.
- Reject invalid site/token/category links, invalid confidence/classification combinations, and inconsistent summaries.
- Verify old registries and configs without the new optional fields remain valid.
- Verify scan schema v2 is rejected where v3 is required rather than interpreted with new semantics.

### Core inference

- Cover exact, near, unmatched, unassessed, tied, unsupported-format, and incompatible-unit cases.
- Use table-driven normalization tests for Compose, React, and Swift representations.
- Verify default tolerance `2`, custom tolerance, and exact-only tolerance `0`.
- Verify deterministic suggestion ranking and count reconciliation.
- Verify context adjusts confidence only and never changes classification.

### Language packs

- Add equivalent context fixtures for Compose, React, and Swift.
- Cover padding, gap or spacing, width, height, size, radius, color, typography, elevation, and unknown fallbacks where the ecosystem supports them.
- Verify fixed dimensions remain present as raw observations.
- Preserve preview/test exclusions and existing false-positive boundaries.

### Reporting and skills

- Verify exact, near, unmatched, and unassessed sections remain separate in terminal and HTML output.
- Verify unmatched and unassessed observations do not appear as token debt.
- Verify report rows include context, evidence, distance, and confidence.
- Test registry refresh dry runs, reviewed writes, preservation rules, validation, sync, and scan reruns.
- Update representative merged-scan fixtures, extractor goldens, report rendering tests, and screenshots.

## Delivery Sequence

This work should be delivered as a new roadmap plan after the current active-plan gates permit it or the maintainer explicitly promotes it:

1. Contract v3, registry value, config, schemas, and fixtures.
2. Core normalization, inference, validation, and counts.
3. Compose, React, and Swift context parity.
4. CLI and `wax-scan` deterministic reporting.
5. `wax-registry-discover` token-value maintenance and delegated refresh flow.
6. Documentation closeout and ADR.

Each implementation task remains one focused PR, and cross-language behavior ships only when all parser-backed packs can express the same contract.
