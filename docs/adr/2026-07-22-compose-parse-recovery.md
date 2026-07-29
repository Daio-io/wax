# ADR: Compose parse recovery and UI scoping

**Status:** Accepted (implemented)
**Date:** 2026-07-22
**Type:** Compose language-pack recovery and scope policy (schema-compatible)
**Related:** [Design spec](../specs/2026-07-22-compose-parse-recovery-design.md) · [Archived implementation plan](../plans/archive/2026-07-22-compose-parse-recovery-plan.md)

## Context

The Compose pack depended on `tree-sitter-kotlin-ng` 1.1.0. Valid modern Kotlin constructs (suspend lambdas, `when` guards, annotated function types, explicit backing fields, context parameters/receivers, annotated type arguments) produced broad `ERROR`/`MISSING` nodes that abandoned later UI facts in the same file. The scanner also treated every PascalCase call as potential UI, so infrastructure constructors such as `MutableStateFlow(...)` polluted unresolved-component metrics. Terminal `wax scan` truncated failure diagnostics with `.take(5)` before formatting, so it could not print truthful totals or omitted counts.

## Decision

1. **Keep the pinned grammar; recover around it** — Do not replace or fork `tree-sitter-kotlin-ng` in this change. Add a byte-preserving recovery layer that normalizes the seven known syntax families, records `SyntaxRegion` metadata, and reparses bounded later-source islands for remaining broad errors.
2. **Preserve original source locations** — Every normalization mutates only non-newline bytes so line/column indexes remain valid against the original file. Fact ids and `ScanFacts` schemas stay unchanged.
3. **UI-bearing scopes own component metrics** — Emit local components, registry usages, local usages, and unresolved PascalCase calls only inside `@Composable` declarations or statically annotated composable lambdas. Nested ordinary lambdas inherit UI scope; recorded suspend-lambda bodies and explicit-backing-field initializers force component extraction to `NonUi`. Token and hard-coded-style traversal continue independently of composable scope.
4. **Status follows unresolved problems** — Fully recovered known syntax reports `Complete` with no `parse_failed`. Unknown or malformed skipped regions emit `parse_failed` and keep the scan `Partial`, even when later facts are recovered. Diagnostics use the smallest useful problem location and mention later-source recovery when islands succeed.
5. **Truthful CLI truncation** — JSON retains every diagnostic. Terminal output prints the actual total, shows at most five rows, and reports how many were omitted with a path to the merged scan JSON.
6. **Compiler and corpus gates without baking Kotlin into Rust tests** — Committed fixtures plus `compiler-matrix.tsv` are validated by an explicit `kotlinc` script and a pinned CI matrix (2.1.0–2.4.0). A maintainer corpus replay command compares sorted ids/diagnostics and runtime against a baseline without committing proprietary corpus sources. Normal Rust tests remain offline.

## Implementation summary

All 5 implementation tasks shipped after the design PR:

| Task | What shipped | PR |
|------|----------------|-----|
| Design | Compose parse recovery and UI-scoping design + implementation plan | [#240](https://github.com/Daio-io/wax/pull/240) |
| 1. Recovery model | `kotlin_recovery` metadata, precise diagnostics, `ParsedKotlinFile` passes | [#245](https://github.com/Daio-io/wax/pull/245) |
| 2. Known syntax | Byte-preserving normalizers for seven families + fixture matrix | [#246](https://github.com/Daio-io/wax/pull/246) |
| 3. UI scopes | `UiScope` walker; component facts only in UI-bearing scopes | [#247](https://github.com/Daio-io/wax/pull/247) |
| 4. Island recovery | Bounded blank-and-reparse, pass iteration, fact dedup | [#249](https://github.com/Daio-io/wax/pull/249) |
| 5. Reporting & gates | Truthful CLI truncation, kotlinc validator, corpus replay, CI matrix | [#250](https://github.com/Daio-io/wax/pull/250) |
| Closeout | ADR, plan archive, design promotion to specs, roadmap `complete` | this PR |

## Consequences

### Positive

- Compose scans tolerate known modern Kotlin and recover later clean islands after unknown gaps without panicking or aborting the pack.
- Component and unresolved-call metrics stop counting infrastructure constructors outside UI scopes.
- Operators see truthful diagnostic totals while JSON remains complete.
- Fixture compilers and CI validate the committed matrix without requiring Kotlin for ordinary Rust tests.

### Negative / trade-offs

- Byte-preserving masking is grammar-lag mitigation, not a full Kotlin frontend; future syntax may need new normalizers or a later grammar migration.
- Island recovery is capped (`min(lexical_boundary_count, 64)`) and may leave unresolved gaps that keep a file `Partial`.
- The proprietary 54-file corpus remains maintainer-operated; automated CI covers the committed fixture matrix and offline harnesses only.

## References

- [Compose parse recovery and UI scope design](../specs/2026-07-22-compose-parse-recovery-design.md)
- [Archived implementation plan](../plans/archive/2026-07-22-compose-parse-recovery-plan.md)
- PRs [#240](https://github.com/Daio-io/wax/pull/240), [#245](https://github.com/Daio-io/wax/pull/245), [#246](https://github.com/Daio-io/wax/pull/246), [#247](https://github.com/Daio-io/wax/pull/247), [#249](https://github.com/Daio-io/wax/pull/249), [#250](https://github.com/Daio-io/wax/pull/250)
