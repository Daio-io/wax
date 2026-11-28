# ADR: Aspect-ratio tokens

**Status:** Accepted  
**Date:** 2026-11-28  
**Type:** Scan-facts contract (schema v5)  
**Related:** [Token inference and reporting](./2026-07-19-token-inference-reporting.md) · [Resolution evidence and callee origin](./2026-08-22-resolution-evidence-and-callee-origin.md)

## Context

Design systems can define reusable layout ratios. Tailwind exposes them through
`--aspect-*` theme tokens, and React, Compose, and SwiftUI all accept literal
ratios in their layout APIs. Wax needs a shared category to count these references
and identify literal values that match a registered token.

## Decision

- Use `aspect_ratio` as both the token category and style context. Accept
  `ratio` and `aspect-ratio` as registry input aliases. Serialize the canonical
  spelling in registries and scan output.
- Classify Tailwind `--aspect-*` tokens during React registry discovery. Detect
  literal ratios in React inline styles and Tailwind arbitrary values, Compose
  `Modifier.aspectRatio`, and SwiftUI `.aspectRatio`.
- Compare same-language canonical values as unitless numbers. Round the quotient
  or scalar to four decimal places and require equality. Aspect ratios are
  ineligible for near matching.
- Use scan schema v5 for per-language and merged facts. Include `aspect_ratio`
  in category and context counts. The insights extractor requires v5 scans and
  omits deltas for older baselines with a compatibility message.

## Scope

The category covers layout aspect ratios. WCAG contrast ratios are computed from
colors, and WCAG text-spacing multipliers belong to typography. DTCG 2025.10
provides a `number` type for unitless numeric values.
