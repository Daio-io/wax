# Adoption Metrics v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Replace misleading registry-only adoption reporting with schema v2 invocation facts, raw counters, per-symbol summaries, parent attribution, and honest reporting labels.

**Architecture:** Extend `wax-contract` first, then update `wax-core` merge/count logic, then update parser-backed language packs to emit local/unresolved invocation facts and parent attribution using the same semantics. Because Wax is alpha, v2 is a direct schema cutover: reporting and scan analytics move to the new counters and summaries without v1 compatibility aliases.

**Tech Stack:** Rust workspace under `engine/`, `serde` JSON contracts, tree-sitter Kotlin/Swift scanners, SWC React scanner, existing golden fixtures, `.wax` JSON config, scan analytics skill scripts/templates.

## Global Constraints

- Language packs emit facts only; `wax-core` owns merged summaries and reporting semantics.
- Prefer raw typed facts and explicit counters over early derived opinions; future fact families should be additive and easy to aggregate.
- Parser-backed packs must stay aligned on scan semantics for `usage_sites`, `match_status`, local invocations, parent attribution, and registry package resolution.
- `.waxrc`, `.wax/wax.config.json`, `wax.lock.json`, scan output JSON, and schema files are user-facing contracts; update schemas, fixtures, docs, and tests when changing them.
- `wax validate` must remain repo-local and CI-friendly; it must not depend on global `~/.wax/` install state.
- `wax scan --no-auto-install` must remain suitable for CI with committed lockfiles and preinstalled language packs.
- Prefer repo-relative paths in config and outputs.
- Run `cargo fmt --all` before committing Rust changes.

---

## Execution Model

- One task = one focused PR unless the maintainer explicitly batches adjacent tasks.
- Branch names should use the repository default prefix, for example `dai/adoption-metrics-v2-contract`.
- Task PR titles should follow `Task N: <description> (adoption metrics v2)`.
- Each task updates this plan's checkboxes for completed steps before opening or updating its PR.
- Implementation tasks should start from this merged plan and use `superpowers:subagent-driven-development` or `superpowers:executing-plans`.

## Reference Spec

- Design spec: [docs/specs/2026-06-20-adoption-metrics-v2-design.md](../specs/2026-06-20-adoption-metrics-v2-design.md)
- Existing contract spec: [docs/specs/2026-05-16-language-packs-and-distribution.md](../specs/2026-05-16-language-packs-and-distribution.md)
- Wax scan analytics design: [docs/specs/2026-06-14-wax-scan-design.md](../specs/2026-06-14-wax-scan-design.md)

## File Structure

- Modify `engine/crates/wax-contract/src/lib.rs` — add v2 contract fields and validation.
- Modify `engine/crates/wax-contract/schemas/scan-facts.schema.json` — publish schema v2 output shape.
- Modify `engine/crates/wax-contract/schemas/waxrc.schema.json` — document nested `adoption` config keys.
- Modify `engine/crates/wax-contract/tests/schema_roundtrip.rs` — add schema v2 round-trip and validation tests.
- Modify `engine/crates/wax-core/src/lib.rs` — aggregate v2 counters and summaries across language scans.
- Modify `engine/crates/wax-core/tests/scan_output.rs` — assert merged v2 output and direct schema cutover.
- Modify `engine/crates/wax-core/tests/subprocess_protocol.rs` — update canned subprocess facts to schema v2.
- Modify `engine/crates/wax-cli/src/commands/scan.rs` — update terminal labels from coverage to invocation adoption/registry resolution.
- Modify `engine/crates/wax-cli/tests/scan_command.rs` — update CLI scan fixtures and output assertions.
- Modify `engine/crates/wax-lang-basic/src/line_scan.rs` and tests — emit schema v2 registry-only facts with explicit capability gaps.
- Modify `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs` — emit local/unresolved Compose invocations and parent attribution.
- Modify `engine/crates/wax-lang-compose/tests/fixtures/small/` — add wrapper and slot examples.
- Modify `engine/crates/wax-lang-react/src/extract.rs` and related tests — emit local/unresolved JSX invocations and parent attribution.
- Modify `engine/crates/wax-lang-react/tests/fixtures/small/` — add wrapper and children examples.
- Modify `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs` and tests — emit SwiftUI local/unresolved invocations and parent attribution.
- Modify `engine/crates/wax-lang-swift/tests/fixtures/small/` — add wrapper and `@ViewBuilder` examples.
- Modify `skills/wax-scan/scripts/extract-insights.sh` — prefer v2 counts and summaries when present.
- Modify `skills/wax-scan/SKILL.md` — replace coverage language with invocation adoption and registry resolution.
- Modify `skills/wax-scan/templates/report.html` and `skills/wax-scan/reference.md` — update labels, charts, insights versioning, and baseline rules.
- Modify `scripts/fixtures/wax-scan/*.json` — update extractor and HTML fixture inputs/expectations.
- Modify `docs/specs/2026-05-16-language-packs-and-distribution.md` — link to the v2 contract and alpha cutover rules.
- Modify `docs/adr/README.md` and add an ADR when implementation completes.
- Modify `docs/plans/README.md` — track active adoption metrics v2 plan.

## Task 1: Extend the Shared Contract

**Files:**
- Modify: `engine/crates/wax-contract/src/lib.rs`
- Modify: `engine/crates/wax-contract/schemas/scan-facts.schema.json`
- Modify: `engine/crates/wax-contract/schemas/waxrc.schema.json`
- Modify: `engine/crates/wax-contract/tests/schema_roundtrip.rs`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`

**Interfaces:**
- Produces: `MatchStatus::Local`, `ParentScope`, `SymbolParentScopeSummary`, `IdentityStability`, `SymbolUsageSummary`, v2 count groups, and v2 metrics.
- Consumes: existing `SourceLocation`, `UsageSite`, `LocalComponent`, `Metrics`, and `CountSummary`.

- [x] **Step 1: Write failing contract tests**

Add tests that deserialize schema v2 facts containing:

```json
{
  "schema_version": 2,
  "usage_sites": [
    {
      "id": "usage.compose:src/Discover.kt:4:5:EpisodeCard",
      "location": {"file": "src/Discover.kt", "line": 4, "column": 5},
      "symbol": "EpisodeCard",
      "qualified_symbol": "com.example.EpisodeCard",
      "match_status": "local",
      "local_definition_id": "local.compose:com.example.EpisodeCard",
      "parent": {
        "parent_id": "compose:composable:com.example.DiscoverScreen",
        "symbol": "DiscoverScreen",
        "qualified_symbol": "com.example.DiscoverScreen",
        "scope_kind": "composable",
        "identity_basis": "package_qualified_symbol",
        "identity_stability": "semantic",
        "location": {"file": "src/Discover.kt", "line": 2, "column": 1}
      }
    }
  ],
  "symbol_usage_summary": [
    {
      "symbol_id": "compose:local:com.example.EpisodeCard",
      "symbol": "EpisodeCard",
      "qualified_symbol": "com.example.EpisodeCard",
      "symbol_kind": "local",
      "match_status": "local",
      "local_definition_id": "local.compose:com.example.EpisodeCard",
      "identity_basis": "package_qualified_symbol",
      "identity_stability": "semantic",
      "raw_invocation_count": 1,
      "parent_scope_count": 1,
      "file_count": 1,
      "parent_scopes": [],
      "parent_scope_limit": 0,
      "parent_scopes_truncated": true
    }
  ]
}
```

Expected before implementation: deserialization or validation fails because v2 fields and `local` match status do not exist.

- [x] **Step 2: Add contract types**

Add typed structs/enums rather than `serde_json::Value` extension blobs:

```rust
pub enum MatchStatus {
    Resolved,
    Candidate,
    Local,
    Unresolved,
}

pub enum SymbolKind {
    Registry,
    Local,
    Candidate,
    Unresolved,
}

pub enum IdentityStability {
    Semantic,
    PathSensitive,
    ScanLocal,
}

pub struct ParentScope {
    pub parent_id: String,
    pub symbol: String,
    pub qualified_symbol: Option<String>,
    pub scope_kind: String,
    pub identity_basis: String,
    pub identity_stability: IdentityStability,
    pub location: Option<SourceLocation>,
}

pub struct SymbolUsageSummary {
    pub symbol_id: String,
    pub symbol: String,
    pub qualified_symbol: Option<String>,
    pub symbol_kind: SymbolKind,
    pub match_status: MatchStatus,
    pub registry_symbol: Option<String>,
    pub local_definition_id: Option<String>,
    pub identity_basis: String,
    pub identity_stability: IdentityStability,
    pub raw_invocation_count: u32,
    pub parent_scope_count: u32,
    pub file_count: u32,
    pub parent_scopes: Vec<SymbolParentScopeSummary>,
    pub parent_scope_limit: Option<u32>,
    pub parent_scopes_truncated: bool,
}

pub struct SymbolParentScopeSummary {
    pub parent_id: String,
    pub symbol: String,
    pub qualified_symbol: Option<String>,
    pub scope_kind: String,
    pub identity_basis: String,
    pub identity_stability: IdentityStability,
    pub invocation_count: u32,
    pub location: Option<SourceLocation>,
}
```

- [x] **Step 3: Add Rust docs and schema descriptions**

Mirror the design spec's Type and Resolution Dictionary in public Rust doc comments and schema descriptions. Each new enum value and output key needs a one-line description, including:

- `resolved`, `local`, `candidate`, and `unresolved`
- `registry`, `local`, `candidate`, and `unresolved` symbol kinds
- `semantic`, `path_sensitive`, and `scan_local`
- `parent_scope_limit: null | 0 | N`
- every new count group under `registry`, `definitions`, `raw_invocations`, `adoption`, and `parent_scopes`

- [x] **Step 4: Update JSON schemas**

Update `engine/crates/wax-contract/schemas/scan-facts.schema.json` for schema v2 facts, including `MatchStatus::Local`, parent attribution, v2 count groups, metrics, and `symbol_usage_summary[]`.

Update `engine/crates/wax-contract/schemas/waxrc.schema.json` for the nested `adoption` config block:

```json
{
  "track_local_invocations": true,
  "track_unresolved_invocations": true,
  "parent_attribution": { "enabled": true, "scope_visibility": ["public", "internal", "private"] },
  "candidate_policy": "report_separately",
  "symbol_usage_summary": { "enabled": true, "parent_scope_limit": null }
}
```

- [x] **Step 5: Extend `UsageSite` and `LocalComponent`**

Add:

```rust
pub qualified_symbol: Option<String>
pub local_definition_id: Option<String>
pub parent: Option<ParentScope>
```

to `UsageSite`, and:

```rust
pub qualified_symbol: Option<String>
pub identity_basis: Option<String>
pub identity_stability: Option<IdentityStability>
```

to `LocalComponent`.

- [x] **Step 6: Add v2 counts and metrics**

Add count groups from the spec and remove v1-only metric fields from the schema v2 output shape. Add explicit denominators for `invocation_adoption_ratio` and `registry_resolution_ratio`.

- [x] **Step 7: Update validation**

Validation must enforce:

- `local` usage sites require `local_definition_id`.
- `resolved` and `candidate` usage sites require `registry_symbol`.
- `local` and `unresolved` usage sites must not carry `registry_symbol`.
- `resolved`, `candidate`, and `unresolved` usage sites must not carry `local_definition_id`.
- `unresolved` usage sites require a non-empty `symbol` and no registry/local linkage.
- `symbol_usage_summary[]` rows must match the represented status: `registry` rows use `resolved`, `candidate` rows use `candidate`, `local` rows use `local`, and `unresolved` rows use `unresolved`.
- `parent_scope_limit: 0` allows empty `parent_scopes` with `parent_scope_count > 0`.
- `parent_scopes_truncated` is true when emitted rows are fewer than `parent_scope_count`.
- Ratios match v2 count denominators within the existing floating-point tolerance.

- [x] **Step 8: Run focused checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-contract
cargo clippy -p wax-contract --all-targets -- -D warnings
```

Expected: all pass.

- [x] **Step 9: Commit**

```bash
git add engine/crates/wax-contract docs/specs/2026-05-16-language-packs-and-distribution.md
git commit -m "feat: extend scan contract for adoption metrics v2"
```

The language-pack distribution spec update must explicitly state that Adoption Metrics v2 supersedes the v1 `adoption_coverage_ratio` semantics for schema v2 outputs.

## Task 2: Add Engine Aggregation and Summary Generation

**Files:**
- Modify: `engine/crates/wax-core/src/lib.rs`
- Modify: `engine/crates/wax-core/tests/scan_output.rs`
- Modify: `engine/crates/wax-core/tests/subprocess_protocol.rs`

**Interfaces:**
- Consumes: v2 `ScanFacts` from Task 1.
- Produces: per-language and root `MergedScan.repo_summary` counters plus `symbol_usage_summary[]` sorted deterministically.

- [x] **Step 1: Write failing merge tests**

Add a fixture with two language scans:

- Compose: resolved `600`, local `150`, unresolved `10`, candidate `0`.
- Swift: resolved `200`, local `40`, unresolved `5`, candidate `0`.

Expected merged counts:

```text
raw_invocations.total = 1005
raw_invocations.resolved = 800
raw_invocations.local = 190
raw_invocations.unresolved = 15
invocation_adoption_ratio = 800 / 1005
```

Assert ratios are recomputed from summed counts, not averaged from per-language percentages.

- [x] **Step 2: Implement summary builder**

Add an engine helper that groups `usage_sites[]` by normalized symbol identity and match status. The grouping order should be:

1. `registry_symbol` for resolved/candidate.
2. `local_definition_id` for local.
3. `qualified_symbol` when present.
4. language id plus `symbol` fallback.

- [x] **Step 3: Implement parent-scope aggregation**

For each symbol summary, group parent rows by `parent_id`, count invocations, and sort by `invocation_count desc`, then `parent_id asc`. Apply `parent_scope_limit` after the full `parent_scope_count` is known.

- [x] **Step 4: Enforce direct schema cutover**

Remove v1 compatibility aliases from v2 merged output. Compute and label `registry_resolution_ratio` and `invocation_adoption_ratio` directly from the new counter groups.

- [x] **Step 5: Update subprocess fixtures**

Update canned subprocess facts in `engine/crates/wax-core/tests/subprocess_protocol.rs` to schema v2 so pack protocol tests cover the new shape.

- [x] **Step 6: Emit root repo summaries**

Add root-level `repo_summary.counts`, `repo_summary.metrics`, and root `symbol_usage_summary[]` to `MergedScan`. Tests must assert repo-level ratios are recomputed from summed counters.

- [ ] **Step 7: Run focused checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-core
cargo clippy -p wax-core --all-targets -- -D warnings
```

Expected: all pass.

- [x] **Step 8: Commit**

```bash
git add engine/crates/wax-core
git commit -m "feat: aggregate adoption metrics v2 summaries"
```

## Task 3: Migrate Basic Pack to Schema v2

**Files:**
- Modify: `engine/crates/wax-lang-basic/src/line_scan.rs`
- Modify: `engine/crates/wax-lang-basic/tests/golden_small.rs`
- Modify: `engine/crates/wax-lang-basic/tests/config_validation.rs`
- Modify: `engine/crates/wax-lang-basic/tests/stdio_cli.rs`
- Modify: `engine/crates/wax-lang-basic/tests/fixtures/small/golden.json`

**Interfaces:**
- Consumes: v2 contract from Task 1.
- Produces: schema v2 registry-only facts with explicit capability gaps.

- [x] **Step 1: Update golden expectations**

Update basic-pack fixtures so output uses `schema_version: 2`, v2 counts, and v2 metrics. Local and unresolved invocation counters should be zero because the text scanner does not collect those facts.

- [x] **Step 2: Keep basic extraction registry-only**

Preserve existing registry text scanning behavior. Do not emit `local` or `unresolved` usage sites from `wax-lang-basic` until a future language-aware detector exists.

- [x] **Step 3: Emit capability diagnostics**

Emit informational diagnostics or capability flags that allow reporting to show data gaps for:

- local invocation tracking
- unresolved UI invocation tracking
- parent attribution

- [ ] **Step 4: Run focused checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-basic
cargo clippy -p wax-lang-basic --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add engine/crates/wax-lang-basic
git commit -m "feat: emit basic schema v2 scan facts"
```

## Task 4: Implement Compose v2 Facts

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/`
- Modify: `engine/crates/wax-lang-compose/tests/golden_small.rs`

**Interfaces:**
- Consumes: v2 contract from Task 1.
- Produces: Compose `local` and `unresolved` usage sites with parent attribution.

- [x] **Step 1: Add failing wrapper fixture**

Add Kotlin fixture:

```kotlin
package com.example.discover

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

Expected:

- `EpisodeCard` local invocations: `2`.
- `Tier` resolved invocations: `1`.
- `BodyText` resolved invocations: `1`.
- `DiscoverScreen` parent for both `EpisodeCard` calls.
- `EpisodeCard` parent for `Tier` and `BodyText`.

- [x] **Step 2: Add failing slot fixture**

Add Kotlin fixture:

```kotlin
@Composable
fun DiscoverScreen() {
    Tier {
        Button()
        Tier { Button() }
    }
}
```

Expected: both `Button` calls and both `Tier` calls have parent `DiscoverScreen`.

- [x] **Step 3: Build local definition index**

Index composable local definitions by package-qualified symbol and source symbol. Use the semantic ID format:

```text
local.compose:<package>.<symbol>
```

- [x] **Step 4: Emit local and unresolved invocations**

For each composable call expression:

1. Resolve registry match first.
2. Else resolve local definition.
3. Else emit `unresolved` only when the call passes the Compose UI invocation detector.

- [x] **Step 5: Implement parent walk**

Walk AST ancestors to the nearest `@Composable` function declaration. Calls inside trailing lambdas remain attributed to the enclosing composable declaration, not the slot callee.

- [ ] **Step 6: Run focused checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose
cargo clippy -p wax-lang-compose --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add engine/crates/wax-lang-compose
git commit -m "feat: emit compose adoption metrics v2 facts"
```

## Task 5: Implement React v2 Facts

**Files:**
- Modify: `engine/crates/wax-lang-react/src/extract.rs`
- Modify: `engine/crates/wax-lang-react/src/facts.rs`
- Modify: `engine/crates/wax-lang-react/tests/fixtures/small/`
- Modify: `engine/crates/wax-lang-react/tests/golden_small.rs`

**Interfaces:**
- Consumes: v2 contract from Task 1.
- Produces: React `local` and `unresolved` JSX usage sites with parent attribution.

- [x] **Step 1: Add failing wrapper fixture**

Add TSX fixture:

```tsx
export function DiscoverScreen() {
  return (
    <Tier>
      <Button />
      <EpisodeCard />
    </Tier>
  );
}

export function EpisodeCard() {
  return <Button />;
}
```

Expected:

- `Button` resolved invocations: `2`.
- `Tier` resolved invocations: `1`.
- `EpisodeCard` local invocations: `1`.
- `<Button />` inside `<Tier>` has parent `DiscoverScreen`, not `Tier`.

- [x] **Step 2: Index local components by module identity**

Use export-aware semantic identity when available. Fall back to path-sensitive identity:

```text
react:component:<module-identity>#<component-name>
```

Emit `identity_stability: "path_sensitive"` for path-derived IDs.

- [x] **Step 3: Emit local and unresolved JSX usage**

For each PascalCase JSX element:

1. Resolve registry through existing import graph.
2. Else resolve local component through local component index.
3. Else emit `unresolved` when the symbol is UI-shaped and not an intrinsic element.

- [x] **Step 4: Implement parent walk**

Parent is the innermost enclosing React component function/class containing the JSX element. Children passed to another component remain attributed to the caller component.

- [ ] **Step 5: Run focused checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-react
cargo clippy -p wax-lang-react --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/wax-lang-react
git commit -m "feat: emit react adoption metrics v2 facts"
```

## Task 6: Implement SwiftUI v2 Facts

**Files:**
- Modify: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-swift/tests/fixtures/small/`
- Modify: `engine/crates/wax-lang-swift/tests/golden_small.rs`

**Interfaces:**
- Consumes: v2 contract from Task 1.
- Produces: SwiftUI `local` and `unresolved` usage sites with parent attribution.

- [x] **Step 1: Add failing SwiftUI fixture**

Add Swift fixture:

```swift
struct DiscoverView: View {
    var body: some View {
        Tier {
            Button("Play") { }
            EpisodeCardView()
        }
    }
}

struct EpisodeCardView: View {
    var body: some View {
        Button("Play") { }
    }
}
```

Expected:

- `Button` resolved invocations: `2`.
- `Tier` resolved invocations: `1`.
- `EpisodeCardView` local invocations: `1`.
- Parent for children inside `Tier` is `DiscoverView`.

- [x] **Step 2: Index local SwiftUI views**

Index `struct X: View` and `@ViewBuilder` declarations. Prefer module-qualified IDs when available.

- [x] **Step 3: Emit local and unresolved invocations**

Resolve registry first, local definitions second, unresolved UI-shaped invocations third. Modifier chains should not inflate invocation counts unless the modifier is itself a configured registry component.

- [x] **Step 4: Implement parent walk**

Parent is the enclosing `View` type body or `@ViewBuilder` function containing the call. Calls inside builder closures passed to another view remain attributed to the caller view, not the slot host.

- [ ] **Step 5: Run focused checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-swift
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add engine/crates/wax-lang-swift
git commit -m "feat: emit swiftui adoption metrics v2 facts"
```

## Task 7: Update CLI and Scan Analytics Reporting

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-cli/tests/scan_command.rs`
- Modify: `skills/wax-scan/SKILL.md`
- Modify: `skills/wax-scan/scripts/extract-insights.sh`
- Modify: `scripts/fixtures/wax-scan/scan-merged.sample.json`
- Modify: `scripts/fixtures/wax-scan/expected-insights.sample.json`
- Modify: `scripts/fixtures/wax-scan/scan-merged.schema-v2.sample.json`
- Modify: `skills/wax-scan/templates/report.html`
- Modify: `skills/wax-scan/reference.md`
- Modify: `docs/specs/2026-06-14-wax-scan-design.md`

**Interfaces:**
- Consumes: v2 merged output from Task 2.
- Produces: honest labels and v2-aware terminal/HTML reports.

- [ ] **Step 1: Add failing extractor fixture**

Update wax-scan fixtures so v2 output includes:

- `raw_invocations.resolved`
- `raw_invocations.local`
- `raw_invocations.unresolved`
- `symbol_usage_summary[]`
- parent scope counters

Expected extractor output should include UI invocation adoption, registry resolution, top local symbols, and parent-scope hotspots.

- [ ] **Step 2: Update CLI labels**

Replace unqualified "coverage" copy with:

- `UI invocation adoption`
- `Registry resolution`
- `Raw DS invocations`
- `Local definitions`
- `Unresolved UI calls`

- [ ] **Step 3: Update HTML and terminal reporting**

Show hero cards in this order:

1. UI invocation adoption.
2. Invocation breakdown.
3. Registry breadth.
4. Local definition inventory.

Use `symbol_usage_summary[]` for top local/unresolved symbols.

- [ ] **Step 4: Update skill instructions**

Update `skills/wax-scan/SKILL.md` so the skill asks for UI invocation adoption, registry resolution, raw DS invocations, local definitions, and unresolved UI calls. Remove unqualified "coverage" language except when explaining old v1 limitations.

- [ ] **Step 5: Version insights and baseline behavior**

Bump the wax-scan extracted insights schema version. Baseline comparisons must support v2-to-v2 deltas for invocation adoption, registry resolution, raw invocations, and symbol summaries. If a supplied baseline is v1, emit a compatibility data gap rather than mixing v1 and v2 denominators.

- [ ] **Step 6: Run focused checks**

Run:

```bash
skills/wax-scan/scripts/test-extract-insights.sh
cd engine
cargo fmt --all
cargo test -p wax-cli
cargo clippy -p wax-cli --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add engine/crates/wax-cli skills/wax-scan scripts/fixtures/wax-scan docs/specs/2026-06-14-wax-scan-design.md
git commit -m "feat: report adoption metrics v2"
```

## Task 8: Cross-Crate Verification and Cutover Docs

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Create: `docs/adr/2026-06-20-adoption-metrics-v2.md`
- Modify: `docs/adr/README.md`
- Modify: `docs/plans/2026-06-20-adoption-metrics-v2-plan.md`
- Modify: `docs/plans/README.md`

**Interfaces:**
- Consumes: all previous task outputs.
- Produces: release-ready docs and completed plan checkboxes.

- [ ] **Step 1: Update user-facing docs**

Document:

- v2 facts-first adoption model.
- `symbol_usage_summary[]`.
- Parent scope limit config.
- Alpha cutover from `adoption_coverage_ratio` to explicit v2 counters and metrics.
- Roadmap active-plan status in `docs/plans/README.md`.

- [ ] **Step 2: Run workspace checks**

Run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 3: Update plan checkboxes**

Tick completed task checkboxes and verification steps in this plan before opening or updating the implementation PR.

- [ ] **Step 4: Commit**

```bash
git add README.md CHANGELOG.md docs/adr/2026-06-20-adoption-metrics-v2.md docs/adr/README.md docs/plans/2026-06-20-adoption-metrics-v2-plan.md docs/plans/README.md
git commit -m "docs: document adoption metrics v2 rollout"
```

## Release Gate

- [x] Wrapper fixture reports local invocations and no false 100% adoption.
- [x] `symbol_usage_summary[]` includes registry, local, candidate, and unresolved rows.
- [x] Parent scope rows are complete by default and respect `parent_scope_limit`.
- [x] Merged scans sum counters and recompute ratios.
- [ ] CLI and HTML reports distinguish invocation adoption from registry resolution.
- [x] v2 uses the new scan format directly without v1 compatibility aliases.
- [x] `wax-lang-basic` emits schema v2 registry-only facts and capability gaps.
- [x] `wax-lang-compose`, `wax-lang-react`, and `wax-lang-swift` all emit local/unresolved invocation facts and parent attribution.
- [x] Subprocess protocol and CLI scan fixtures use schema v2 facts.
- [x] `engine/crates/wax-contract/schemas/scan-facts.schema.json` and `waxrc.schema.json` document v2 fields and config.
- [ ] `skills/wax-scan/SKILL.md`, extractor fixtures, and baseline behavior are v2-aware.
- [ ] Adoption Metrics v2 ADR is added and indexed.
- [ ] Workspace fmt, tests, and clippy pass.
