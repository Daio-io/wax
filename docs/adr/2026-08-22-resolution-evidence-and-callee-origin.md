# ADR: Resolution evidence and callee origin

**Status:** Accepted (implemented)
**Date:** 2026-08-22
**Type:** Scan-facts contract (schema v4) across parser-backed packs
**Related:** [Adoption Metrics v2 design](../specs/2026-06-20-adoption-metrics-v2-design.md) · [Adoption Metrics v2 ADR](./2026-06-20-adoption-metrics-v2.md) · [Archived plan stub](../plans/archive/2026-08-22-phase-2b-resolution-evidence-plan.md)

## Context

Adoption Metrics v2 classified every usage site with `match_status`, but
consumers could not tell why a site was resolved, a candidate, or unresolved.
Package mismatches lost the observed import, so reports had to reconstruct
qualified symbols by concatenating fields.

## Decision

1. **Explicit callee origin.** Every `usage_sites[]` row carries a closed
   `callee_origin`: `registry`, `local`, `framework`, `external`,
   `application`, or `unknown`. It comes from the same resolver decision as
   `match_status`.
2. **Structured resolution evidence.** Every row carries
   `resolution_evidence` with a closed `kind` (`registry_package_match`,
   `registry_name_only_legacy`, `registry_import_missing`,
   `registry_import_ambiguous`, `local_same_file`, `local_package_match`,
   `package_mismatch`, `no_matching_definition`) and an optional observed
   `package`.
3. **Derived origin counts.** Merged counts add `invocation_origins`, derived
   only from `usage_sites[]`.
4. **Pack parity.** Basic, Compose, React, and Swift emit the same fields, and
   a shared `wax-lang-api` resolution-parity test keeps their outcomes aligned.
   Scan facts move to `schema_version: 4`.

## Implementation summary

Shipped in [#293](https://github.com/Daio-io/wax/pull/293) (THE-310):
contract types and schema, core merge counts, per-pack emission and goldens,
the parity suite, and `wax-scan` extractor support for v4 input.

## Consequences

- Reports can explain each classification without re-parsing source.
- Observed import symbols survive aliasing and package mismatches.
- Consumers of scan facts must accept schema v4.
