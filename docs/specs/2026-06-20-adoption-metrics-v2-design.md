# Adoption Metrics v2 Design

**Status:** Accepted (implemented)  
**Date:** 2026-06-20  
**Audience:** Wax engine, contract, language-pack, and reporting implementers  
**Implementation plan:** [Archived Adoption Metrics v2 implementation plan](../plans/archive/2026-06-20-adoption-metrics-v2-plan.md)  
**Related:** [Language packs and distribution](./2026-05-16-language-packs-and-distribution.md), [Wax scan analytics skill](./2026-06-14-wax-scan-design.md), [Component tracker design](./2026-05-13-component-tracker-design.md)

## Summary

Wax currently reports adoption from registry-resolved primitive usage sites. That makes component-based applications look healthier than they are: a screen can call local wrappers everywhere while those wrappers call design-system primitives internally, and the scan can still show 100% adoption.

Adoption Metrics v2 changes the contract from "one hero ratio" to "facts first, metrics second." Language packs should emit every detected UI invocation, classify each invocation as design-system, local, candidate, or unresolved, and attach optional parent-scope attribution. The engine should preserve raw counters and derived summaries so reporting layers can choose honest decision metrics without reverse-engineering thousands of call sites.

The core rule is:

> Definitions are inventory. Invocations are adoption evidence.

## Goals

- Add a schema v2 scan contract for language-neutral UI invocation facts.
- Track all detected UI invocations in parser-backed packs, not only registry-resolved primitive invocations.
- Preserve raw repo-level counters, language-level counters, and per-symbol usage summaries in scan output.
- Add parent-scope attribution so reports can identify which screens, views, or components invoke local UI.
- Keep file and line locations for navigation while avoiding location-based trend identity where semantic identity is available.
- Let reporting layers decide which derived metrics to display from raw facts and counters.
- Treat alpha contract changes as breaking when that keeps the output simpler and more honest.

## Non-Goals

- Runtime telemetry or production instrumentation.
- Automatic source migrations.
- Perfect rename or move tracking across all ecosystems.
- Product-specific tagging workflows for owners, teams, routes, or feature labels.
- Full graph analysis in v2.0 beyond parent-scope summaries derived from usage sites.

## Problem

In v1, parser-backed language packs usually emit `usage_sites` only when an invocation matches the design-system registry. Local component definitions are emitted as `local_components`, but local invocations are mostly absent from `usage_sites`.

That creates a misleading denominator:

```text
adoption_coverage_ratio = resolved_count / usage_site_count
```

If `usage_site_count` excludes calls to local wrappers, wrapper-heavy code can appear fully adopted. The scan result contains raw primitive usage, but not enough application-level invocation facts to answer:

- Which local UI abstractions are called most?
- Which parent scopes still invoke local UI?
- How much of the app surface calls design-system UI directly?
- How much of the design-system registry is represented at all?

## Contract Principle

The scan result is a facts contract, not a single opinionated adoption score.

The engine and packs should emit:

- `usage_sites[]`: lossless detected UI invocation events.
- `local_components[]`: local definition inventory.
- `counts`: raw counters grouped by registry, definitions, raw invocations, adoption eligibility, and parent scopes.
- `symbol_usage_summary[]`: per-callee summaries derived from `usage_sites[]`.
- `metrics`: convenience ratios computed from explicit numerator and denominator counters.

Consumers should use `usage_sites[]` for call-site detail, `symbol_usage_summary[]` for component-level counters, and `counts` for repo/language rollups.

## Derived Field Ownership

Language packs own raw extraction facts. The engine owns derived counters, metrics, and summaries.

| Surface | Owner | Notes |
|---------|-------|-------|
| `design_system_components[]` | Language pack | Registry entries loaded for that language. |
| `local_components[]` | Language pack | Local definition inventory. |
| `usage_sites[]` | Language pack | Raw invocation facts, including parent attribution when enabled. |
| Per-language `counts` | Engine | Recomputed from raw facts after pack output is validated. |
| Per-language `metrics` | Engine | Recomputed from `counts`; packs should not hand-author ratios. |
| Per-language `symbol_usage_summary[]` | Engine | Derived from that language's `usage_sites[]` and config limits. |
| Merged `repo_summary` | Engine | Sum counters across languages and recompute ratios. |
| Merged `symbol_usage_summary[]` | Engine | Derived from all language summaries or directly from all usage sites. |

Implementation should follow a `recompute_derived()` pattern: deserialize and validate raw pack facts, recompute all derived fields, then write per-language and merged artifacts. This keeps parser packs focused on extraction and prevents inconsistent summary math.

## Core Concepts

| Term | Meaning |
|------|---------|
| UI invocation | A syntactic call/site where application code uses a UI abstraction such as a component, composable, view, element factory, or JSX element. |
| Callee | The symbol referenced by the invocation. |
| DS registry | Configured design-system component registry for a language. |
| Local definition | A UI abstraction defined in scanned source that is not a DS registry entry. |
| Parent scope | Innermost enclosing UI declaration that contains the invocation. |
| Raw invocation | A detected UI invocation event before report-level filtering. |
| Symbol usage summary | Derived per-callee counter row grouped from `usage_sites[]`. |
| Invocation adoption | Resolved design-system invocations divided by adoption-eligible invocations. |
| Registry resolution | Registry-resolved invocations divided by all detected UI invocations. |

## Match Status

Every detected UI invocation must have exactly one `match_status`.

| Status | Meaning | Primary adoption denominator | Primary adoption numerator |
|--------|---------|------------------------------|----------------------------|
| `resolved` | Callee matches a DS registry entry. | Included | Included |
| `local` | Callee matches a scanned local UI definition. | Included | Excluded |
| `candidate` | Callee may be DS but needs review because import, alias, or package evidence is ambiguous. | Excluded by default | Excluded |
| `unresolved` | Invocation has UI shape, but callee is neither registry nor local. | Included | Excluded |

Candidate policy is explicit. v2 defaults to reporting candidates separately rather than counting them as DS or non-DS adoption.

## Type and Resolution Dictionary

Every new enum-like field must be documented in schemas, Rust API docs, and report references with these meanings.

### `match_status`

`match_status` describes how one UI invocation resolved.

| Value | Brief description |
|-------|-------------------|
| `resolved` | The invocation resolved to a configured design-system registry component. |
| `local` | The invocation resolved to a UI definition declared in the scanned repository. |
| `candidate` | The invocation may refer to a design-system component, but import/package/alias evidence is ambiguous and needs review. |
| `unresolved` | The invocation has UI shape, but Wax could not match it to the registry or local definition catalog. |

### `symbol_kind`

`symbol_kind` describes what a `symbol_usage_summary[]` row represents after grouping usage sites.

| Value | Brief description |
|-------|-------------------|
| `registry` | A grouped design-system registry symbol with resolved invocations. |
| `local` | A grouped in-repository UI symbol with local invocations. |
| `candidate` | A grouped symbol with candidate design-system invocations that should be reviewed separately. |
| `unresolved` | A grouped UI-shaped symbol that remains unmatched after registry and local resolution. |

### `identity_stability`

`identity_stability` tells trend consumers how durable an ID is expected to be.

| Value | Brief description |
|-------|-------------------|
| `semantic` | Based on language identity such as package/module plus declaration; file moves should usually preserve it. |
| `path_sensitive` | Based partly or fully on file/module path; moving files may create remove/add trend churn. |
| `scan_local` | Stable only inside one scan result; do not use as a long-term trend key. |

### `candidate_policy`

`candidate_policy` controls how candidate invocations affect primary adoption.

| Value | Brief description |
|-------|-------------------|
| `report_separately` | Default. Exclude candidates from the primary adoption numerator and denominator, and expose candidate counters separately. |
| `count_as_non_adopted` | Include candidates in the primary denominator but not the numerator. Reserved for stricter teams. |
| `count_as_adopted` | Include candidates in numerator and denominator. Not recommended; reports must label the policy because this can overstate adoption. |

Candidate policy formulas:

| Policy | `eligible_invocation_count` | `adopted_invocation_count` |
|--------|-----------------------------|----------------------------|
| `report_separately` | `resolved + local + unresolved` | `resolved` |
| `count_as_non_adopted` | `resolved + local + unresolved + candidate` | `resolved` |
| `count_as_adopted` | `resolved + local + unresolved + candidate` | `resolved + candidate` |

### `parent_scope_limit`

`parent_scope_limit` controls how many per-symbol parent rows are emitted.

| Value | Brief description |
|-------|-------------------|
| `null` or omitted | Emit every discovered parent scope row. |
| `0` | Emit no parent rows, but preserve aggregate `parent_scope_count`. |
| Positive integer | Emit up to N parent rows per symbol, sorted by invocation count descending. |

## New Output Key Dictionary

These fields are new or newly clarified in v2. Schemas and public Rust docs should carry equivalent one-line descriptions.

| Key | Brief description |
|-----|-------------------|
| `qualified_symbol` | Best-effort semantic symbol identity, such as package/module plus declaration name. |
| `local_definition_id` | ID of the matching `local_components[]` definition for a `local` invocation. |
| `parent` | Parent-scope object attached to a usage site when parent attribution is enabled. |
| `parent_id` | Grouping key for a parent scope; prefer semantic identity over file path. |
| `scope_kind` | Language-defined parent category such as `composable`, `view`, or `component`. |
| `identity_basis` | Human-readable explanation of how an ID was built, such as `registry_id` or `package_qualified_symbol`. |
| `registry.component_count` | Number of configured design-system registry components for the language or merged scan. |
| `registry.used_component_count` | Number of distinct registry components with at least one resolved invocation. |
| `registry.resolved_raw_invocation_count` | Raw count of resolved design-system invocations. |
| `registry.candidate_raw_invocation_count` | Raw count of candidate design-system invocations. |
| `definitions.local_definition_count` | Number of local UI definitions discovered in source. |
| `definitions.invoked_local_definition_count` | Number of local definitions with at least one `local` invocation. |
| `definitions.unused_local_definition_count` | Number of local definitions with no local invocations. |
| `raw_invocations.total` | Count of all detected UI invocations across statuses. |
| `raw_invocations.resolved` | Count of invocations with `match_status: "resolved"`. |
| `raw_invocations.local` | Count of invocations with `match_status: "local"`. |
| `raw_invocations.candidate` | Count of invocations with `match_status: "candidate"`. |
| `raw_invocations.unresolved` | Count of invocations with `match_status: "unresolved"`. |
| `adoption.eligible_invocation_count` | Denominator for primary invocation adoption after candidate policy is applied. |
| `adoption.adopted_invocation_count` | Numerator for primary invocation adoption; resolved invocations by default. |
| `adoption.non_adopted_invocation_count` | Adoption-eligible invocations that are not counted as adopted. |
| `parent_scopes.total` | Number of unique parent scopes found in attributed usage sites. |
| `parent_scopes.with_resolved_invocations` | Number of parent scopes containing at least one resolved invocation. |
| `parent_scopes.with_local_invocations` | Number of parent scopes containing at least one local invocation. |
| `parent_scopes.with_unresolved_invocations` | Number of parent scopes containing at least one unresolved invocation. |
| `invocation_adoption_ratio` | Primary adoption ratio from explicit adoption numerator and denominator counters. |
| `registry_resolution_ratio` | Resolved raw invocations divided by all raw invocations. |
| `symbol_usage_summary[]` | Derived per-callee summary rows grouped from `usage_sites[]`. |
| `symbol_id` | Normalized callee grouping key for a symbol summary row. |
| `raw_invocation_count` | Number of usage sites represented by a symbol summary row. |
| `parent_scope_count` | Number of unique parent scopes represented by a symbol summary row, regardless of row limit. |
| `file_count` | Number of files containing invocations represented by a symbol summary row. |
| `parent_scopes[]` | Complete or limited parent-scope rows for a symbol summary. |
| `invocation_count` | Number of invocations for one parent scope inside a symbol summary. |
| `parent_scopes_truncated` | Whether `parent_scopes[]` omits rows because a limit was applied. |

## UI Invocation Detection Boundary

Parser-backed packs must define a conservative UI invocation detector for their ecosystem. This prevents unresolved counts from ballooning because ordinary helpers, constructors, modifiers, or test utilities were treated as UI.

Baseline rules:

- Compose: `@Composable` function calls and composable-looking call expressions inside configured source roots.
- SwiftUI: `View` initializers, `View` body expressions, and `@ViewBuilder` calls; modifier chains should attribute to the underlying view/call rather than count every modifier as an adoption candidate.
- React: JSX component elements and known component factory calls; intrinsic lowercase elements such as `<div>` are not DS/local adoption evidence.
- Basic text scanner: remains registry-only and should not claim v2 local/unresolved semantics until it has a language-aware UI detector.

Each parser-backed pack should document ecosystem exclusions in its tests. Examples include previews, tests, generated sources, declaration files, and framework-native controls when they are outside the configured registry and not intended to be local DS adoption signals.

### Basic Pack Cutover

`wax-lang-basic` must still emit schema v2 after the alpha cutover, but it should stay honest about its lower-fidelity extraction model:

- It may emit only registry-backed `resolved` or `candidate` usage sites.
- It should not emit `local` or `unresolved` invocation facts until it has a language-aware local-definition and UI-shape detector.
- Its derived counts should set local and unresolved invocation counts to zero because those facts were not collected.
- It should emit diagnostics or capability flags that let reports show data gaps for local invocations, unresolved invocations, and parent attribution.

This keeps all packs on one JSON schema without pretending the text scanner has parser-backed adoption semantics.

## Parent Attribution

Parent attribution links a usage site to the innermost enclosing UI scope, not to slot hosts.

Examples:

- Compose parent: nearest enclosing `@Composable` function declaration.
- SwiftUI parent: enclosing `View` type `body` or `@ViewBuilder` function.
- React parent: enclosing function/class/component body containing the JSX element.

Slot/content lambda rule:

```kotlin
@Composable
fun DiscoverScreen() {
    Tier {
        Button()
        Tier { Button() }
        EpisodeCard()
    }
}
```

All calls above have parent `DiscoverScreen`, not `Tier`. `Tier` is a callee, not the owner of the lambda body for adoption attribution.

Parent attribution should be implemented for Compose in the first v2 task because it is needed for useful local-wrapper reports. React and SwiftUI should follow the same semantics before their v2 facts are considered complete.

## Identity and Locations

Locations are navigational metadata, not durable trend identity.

IDs should prefer semantic language identity over file paths:

| Ecosystem | Preferred parent identity |
|-----------|---------------------------|
| Compose | Package-qualified composable declaration, such as `compose:composable:com.example.discover.DiscoverScreen`. |
| SwiftUI | Module-qualified view or builder declaration, such as `swiftui:view:App.DiscoverView`. |
| React | Export or import-resolved module identity when available; otherwise path-sensitive module plus component name. |

When a path-sensitive identity is unavoidable, emit enough metadata for consumers to treat trends carefully:

```json
{
  "parent_id": "react:component:src/features/discover/DiscoverScreen",
  "identity_basis": "module_path_export",
  "identity_stability": "path_sensitive"
}
```

Allowed `identity_stability` values:

| Value | Meaning |
|-------|---------|
| `semantic` | File moves should not change the ID when package/module declarations remain stable. |
| `path_sensitive` | File moves may produce remove/add trend churn. |
| `scan_local` | Stable only within one scan result. |

v2 does not guarantee perfect stability across renames or path-sensitive module moves. Aggregate counters remain valid when a move preserves invocations; identity-level trend reports may show churn.

## Usage Site v2

`usage_sites[]` remains the lossless event stream. v2 extends it with local matching and optional parent attribution:

```json
{
  "id": "usage.compose:src/discover/Discover.kt:42:13:EpisodeCard",
  "location": {
    "file": "src/discover/Discover.kt",
    "line": 42,
    "column": 13
  },
  "symbol": "EpisodeCard",
  "qualified_symbol": "com.example.discover.EpisodeCard",
  "match_status": "local",
  "registry_symbol": null,
  "local_definition_id": "local.compose:com.example.discover.EpisodeCard",
  "parent": {
    "parent_id": "compose:composable:com.example.discover.DiscoverScreen",
    "symbol": "DiscoverScreen",
    "qualified_symbol": "com.example.discover.DiscoverScreen",
    "scope_kind": "composable",
    "identity_basis": "package_qualified_symbol",
    "identity_stability": "semantic",
    "location": {
      "file": "src/discover/Discover.kt",
      "line": 38,
      "column": 1
    }
  }
}
```

Field rules:

| Field | Required | Notes |
|-------|----------|-------|
| `id` | Yes | Stable within one scan. May be location-based because it represents one occurrence. |
| `location` | Yes | Navigation only. Repo-relative file, one-based line/column. |
| `symbol` | Yes | Source-level callee text. |
| `qualified_symbol` | Best effort | Semantic callee identity when language can provide it. |
| `match_status` | Yes | `resolved`, `local`, `candidate`, or `unresolved`. |
| `registry_symbol` | If `resolved` or `candidate` | Canonical registry symbol. |
| `local_definition_id` | If `local` | Links to `local_components[].id`. |
| `parent` | When parent attribution is enabled | Omitted when disabled or unknown. |

## Local Definitions

`local_components[]` remains definition inventory. Unused local definitions must not affect invocation adoption.

```json
{
  "id": "local.compose:com.example.discover.EpisodeCard",
  "symbol": "EpisodeCard",
  "qualified_symbol": "com.example.discover.EpisodeCard",
  "identity_basis": "package_qualified_symbol",
  "identity_stability": "semantic",
  "location": {
    "file": "src/discover/EpisodeCard.kt",
    "line": 12,
    "column": 1
  }
}
```

## Counts

v2 should expose raw counters by purpose rather than forcing consumers to infer denominators from ratios.

```json
{
  "counts": {
    "registry": {
      "component_count": 71,
      "used_component_count": 44,
      "resolved_raw_invocation_count": 831,
      "candidate_raw_invocation_count": 5
    },
    "definitions": {
      "local_definition_count": 166,
      "invoked_local_definition_count": 72,
      "unused_local_definition_count": 94
    },
    "raw_invocations": {
      "total": 1055,
      "resolved": 831,
      "local": 200,
      "candidate": 5,
      "unresolved": 19
    },
    "adoption": {
      "eligible_invocation_count": 1050,
      "adopted_invocation_count": 831,
      "non_adopted_invocation_count": 219
    },
    "parent_scopes": {
      "total": 84,
      "with_resolved_invocations": 79,
      "with_local_invocations": 31,
      "with_unresolved_invocations": 5
    }
  }
}
```

Rules:

- `raw_invocations.total = resolved + local + candidate + unresolved`.
- `adoption.eligible_invocation_count = resolved + local + unresolved` by default.
- `adoption.adopted_invocation_count = resolved`.
- `registry.resolved_raw_invocation_count` is the raw DS primitive invocation counter.
- Merged scans sum counts across languages and recompute ratios. They must never average per-language percentages.

## Merged Scan Shape

Repo-level totals should live on the `MergedScan` root so consumers do not need to sum every language before rendering common reports.

```json
{
  "schema_version": 2,
  "recorded_at": "2026-06-20T12:00:00Z",
  "repo_summary": {
    "languages": ["compose", "react", "swift"],
    "counts": { "...": "same count groups as language facts" },
    "metrics": { "...": "same metric fields as language facts" }
  },
  "symbol_usage_summary": [],
  "languages": {
    "compose": { "...": "per-language ScanFacts" }
  }
}
```

Rules:

- `repo_summary.counts` sums compatible raw counters across all languages.
- `repo_summary.metrics` recomputes ratios from repo-level counters.
- Root `symbol_usage_summary[]` groups by language-qualified `symbol_id`; identical source names from different languages remain separate unless they share a registry id.
- Consumers may still inspect `languages.<id>.counts` for per-language reporting, but root summary is authoritative for repo-level numbers.

## Metrics

Metrics are derived conveniences over explicit counters:

```json
{
  "metrics": {
    "invocation_adoption_ratio": 0.791,
    "registry_resolution_ratio": 0.787
  }
}
```

Definitions:

```text
invocation_adoption_ratio =
  adoption.adopted_invocation_count / adoption.eligible_invocation_count

registry_resolution_ratio =
  raw_invocations.resolved / raw_invocations.total
```

When a denominator is zero, the ratio is `null`.

Reporting labels must distinguish:

- UI invocation adoption
- Registry resolution
- Raw DS invocations
- Local definitions in repo
- Unresolved UI calls

Reports must not present registry resolution as unqualified "design system coverage."

## Symbol Usage Summary

`symbol_usage_summary[]` is a derived index grouped by normalized callee identity. It should include registry symbols, local symbols, candidate symbols, and unresolved UI-shaped symbols.

```json
{
  "symbol_usage_summary": [
    {
      "symbol_id": "compose:registry:ds.button",
      "symbol": "Button",
      "qualified_symbol": "com.acme.design.Button",
      "symbol_kind": "registry",
      "match_status": "resolved",
      "registry_symbol": "ds.button",
      "local_definition_id": null,
      "identity_basis": "registry_id",
      "identity_stability": "semantic",
      "raw_invocation_count": 312,
      "parent_scope_count": 48,
      "file_count": 27,
      "parent_scopes": [
        {
          "parent_id": "compose:composable:com.example.discover.DiscoverScreen",
          "symbol": "DiscoverScreen",
          "qualified_symbol": "com.example.discover.DiscoverScreen",
          "scope_kind": "composable",
          "identity_basis": "package_qualified_symbol",
          "identity_stability": "semantic",
          "invocation_count": 6,
          "location": {
            "file": "src/discover/Discover.kt",
            "line": 38,
            "column": 1
          }
        }
      ],
      "parent_scope_limit": null,
      "parent_scopes_truncated": false
    },
    {
      "symbol_id": "compose:local:com.example.discover.EpisodeCard",
      "symbol": "EpisodeCard",
      "qualified_symbol": "com.example.discover.EpisodeCard",
      "symbol_kind": "local",
      "match_status": "local",
      "registry_symbol": null,
      "local_definition_id": "local.compose:com.example.discover.EpisodeCard",
      "identity_basis": "package_qualified_symbol",
      "identity_stability": "semantic",
      "raw_invocation_count": 42,
      "parent_scope_count": 12,
      "file_count": 9,
      "parent_scopes": [],
      "parent_scope_limit": 0,
      "parent_scopes_truncated": true
    }
  ]
}
```

Field rules:

| Field | Required | Notes |
|-------|----------|-------|
| `symbol_id` | Yes | Normalized callee identity. Prefer registry id or semantic local identity. |
| `symbol_kind` | Yes | `registry`, `local`, `candidate`, or `unresolved`. |
| `match_status` | Yes | Dominant or exact status for the grouped symbol. A symbol with mixed statuses should split into separate rows by normalized identity and status. |
| `raw_invocation_count` | Yes | Count of matching `usage_sites[]`. |
| `parent_scope_count` | Yes | Count of unique parent scopes, even when `parent_scopes` rows are suppressed. |
| `file_count` | Yes | Count of files with matching invocations. |
| `parent_scopes` | Yes | Complete or limited parent rows, depending on config. |
| `parent_scope_limit` | Yes | `null`, `0`, or positive integer after config resolution. |
| `parent_scopes_truncated` | Yes | `true` when not all rows are emitted. |

Unresolved grouping should use the strongest identity available:

1. Language-qualified or import-resolved symbol identity.
2. Module plus symbol when available.
3. Language plus source symbol as a fallback.

If the same unresolved symbol appears in unrelated modules and cannot be distinguished semantically, grouping by language plus source symbol is acceptable but should use `identity_stability: "scan_local"` or `path_sensitive` as appropriate.

## Parent Scope Limits

By default, v2 emits all parent scope rows in `symbol_usage_summary[]`.

Config:

```json
{
  "adoption": {
    "symbol_usage_summary": {
      "enabled": true,
      "parent_scope_limit": null
    }
  }
}
```

Semantics:

| Value | Meaning |
|-------|---------|
| `null` or omitted | Emit all parent scopes. |
| `0` | Suppress per-parent rows while preserving `parent_scope_count`. |
| Positive integer | Emit up to N parent rows per symbol, sorted by invocation count descending. |

When a limit suppresses rows, `parent_scopes_truncated` must be `true`.

Parent rows sort by:

1. `invocation_count` descending.
2. `parent_id` ascending.

Symbols sort by:

1. `raw_invocation_count` descending.
2. `symbol_kind` ascending.
3. `symbol_id` ascending.

## Language Config

All parser-backed packs should use the same config semantics:

```json
{
  "id": "compose",
  "enabled": true,
  "registry": ".wax/compose.registry.json",
  "roots": ["feature/**/src/main/kotlin"],
  "adoption": {
    "track_local_invocations": true,
    "track_unresolved_invocations": true,
    "parent_attribution": {
      "enabled": true,
      "scope_visibility": ["public", "internal", "private"]
    },
    "candidate_policy": "report_separately",
    "symbol_usage_summary": {
      "enabled": true,
      "parent_scope_limit": null
    }
  }
}
```

v2 introduces only the nested `adoption` block above. It does not introduce a generic `exclude` key; packs should continue using their existing root discovery, file-extension, generated-source, and test/preview filtering behavior until a separate config plan standardizes exclusions.

Defaults:

| Key | Default |
|-----|---------|
| `track_local_invocations` | `true` for parser-backed packs in v2. |
| `track_unresolved_invocations` | `true` when the pack has a conservative UI detector. |
| `parent_attribution.enabled` | `true` for v2-complete parser-backed packs. |
| `candidate_policy` | `report_separately`. |
| `symbol_usage_summary.enabled` | `true`. |
| `symbol_usage_summary.parent_scope_limit` | `null`. |

Disable behavior:

| Config | When false or zero | Required reporting behavior |
|--------|--------------------|-----------------------------|
| `track_local_invocations: false` | Packs still emit `local_components[]`, but local invocation usage sites are not emitted. | Reports must show a local-invocation data gap; invocation adoption is partial. |
| `track_unresolved_invocations: false` | Packs omit unresolved invocation usage sites. | Reports must show an unresolved-invocation data gap; adoption denominator excludes unknown UI calls. |
| `parent_attribution.enabled: false` | Usage sites omit `parent`; parent scope counts are zero or absent. | Reports must hide parent-scope rankings and show a parent-attribution data gap. |
| `symbol_usage_summary.enabled: false` | Engine omits `symbol_usage_summary[]` while preserving raw `usage_sites[]` and aggregate `counts`. | Reports must derive summaries themselves or show a summary data gap. |
| `symbol_usage_summary.parent_scope_limit: 0` | Engine emits symbol rows with `parent_scope_count`, empty `parent_scopes[]`, and `parent_scopes_truncated: true` when parents exist. | Reports may show breadth counts but not parent names. |

`candidate_policy` affects only adoption counters and metrics. Raw candidate invocation counts are always preserved when candidate sites are emitted.

## Worked Example

```kotlin
@Composable
fun DiscoverScreen() {
    EpisodeCard()
    EpisodeCard()
}

@Composable
fun EpisodeCard() {
    Tier { BodyText("title") }
}
```

Expected v2 facts:

| Invocation | Status | Parent |
|------------|--------|--------|
| `EpisodeCard()` x2 | `local` | `DiscoverScreen` |
| `Tier()` | `resolved` | `EpisodeCard` |
| `BodyText()` | `resolved` | `EpisodeCard` |

Counts:

```json
{
  "raw_invocations": {
    "total": 4,
    "resolved": 2,
    "local": 2,
    "candidate": 0,
    "unresolved": 0
  },
  "adoption": {
    "eligible_invocation_count": 4,
    "adopted_invocation_count": 2,
    "non_adopted_invocation_count": 2
  }
}
```

`invocation_adoption_ratio = 0.5`. The scan no longer reports 100% adoption just because local wrappers call DS primitives internally.

## Fact Extensibility

v2 should make it cheap to collect more raw facts later. The guiding rule is to preserve evidence before deciding which product metric matters.

Future fact families should follow these rules:

- Add typed fact arrays or typed count groups instead of hiding data in a generic JSON blob.
- Keep raw facts lossless enough that reports can be recomputed without rescanning source.
- Prefer explicit numerator and denominator counters for every derived ratio.
- Keep derived metrics secondary to raw facts and summaries.
- Use deterministic ordering so snapshots and diffs stay reviewable.
- During alpha, prefer a clean schema bump over compatibility aliases when a simpler contract is clearer.

Candidate future fact families include:

| Future fact family | Examples of raw evidence |
|--------------------|--------------------------|
| Overrides | Prop/style/modifier override sites, overridden token or component slot, parent scope. |
| Deprecations | Deprecated registry component invocations, replacement target, parent scope. |
| Tokens | Token usage sites, hard-coded value candidates, token category. See [Token Scanning Design](./2026-07-03-token-scanning-design.md). |
| Variants | Component variant prop usage, unsupported variant candidates. |
| Imports | Import source, alias, package/module evidence used for resolution. |
| Ownership | File/module/team tags when configured by the repository. |

## Alpha Cutover

Wax is still alpha, so v2 should move directly to the new scan format instead of emitting v1 compatibility aliases.

Cutover rules:

- Bump `schema_version` for `ScanFacts` and `MergedScan`.
- Remove `adoption_coverage_ratio` from the v2 metrics shape.
- Emit `invocation_adoption_ratio` and `registry_resolution_ratio` from explicit counters.
- Update CLI, reports, fixtures, and scan analytics to read v2 fields directly.
- v1 consumers should reject `schema_version: 2` rather than silently interpreting it as v1.

## Reporting Contract

Hero reporting should display:

1. UI invocation adoption.
2. Invocation breakdown: DS, local, unresolved, candidate.
3. Raw DS invocations.
4. Registry breadth: used registry components / total registry components.
5. Local definition inventory.

Reports should include:

- Top local symbols by `raw_invocation_count`.
- Top parent scopes with local invocations, when parent attribution is enabled.
- Unresolved UI symbols by raw invocation count.
- Registry components with high raw invocation count and broad parent-scope reach.
- Data gaps when parent attribution or unresolved tracking is disabled.

Reports may show top-N rows for readability, but the machine-readable scan output should keep complete rows by default.

## Acceptance Criteria

1. A wrapper-heavy fixture reports local invocations and no longer shows false 100% invocation adoption.
2. Every invoked local definition appears in `usage_sites[]` with `match_status: "local"`.
3. `symbol_usage_summary[]` includes registry, local, candidate, and unresolved symbol rows with raw invocation counters.
4. Parent scope rows are complete by default, suppressible with `parent_scope_limit: 0`, and limitable with positive integers.
5. File, line, and column are navigation metadata only; trend identities prefer semantic IDs.
6. Merged scans sum counters and recompute ratios.
7. Reports distinguish UI invocation adoption, registry resolution, raw DS invocations, local definitions, and unresolved UI calls.
