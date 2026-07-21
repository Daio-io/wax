# Token Inference and Reporting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn raw hard-coded styling observations into explainable exact, near, unmatched, and unassessed token findings without counting unsupported fixed dimensions as token debt.

**Architecture:** Parser-backed packs emit raw values with typed usage context, while `wax-core` performs one deterministic merged-scan inference pass against optional canonical registry values. CLI and `wax-scan` consume the same core-owned classifications, and `wax-registry-discover` provides a reviewed authoring workflow for missing or stale canonical values.

**Tech Stack:** Rust workspace under `engine/`, `serde`, JSON Schema, tree-sitter Kotlin/Swift scanners, SWC React scanner, `jq`-based extraction, shell fixture tests, self-contained HTML reports, Markdown Agent Skills.

## Global Constraints

- Retain every parser-backed hard-coded styling observation; inference adds conclusions without deleting raw facts.
- Language packs own syntax context only; `wax-core` owns normalization, comparison, confidence, counts, and merged reporting facts.
- Basic continues to emit token references only and never emits hard-coded observations or inference rows.
- `DesignSystemToken.value` is optional and contains one source-facing canonical value per language registry.
- Match only within the same language and token category.
- Near matching applies only to compatible numeric scalar values; colors, shadows, and composite typography require exact normalized matches.
- The repo-local default numeric tolerance is exactly `2`; `0` disables near matching; negative and non-finite values are invalid.
- Exact, near, unmatched, and unassessed counts remain separate; do not add a weighted debt, health, maturity, or compliance score.
- Context adjusts confidence only; it never suppresses a value match or removes an observation.
- Multiple equally good suggestions remain visible and lower confidence by one level, with `low` as the floor.
- AI-derived registry values affect deterministic metrics only after the user reviews and writes them.
- Parser-backed packs (`compose`, `react`, and `swift`) must ship context parity in the same task.
- Scan JSON, `.wax/wax.config.json`, registry JSON, schemas, fixtures, CLI output, and skill insight JSON are user-facing contracts; update them together.
- Keep `wax validate` repo-local and keep `wax scan --no-auto-install` suitable for CI.
- Run `cargo fmt --all` before every Rust commit.

---

## Execution Model

- One task = one focused PR unless the maintainer explicitly batches adjacent tasks.
- Start each task from the merged predecessor because contract, core, packs, reporting, and skill maintenance are ordered dependencies.
- Use branch names such as `dai/token-inference-contract-v3` and conventional commits without AI/tool attribution.
- Tick the task completion checkbox and every completed step in the same PR.
- Do not begin implementation while the roadmap's active-plan gate blocks this plan unless the maintainer explicitly promotes it.
- The top-level File Structure is the inventory for the full plan; each task's Files section is authoritative for that task PR, including mechanical cutover files discovered by its inventory step.

**Intermediate PR behavior:** Task 1 must leave every merged scan valid by emitting one `unassessed` row per raw hard-coded site. Task 2 replaces that complete stub with configured deterministic inference; there is no intermediate state where nonempty raw sites may carry an empty inference report.

## Reference Spec

- Design: [docs/specs/2026-07-19-token-inference-reporting-design.md](../specs/2026-07-19-token-inference-reporting-design.md)
- Token scanning v1: [docs/specs/2026-07-03-token-scanning-design.md](../specs/2026-07-03-token-scanning-design.md)
- Wax scan analytics: [docs/specs/2026-06-14-wax-scan-design.md](../specs/2026-06-14-wax-scan-design.md)
- Registry sync/config v2: [docs/specs/2026-07-04-registry-sync-config-design.md](../specs/2026-07-04-registry-sync-config-design.md)

## File Structure

- Modify `engine/crates/wax-contract/src/lib.rs` — schema-v3 types, inference linkage/count validation, and removal of `token_reference_ratio`.
- Modify `engine/crates/wax-contract/schemas/scan-facts.schema.json` — schema-v3 per-language token values and raw style context.
- Modify `engine/crates/wax-contract/schemas/waxrc.schema.json` — `token_inference.numeric_tolerance`.
- Modify `engine/crates/wax-lang-api/src/token_registry.rs` — optional canonical token values.
- Create `engine/crates/wax-core/src/token_inference.rs` — normalization, matching, confidence, evidence, and counts.
- Modify `engine/crates/wax-core/src/adoption_merge.rs` and `engine/crates/wax-core/src/lib.rs` — core inference orchestration.
- Modify the Compose, React, and Swift style extractors and golden fixtures — context parity.
- Modify `engine/crates/wax-cli/src/commands/scan.rs` — separated inference reporting.
- Modify wax-scan extractor, template, renderer, fixtures, tests, and skill docs — deterministic terminal/HTML reporting.
- Modify `skills/wax-registry-discover/` — reviewed canonical-value maintenance.
- Modify roadmap/spec/ADR documentation during closeout.

---

### Task 1: Cut the Shared Contract to Schema v3

- [x] **Task 1 complete**

**Files:**
- Modify: `engine/crates/wax-contract/src/lib.rs`
- Modify: `engine/crates/wax-contract/schemas/scan-facts.schema.json`
- Modify: `engine/crates/wax-lang-api/src/token_registry.rs`
- Modify: `engine/crates/wax-core/src/adoption_merge.rs`
- Modify: `engine/crates/wax-contract/tests/schema_roundtrip.rs`
- Modify: `engine/crates/wax-lang-api/tests/token_registry.rs`
- Modify: `engine/crates/wax-core` merge tests found by Step 1
- Modify: affected Rust fixture constructors and embedded scan JSON found by Step 1

**Interfaces:**
- Consumes: existing `TokenCategory`, `DesignSystemToken`, `HardcodedStyleSite`, `LanguageId`, `MergedScan`, and `ScanFactsError`.
- Produces: `SCHEMA_VERSION = 3`, `StyleContext`, inference types, `DesignSystemToken.value`, `HardcodedStyleSite.context`, `MergedScan.token_inference`, and schema-v3 validation used by Tasks 2–5.

- [x] **Step 1: Inventory every schema-v2 and affected struct literal**

Run:

```bash
cd engine
rg -n 'SCHEMA_VERSION|schema_version[^\n]*2|DesignSystemToken \{|HardcodedStyleSite \{|token_reference_ratio|MergedScan \{' crates
```

Expected: matches in contract, packs, core/CLI tests, wire tests, and embedded fixture scripts. Save the list in the PR description as the cutover checklist.

- [x] **Step 2: Write the failing schema-v3 round-trip test**

Add `schema_v3_token_inference_roundtrips` to `schema_roundtrip.rs` using:

```rust
let token = DesignSystemToken {
    id: "spacing.s".into(),
    key: "Spacing.s".into(),
    category: TokenCategory::Spacing,
    aliases: vec![],
    value: Some("4.dp".into()),
};
let site = HardcodedStyleSite {
    id: "hardcoded.compose:src/Card.kt:8:20:spacing".into(),
    location: SourceLocation { file: "src/Card.kt".into(), line: 8, column: Some(20) },
    value: "4.dp".into(),
    category: TokenCategory::Spacing,
    context: StyleContext::Padding,
    parent: None,
};
let inference = HardcodedStyleInference {
    language: LanguageId::try_from("compose").unwrap(),
    site_id: site.id.clone(),
    classification: TokenInferenceClassification::Exact,
    confidence: Some(TokenInferenceConfidence::VeryHigh),
    suggestions: vec![TokenReplacementSuggestion {
        token_id: token.id.clone(),
        token_key: token.key.clone(),
        canonical_value: "4.dp".into(),
        match_kind: TokenMatchKind::Exact,
        distance: Some(0.0),
        normalized_unit: Some("dp".into()),
    }],
    evidence: vec![TokenInferenceEvidence::ExactValue, TokenInferenceEvidence::ClearUsageContext],
};
```

Validate the token and site through the per-language JSON schema. Place the row in `MergedScan.token_inference.sites`, provide reconciled counts, serialize/deserialize the merged value, call `MergedScan::validate`, and assert equality. Do not validate a merged scan against `scan-facts.schema.json`; that schema is intentionally scoped to `ScanFacts`.

- [x] **Step 3: Run the test and confirm the red state**

```bash
cd engine
cargo test -p wax-contract --test schema_roundtrip schema_v3_token_inference_roundtrips -- --exact
```

Expected: FAIL to compile because the v3 types and fields do not exist.

- [x] **Step 4: Add the schema-v3 public types**

Set `SCHEMA_VERSION` to `3`, remove `Metrics.token_reference_ratio`, and add:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StyleContext {
    Padding, Margin, Gap, Width, Height, Size,
    Radius, Color, Typography, Elevation, Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenInferenceClassification { Exact, Near, Unmatched, Unassessed }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TokenInferenceConfidence { Low, Medium, High, VeryHigh }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenMatchKind { Exact, Near }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TokenInferenceEvidence {
    ExactValue, WithinNumericTolerance, ClearUsageContext,
    GenericDimensionContext, MultipleEqualMatches, MissingCanonicalValues,
    IncompleteCanonicalCoverage, UnsupportedCanonicalFormat,
    IncompatibleUnits, OutsideNumericTolerance,
}
```

Add documented `TokenReplacementSuggestion` and `HardcodedStyleInference`. Define the count structs exactly as:

```rust
pub struct TokenConfidenceCounts {
    pub very_high: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
}

pub struct StyleContextCounts {
    pub padding: u32,
    pub margin: u32,
    pub gap: u32,
    pub width: u32,
    pub height: u32,
    pub size: u32,
    pub radius: u32,
    pub color: u32,
    pub typography: u32,
    pub elevation: u32,
    pub unknown: u32,
}

pub struct TokenInferenceCounts {
    pub hardcoded_observation_count: u32,
    pub assessed_observation_count: u32,
    pub exact_replacement_candidate_count: u32,
    pub near_replacement_candidate_count: u32,
    pub unmatched_observation_count: u32,
    pub unassessed_observation_count: u32,
    pub candidates_by_confidence: TokenConfidenceCounts,
    pub candidates_by_category: TokenCategoryCounts,
    pub candidates_by_context: StyleContextCounts,
}

pub struct TokenInferenceReport {
    pub numeric_tolerance: f64,
    pub counts: TokenInferenceCounts,
    pub sites: Vec<HardcodedStyleInference>,
}
```

- [x] **Step 5: Extend raw fields and registry parsing**

Add:

```rust
pub struct DesignSystemToken {
    // existing fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

pub struct HardcodedStyleSite {
    // existing fields
    pub context: StyleContext,
}

pub struct MergedScan {
    // existing fields
    pub token_inference: TokenInferenceReport,
}
```

In `parse_registry_tokens`, parse `value` as an optional non-empty string and preserve it verbatim. Add tests proving absent values become `None`, present values round-trip, and `"value": ""` returns `InvalidTokenField` for `value`.

- [x] **Step 6: Add inference linkage and count validation**

Extend `MergedScan::validate` to enforce:

```text
every raw hard-coded site has exactly one inference row
raw (language, site_id) keys are unique
every row language exists and its site id resolves in that language
duplicate (language, site_id) pairs are invalid
suggested token exists in the same language and category
suggestion canonical value matches that registry token's present canonical value
exact/near rows have suggestions and confidence
unmatched/unassessed rows have neither
suggestion match kind agrees with classification
exact distance is absent or 0; near distance is finite and positive
assessed = exact + near + unmatched
observations = assessed + unassessed
hardcoded_observation_count = inference row count = total raw hard-coded site count
```

Add one negative test per invariant with exact `ContractViolation` field paths. Include duplicate-raw-key, missing-row, duplicate-row, extra-row, and raw-count mismatch tests so an empty inference report cannot validate when raw sites exist. Task 2's inference tests enforce the stronger builder rule that numeric exact matches emit `Some(0.0)` while nonnumeric exact matches emit `None`.

- [x] **Step 7: Publish the per-language schema-v3 JSON shape**

Update the schema id and required version to `3`, require `hardcoded_style_site.context`, allow optional `design_system_token.value`, and remove `metrics.token_reference_ratio`. Add strict `$defs` for the raw enums and structs used by `ScanFacts`. Keep merged inference validation in `MergedScan::validate`. Add a test rewriting valid per-language JSON to version `2` and expecting `UnsupportedSchemaVersion { found: 2, supported: 3 }`.

- [x] **Step 8: Complete the mechanical workspace cutover**

For each Step 1 match, add `value: None`, use `StyleContext::Unknown` until Task 3, remove ratio fields/assertions, and update embedded scan facts to v3.

In `adoption_merge.rs`, add a temporary private `build_unassessed_token_inference` helper used by the default merge path. It must emit exactly one row for every raw hard-coded site with `classification: Unassessed`, absent confidence, no suggestions, and `MissingCanonicalValues` evidence; set tolerance to `2.0` and derive counts so `hardcoded_observation_count == unassessed_observation_count == total raw sites`. Task 2 replaces this stub with deterministic matching. Add a merge test proving nonempty raw sites cannot produce an empty report.

Validate the constructed `MergedScan` before returning it so invalid raw-site identities or generated inference facts fail before any scan output is written.

Re-run Step 1; only explicit v2 incompatibility tests may remain.

- [x] **Step 9: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-contract
cargo test -p wax-lang-api
cargo test -p wax-core
cargo check --workspace --all-targets
cargo clippy -p wax-contract -p wax-lang-api -p wax-core --all-targets -- -D warnings
cd ..
git add engine docs/plans/2026-07-19-token-inference-reporting-plan.md
git commit -m "feat: add token inference contract v3"
```

Expected: every command exits `0` and only Task 1 files are committed.

---

### Task 2: Implement Deterministic Core Inference

- [x] **Task 2 complete**

**Files:**
- Modify: `engine/crates/wax-core/src/config/waxrc.rs`
- Modify: `engine/crates/wax-contract/schemas/waxrc.schema.json`
- Create: `engine/crates/wax-core/src/token_inference.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Modify: `engine/crates/wax-core/src/adoption_merge.rs`
- Modify: `engine/crates/wax-core/tests/config_v2.rs`
- Modify: `engine/crates/wax-core/tests/scan_output.rs`

**Interfaces:**
- Consumes: Task 1's merged inference contract.
- Produces: `TokenInferenceConfig`, `MergeOptions`, and `build_token_inference(&BTreeMap<LanguageId, ScanFacts>, &TokenInferenceConfig) -> Result<TokenInferenceReport, ScanFactsError>`.

- [x] **Step 1: Write failing config tests**

Test default `2.0`, custom `0.5`, exact-only `0`, and rejection of `-1`, `null`, strings, and unknown keys. Use:

```rust
assert_eq!(load_waxrc(minimal.path()).unwrap().token_inference.numeric_tolerance, 2.0);
assert_eq!(load_waxrc(custom.path()).unwrap().token_inference.numeric_tolerance, 0.5);
```

- [x] **Step 2: Confirm the config red state**

```bash
cd engine
cargo test -p wax-core --test config_v2 token_inference -- --nocapture
```

Expected: FAIL because `WaxRc.token_inference` does not exist.

- [x] **Step 3: Add typed repo configuration and schema**

Implement:

```rust
pub const DEFAULT_NUMERIC_TOKEN_TOLERANCE: f64 = 2.0;

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TokenInferenceConfig {
    #[serde(default = "default_numeric_token_tolerance")]
    pub numeric_tolerance: f64,
}

impl Default for TokenInferenceConfig {
    fn default() -> Self {
        Self { numeric_tolerance: DEFAULT_NUMERIC_TOKEN_TOLERANCE }
    }
}
```

Add it to `WaxRc`/`WaxRcRaw`, validate finite non-negative values, and add `token_inference.numeric_tolerance` to `waxrc.schema.json` with `minimum: 0`.

- [x] **Step 4: Write failing normalization tests**

Create `token_inference.rs` tests for:

```text
Compose 4.dp vs 4dp = exact; 4.dp vs 4.sp = incompatible
React width 4 vs 4px = exact; 1rem vs 16px = incompatible
React color "#FFF" vs #fff = exact with distance absent
Swift gap 3 vs 4 = numeric distance 1
```

- [x] **Step 5: Implement conservative normalizers**

Use private helpers:

```rust
enum NormalizedValue {
    Numeric { scalar: f64, unit: Option<String> },
    ExactText(String),
    Unsupported,
}

fn normalize_observed(language: &LanguageId, site: &HardcodedStyleSite) -> NormalizedValue;
fn normalize_canonical(
    language: &LanguageId,
    category: TokenCategory,
    context: StyleContext,
    value: &str,
) -> NormalizedValue;
fn numeric_distance(left: &NormalizedValue, right: &NormalizedValue) -> Option<f64>;
```

Normalize Compose `dp`/`sp`, React deterministic pixel-bearing numbers, Swift scalar layout values, quotes/whitespace, and hex case. Do not convert `rem`, `%`, viewport units, or incompatible units.

- [x] **Step 6: Write failing classification/confidence tests**

Cover:

```text
4->4 padding = exact/very_high
4->4 width = exact/high
3->4 gap at tolerance 2 = near/medium
3->4 width at tolerance 2 = near/low
3->4 at tolerance 0 = unmatched
200->4 = unmatched/no confidence
missing or incomplete canonical values without exact match = unassessed
two equal exact tokens = two suggestions and one-level confidence reduction
no same-category tokens = unassessed
```

Assert evidence arrays and deterministic token-id ordering.

- [x] **Step 7: Implement inference and counts**

Expose:

```rust
pub fn build_token_inference(
    languages: &BTreeMap<LanguageId, ScanFacts>,
    config: &TokenInferenceConfig,
) -> Result<TokenInferenceReport, ScanFactsError>;
```

Emit one row per raw site, prefer exact matches, require complete usable category values before near/unmatched, keep only nearest ties, apply the confidence table, sort by language/location/site id, and derive site counts from the finished rows. Numeric exact suggestions use `Some(0.0)`, numeric near suggestions use their positive absolute distance, and nonnumeric exact suggestions use `None`.

- [x] **Step 8: Integrate explicit merge options**

Add:

```rust
pub struct MergeOptions {
    pub parent_scope_limit: Option<u32>,
    pub token_inference: TokenInferenceConfig,
}

pub fn merge_language_scans_with_options(
    languages: BTreeMap<LanguageId, ScanFacts>,
    options: &MergeOptions,
) -> Result<MergedScan, ScanFactsError>;
```

Replace Task 1's `build_unassessed_token_inference` stub with this deterministic builder. Keep `merge_language_scans` as a default test convenience. Pass config from `Engine::scan` into the new options and populate `MergedScan.token_inference` after raw facts are recomputed.

- [x] **Step 9: Add end-to-end scan output coverage**

Make a fixture emit a `4px` spacing token, a padding value `3`, and a width `200`. Assert one near row, one unmatched row, tolerance `2`, reconciled counts, and no debt classification for `200`.

- [x] **Step 10: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-core
cargo test -p wax-contract
cargo clippy -p wax-core -p wax-contract --all-targets -- -D warnings
cd ..
git add engine docs/plans/2026-07-19-token-inference-reporting-plan.md
git commit -m "feat: infer token replacement candidates"
```

Expected: all commands exit `0`; merged output validates as schema v3.

---

### Task 3: Emit Parser-Pack Style Context with Parity

- [x] **Task 3 complete**

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/app/src/main/kotlin/Sample.kt`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-compose/tests/golden_small.rs`
- Modify: `engine/crates/wax-lang-react/src/style_extract.rs`
- Modify: `engine/crates/wax-lang-react/tests/fixtures/small/src/Sample.tsx`
- Modify: `engine/crates/wax-lang-react/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-react/tests/golden_small.rs`
- Modify: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/Sample.swift`
- Modify: `engine/crates/wax-lang-swift/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-swift/tests/golden_small.rs`

**Interfaces:**
- Consumes: Task 1's `StyleContext` and Task 2's inference semantics.
- Produces: precise context on every parser-backed hard-coded site with equivalent outcomes for shared concepts.

- [x] **Step 1: Add failing parity assertions**

For every supported ecosystem concept, assert sites for padding, gap/spacing, width, height, radius, typography, color, and elevation. Include a fixed width `200` and assert it remains a raw site. Assert previews/demos and non-style numbers stay absent.

- [x] **Step 2: Confirm all three red states**

```bash
cd engine
cargo test -p wax-lang-compose --test golden_small
cargo test -p wax-lang-react --test golden_small
cargo test -p wax-lang-swift --test golden_small
```

Expected: FAIL because Task 1 used `Unknown` during cutover.

- [x] **Step 3: Implement Compose metadata mapping**

Replace category-only lookup with:

```rust
fn compose_style_metadata(call: &str) -> Option<(TokenCategory, StyleContext)> {
    match call {
        "Color" | "background" => Some((TokenCategory::Color, StyleContext::Color)),
        "padding" => Some((TokenCategory::Spacing, StyleContext::Padding)),
        "size" => Some((TokenCategory::Spacing, StyleContext::Size)),
        "width" => Some((TokenCategory::Spacing, StyleContext::Width)),
        "height" => Some((TokenCategory::Spacing, StyleContext::Height)),
        "spacedBy" => Some((TokenCategory::Spacing, StyleContext::Gap)),
        "fontSize" | "TextStyle" => Some((TokenCategory::Typography, StyleContext::Typography)),
        "clip" | "cornerRadius" | "RoundedCornerShape" => Some((TokenCategory::Radius, StyleContext::Radius)),
        "shadow" | "elevation" => Some((TokenCategory::Elevation, StyleContext::Elevation)),
        _ => None,
    }
}
```

Preserve direct-argument scoping and preview exclusions. Do not map the outer `border` call itself: its arguments can represent color, width, or shape, and nested `Color` values are already detected independently. Defer border-width context until argument-role parsing can distinguish it safely.

Compose does not expose a general margin modifier, so no Compose margin fixture is required. Treat that as an ecosystem capability absence while maintaining parity for concepts shared across the packs.

- [x] **Step 4: Implement React property metadata**

Map padding and margin longhands, `gap`/`rowGap`/`columnGap`, width, height, font properties, radius, colors, and shadows to `(TokenCategory, StyleContext)`. Keep the inline JSX style-object boundary; broader CSS-in-JS remains out of scope.

- [x] **Step 5: Implement Swift call and label metadata**

Carry `(TokenCategory, StyleContext)` through `HardcodedLiteral`. Distinguish `.padding`, stack `spacing`, frame `width`/`height`, font `size`, radius labels, color, and shadow. For `.frame(width: 200, height: 40)`, emit separate sites. Preserve nested-call ownership and longest-range deduplication.

- [x] **Step 6: Add native canonical fixture values**

Use Compose `"4.dp"`, React `"4px"`, and Swift `"4"`. Do not add a `200` token. Pack tests assert raw context only; core tests remain the sole inference-policy tests.

- [x] **Step 7: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose
cargo test -p wax-lang-react
cargo test -p wax-lang-swift
cargo clippy -p wax-lang-compose -p wax-lang-react -p wax-lang-swift --all-targets -- -D warnings
cd ..
git add engine/crates/wax-lang-compose engine/crates/wax-lang-react engine/crates/wax-lang-swift docs/plans/2026-07-19-token-inference-reporting-plan.md
git commit -m "feat: emit token usage context across language packs"
```

Expected: every command exits `0`; context coverage expands without ordinary-literal false positives.

---

### Task 4: Report Deterministic Token Inference

- [x] **Task 4 complete**

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-cli/tests/scan_command.rs`
- Modify: `skills/wax-scan/scripts/extract-insights.sh`
- Modify: `skills/wax-scan/scripts/test-extract-insights.sh`
- Modify: `skills/wax-scan/SKILL.md`
- Modify: `skills/wax-scan/reference.md`
- Modify: `skills/wax-scan/templates/report.html`
- Modify: `scripts/render-wax-scan-fixture-report.sh`
- Modify: `scripts/test-wax-scan-extract-insights.sh`
- Modify: `scripts/test-wax-scan-render-report.sh`
- Modify: `scripts/fixtures/wax-scan/scan-merged.sample.json`
- Modify: `scripts/fixtures/wax-scan/expected-insights.sample.json`
- Modify: `tests/wax-scan-report-screenshot.spec.mjs`
- Modify: `tests/goldens/wax-scan-report-desktop.png`

**Interfaces:**
- Consumes: schema-v3 `MergedScan.token_inference` and raw language token facts.
- Produces: CLI sections and wax-scan insights schema-v3 fields `summary`, `confirmed_candidates`, `possible_candidates`, `unmatched_observations`, and `unassessed_observations`.

- [x] **Step 1: Write failing CLI summary tests**

Construct one row of each classification and assert:

```text
token metrics:
  Token references: 6
  Confirmed migration candidates: 1
  Possible migration candidates: 1
  Unmatched observations: 1 (informational)
  Unassessed observations: 1 (registry values needed)
```

Assert output omits `Token reference ratio` and does not call unmatched or unassessed rows debt.

- [x] **Step 2: Implement the CLI summary cutover**

Read `merged.token_inference.counts`. Print up to five exact/near rows sorted by confidence then source location:

```text
  src/Card.tsx:12 padding 4px -> spacing.s (exact, very high)
```

Before rendering details, build a raw-site index keyed by `(language, site_id)` from each language's `hardcoded_style_sites[]`. Require every inference row to resolve to exactly one raw site and read source location, context, and observed value from that joined raw record. A missing or duplicate join returns an error and suppresses the report instead of printing partial findings.

Keep unmatched rows out of the ranked migration list. When unassessed is nonzero, print `Run wax-registry-discover to review missing canonical token values.` Add a first-run test using a registry whose tokens have no `value`: every raw site is unassessed, no unmatched claim is printed, and the maintenance guidance is present.

- [x] **Step 3: Add a schema-v3 report fixture and failing assertions**

Put one exact, near, unmatched, and unassessed row in `scan-merged.sample.json`. Add:

```bash
assert_eq "confirmed" "$(jq '.token_inference.confirmed_candidates | length' <<<"$ACTUAL")" "1"
assert_eq "possible" "$(jq '.token_inference.possible_candidates | length' <<<"$ACTUAL")" "1"
assert_eq "unmatched" "$(jq '.token_inference.unmatched_observations | length' <<<"$ACTUAL")" "1"
assert_eq "unassessed" "$(jq '.token_inference.unassessed_observations | length' <<<"$ACTUAL")" "1"
```

Expected before extractor changes: FAIL because schema 3 and token inference output are unsupported.

- [x] **Step 4: Update the deterministic extractor**

Require scan schema `3` and set insights schema to `3`. Build a unique raw-site lookup keyed by `(language, site_id)`, reject duplicate raw keys, and fail if any inference row has zero or multiple matches. Enrich each emitted finding with the joined raw `location`, `context`, and observed `value`; do not trust or synthesize those fields from the inference row. Then add the four classification arrays:

```jq
token_inference: {
  summary: .token_inference.counts,
  confirmed_candidates: [$enriched[] | select(.classification == "exact")],
  possible_candidates: [$enriched[] | select(.classification == "near")],
  unmatched_observations: [$enriched[] | select(.classification == "unmatched")],
  unassessed_observations: [$enriched[] | select(.classification == "unassessed")]
}
```

Sort candidates by `very_high`, `high`, `medium`, `low`, then language/file/line. Treat schema-v2 baselines as incompatible because they lack inference classifications.

- [x] **Step 5: Extend HTML rendering with escaped token tables**

Add sections for confirmed token migrations, possible token migrations, and registry metadata gaps. Rows contain the joined raw context, observed value, and source location plus token key, canonical value, distance, confidence, and evidence from inference. Show unmatched only as a secondary informational count. Pass every scan-derived string through `html-escape.sh` before template substitution.

- [x] **Step 6: Update skill and reference semantics**

Add these rules verbatim in substance:

```text
Exact rows are deterministic confirmed migration candidates.
Near rows are deterministic possible migration candidates.
Unmatched rows are informational observations, not debt.
Unassessed rows are registry metadata gaps and may trigger wax-registry-discover.
Never synthesize a replacement confidence that disagrees with token_inference.
Join inference to raw observations by (language, site_id); fail closed if the join is missing or ambiguous.
```

Remove any headline use of the retired token ratio and any combined exact/near debt count. Explain that existing registries without canonical values initially produce unassessed findings until reviewed values are added and a fresh scan runs.

- [x] **Step 7: Verify scripts and rendered HTML**

```bash
scripts/test-wax-scan-extract-insights.sh
scripts/test-wax-scan-html-escape.sh
scripts/test-wax-scan-render-report.sh
scripts/test-wax-scan-integration-smoke.sh
```

Expected: every script prints its PASS summary and no rendered `{{...}}` placeholders remain.

- [x] **Step 8: Regenerate and inspect the screenshot golden**

```bash
npx playwright test tests/wax-scan-report-screenshot.spec.mjs --update-snapshots
npx playwright test tests/wax-scan-report-screenshot.spec.mjs
```

Expected: the second command exits `0`. Inspect the image for overflow, readable confidence labels, and separation of exact, near, and metadata-gap sections.

- [x] **Step 9: Verify CLI and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-cli
cargo clippy -p wax-cli --all-targets -- -D warnings
cd ..
git add engine/crates/wax-cli skills/wax-scan scripts tests docs/plans/2026-07-19-token-inference-reporting-plan.md
git commit -m "feat: report token replacement confidence"
```

Expected: all commands exit `0`; CLI and HTML use the same classifications.

---

### Task 5: Expand Registry Discovery into Reviewed Token Maintenance

- [ ] **Task 5 complete**

**Files:**
- Modify: `skills/wax-registry-discover/SKILL.md`
- Create: `skills/wax-registry-discover/token-value-maintenance.md`
- Create: `skills/wax-registry-discover/examples/token-value-refresh.md`
- Modify: `skills/wax-scan/SKILL.md`
- Modify: `skills/wax-scan/reference.md`
- Create: `scripts/test-wax-registry-skill-contract.sh`

**Interfaces:**
- Consumes: schema-v3 unassessed rows, optional registry token values, remembered upstream resolution, `wax registry discover --dry-run`, `wax validate`, and `wax sync`.
- Produces: a direct or delegated reviewed workflow; it adds no AI dependency to engine scanning or validation.

- [ ] **Step 1: Write a failing skill-contract test**

Create an executable shell test with:

```bash
require_text() {
  local file="$1"
  local text="$2"
  grep -Fq -- "$text" "$file" || {
    echo "FAIL: expected $file to contain: $text" >&2
    exit 1
  }
}

require_text "skills/wax-registry-discover/SKILL.md" "structured diff"
require_text "skills/wax-registry-discover/SKILL.md" "explicit approval"
require_text "skills/wax-registry-discover/SKILL.md" "Never delete"
require_text "skills/wax-registry-discover/token-value-maintenance.md" "source evidence"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Before registry"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Proposed diff"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Explicit approval"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "After registry"
require_text "skills/wax-scan/SKILL.md" "unassessed"
require_text "skills/wax-scan/SKILL.md" "wax-registry-discover"
```

Also extract the labeled before/after registry JSON blocks from the golden workflow into temporary files and use `jq -e` to assert: both parse; the before token has no `value`; the after token has the expected canonical value; ids, keys, categories, aliases, and metadata are unchanged; and the after token/component counts are not lower. This makes the smoke test enforce the promised preservation behavior rather than only checking prose keywords.

Expected before documentation changes: FAIL because the token maintenance reference and golden workflow are missing.

- [ ] **Step 2: Define direct and delegated entry points**

Document:

```text
Direct: discover, update, refresh, or audit a Wax registry.
Delegated: wax-scan finds unassessed observations and offers registry enrichment.
```

Resolve the publisher repo through `design_systems` config or remembered upstream metadata. If it cannot be resolved, stop with instructions; never edit an app-local synced copy as authoritative source.

- [ ] **Step 3: Define canonical-value evidence rules**

Require every proposal to show language, token id/key/category, current value, proposed source-facing value, source file/line, resolution explanation, and confidence. Accept direct constants and traceable simple aliases. Treat computed, runtime, or theme-dependent values as ambiguous and do not flatten modes.

- [ ] **Step 4: Define the reviewed write workflow**

Run deterministic component discovery in preview mode, compare with the current registry, inspect token source, and show separate diff groups for component changes, token additions, values filled, values changed, and potential removals. Require approval for additions/changes and separate approval for removals. Preserve ids, keys, aliases, categories, metadata, and values outside the approved diff. Use `apply_patch` for registry edits.

Add `examples/token-value-refresh.md` as a golden end-to-end workflow showing: a before registry whose token lacks `value`; the source declaration and line used as evidence; a proposed structured diff that fills the canonical value; the explicit approval boundary; the after registry; validation/sync/rescan results; and confirmation that no registry entries were deleted. The skill-contract test must assert these sections exist so verification covers behavior, not only isolated keywords.

- [ ] **Step 5: Define validation, sync, and rerun behavior**

After an approved publisher-registry edit, run:

```bash
wax validate
```

For a delegated app flow with resolvable upstream, return to the app and run:

```bash
wax sync
wax validate
wax scan
```

Then regenerate the report. A failed write or validation leaves the previous registry recoverable and stops before sync.

- [ ] **Step 6: Make wax-scan delegation explicit**

Report unassessed counts, explain missing metadata, offer maintenance, delegate only after acceptance, never insert inferred values directly into metrics, and rerun a fresh scan after successful maintenance. Describe reviewed value maintenance as the unlock from the expected first-run all-unassessed state: only the fresh post-sync scan may reclassify observations as exact, near, or unmatched.

- [ ] **Step 7: Verify and commit**

```bash
chmod +x scripts/test-wax-registry-skill-contract.sh
scripts/test-wax-registry-skill-contract.sh
scripts/test-wax-scan-extract-insights.sh
scripts/test-wax-scan-render-report.sh
git add skills/wax-registry-discover skills/wax-scan scripts/test-wax-registry-skill-contract.sh docs/plans/2026-07-19-token-inference-reporting-plan.md
git commit -m "feat: add reviewed token registry maintenance"
```

Expected: all scripts exit `0` and the skill contract enforces evidence, approval, preservation, no automatic deletion, and delegation.

---

### Task 6: Documentation, Full Verification, and Closeout

- [ ] **Task 6 complete**

**Files:**
- Modify: `README.md`
- Modify: `docs/specs/2026-07-03-token-scanning-design.md`
- Modify: `docs/specs/2026-07-19-token-inference-reporting-design.md`
- Modify: `docs/specs/2026-07-04-registry-sync-config-design.md`
- Modify: `docs/plans/2026-07-19-token-inference-reporting-plan.md`
- Modify: `docs/plans/README.md`
- Create: `docs/adr/2026-07-19-token-inference-reporting.md`
- Modify: `docs/adr/README.md`
- Modify: `docs/plans/archive/README.md`

**Interfaces:**
- Consumes: all completed implementation tasks and verified behavior.
- Produces: current setup/config/report docs, accepted ADR, archived plan, and full-workspace evidence.

- [ ] **Step 1: Record shipped names before editing docs**

Use this consistency checklist:

```text
scan schema: 3
config: token_inference.numeric_tolerance
default: 2; exact-only: 0
classifications: exact, near, unmatched, unassessed
confidence: very_high, high, medium, low
registry field: value
retired metric: token_reference_ratio
```

- [ ] **Step 2: Update user-facing setup and config documentation**

Add registry and tolerance examples. Explain confirmed, possible, informational, and unassessed findings. State that fixed dimensions remain visible but are not debt without exact/near registry evidence.

- [ ] **Step 3: Update related specs without rewriting v1 history**

Mark token-scanning v1's replacement/context non-goals as delivered by this addendum. Link registry sync to skill-assisted refresh. Change the new design to `Accepted (implemented)` and link its plan/ADR.

- [ ] **Step 4: Create the ADR**

Record pack-owned context, core-owned inference, one optional canonical value, separate classifications, light context confidence, tolerance `2`, schema-v3 ratio removal, and reviewed registry writes. Add implementation PR links once known.

- [ ] **Step 5: Run release/report checks**

```bash
scripts/test-generate-pack-index.sh
scripts/install.sh --help
scripts/test-wax-scan-extract-insights.sh
scripts/test-wax-scan-html-escape.sh
scripts/test-wax-scan-render-report.sh
scripts/test-wax-scan-integration-smoke.sh
scripts/test-wax-registry-skill-contract.sh
```

Expected: every command exits `0` and each test prints PASS.

- [ ] **Step 6: Run full Rust verification**

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: formatting is clean, all tests pass, and clippy emits no warnings.

- [ ] **Step 7: Audit contract and documentation drift**

```bash
rg -n 'token_reference_ratio' engine skills scripts README.md docs \
  --glob '!docs/adr/2026-07-03-token-scanning.md' \
  --glob '!docs/specs/2026-07-03-token-scanning-design.md' \
  --glob '!docs/plans/archive/**'
rg -n 'T(BD)|TO(DO)|FIX(ME)' docs/specs/2026-07-19-token-inference-reporting-design.md docs/plans/2026-07-19-token-inference-reporting-plan.md docs/adr/2026-07-19-token-inference-reporting.md
```

Expected: only explicit migration/history references remain in the first result; the second returns no matches.

- [ ] **Step 8: Close the roadmap and archive the plan**

After all task PRs merge, tick every checkbox, move this file to `docs/plans/archive/2026-07-19-token-inference-reporting-plan.md`, update the archive index, and set roadmap statuses to `merged` and `complete`. Do not change the active plan until roadmap gates permit it.

- [ ] **Step 9: Commit closeout**

```bash
git add README.md docs engine/crates/wax-contract/schemas skills
git commit -m "docs: close token inference and reporting plan"
```

---

## Final Acceptance Checklist

- [ ] Every raw parser-backed hard-coded observation survives with typed context.
- [ ] Optional canonical values round-trip and empty values fail validation.
- [ ] Core emits exactly one deterministic inference row per observation.
- [ ] Exact, near, unmatched, and unassessed counts reconcile and remain separate.
- [ ] A fixed unmatched width such as `200px` is informational, not migration debt.
- [ ] Default tolerance is `2`, custom tolerance is reproducible, and `0` disables near matching.
- [ ] Context changes confidence only and tied suggestions remain visible.
- [ ] CLI, JSON, terminal skill output, and HTML consume the same classifications.
- [ ] Registry maintenance requires evidence, diff review, approval, and no automatic deletion.
- [ ] Compose, React, and Swift fixtures demonstrate context parity.
- [ ] All focused scripts, workspace tests, formatting, and clippy pass.
