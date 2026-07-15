# Token Scanning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add additive design-token scan facts for all current Wax language packs, with parser-backed hard-coded styling candidates and basic-pack token reference matching.

**Architecture:** Extend `wax-contract` with a separate token fact family, then add shared registry token parsing in `wax-lang-api`. Update each pack to load per-language registry tokens and emit token facts, while `wax-core` recomputes token counts, summaries, and repo-level metrics from raw facts.

**Tech Stack:** Rust workspace under `engine/`, `serde` JSON contracts, JSON Schema, tree-sitter Kotlin and Swift scanners, SWC React scanner, existing golden fixtures, existing `wax scan` CLI summary.

## Global Constraints

- Token facts are separate from component `usage_sites[]`; do not model tokens as UI invocations.
- Existing component registry entries, component usage facts, and invocation metrics keep their current meanings.
- Registries with no `tokens` key or an empty `tokens` array remain valid.
- Token registry matching uses exact `key` and exact `aliases`; do not add regex or pattern-based token matching.
- Do not add suggested replacement tokens, fuzzy value matching, unit normalization, theme-mode matching, cross-platform token equivalence, or a token `style_context` field.
- The basic text scanner emits token references only and must not emit hard-coded styling candidates.
- Parser-backed packs emit hard-coded styling candidates only in styling contexts and must stay conservative.
- Reuse `ParentScope` for token facts when parent attribution is available.
- `wax-core` owns derived token counts, token summaries, and token ratios.
- v1 reporting is CLI-only (`wax scan` terminal summary). wax-scan HTML templates, baseline fixtures, and branded report updates are follow-on work.
- `wax validate` registry token entry validation (duplicate ids, empty keys, bad categories) is follow-on work; scan-time contract validation covers emitted facts.
- Registry discovery or skill-assisted population of `tokens[]` is out of scope for v1; token registries are authored or synced explicitly.
- Run `cargo fmt --all` before committing Rust changes.

---

## File Structure

- Modify `engine/crates/wax-contract/src/lib.rs`: add `TokenCategory`, `DesignSystemToken`, `TokenSite`, `HardcodedStyleSite`, `TokenCounts`, `TokenCategoryCounts`, `TokenUsageSummary`, token fields on `ScanFacts` and `MergedScan`, token ratio on `Metrics`, validation, recomputation, and merge helpers.
- Modify `engine/crates/wax-contract/schemas/scan-facts.schema.json`: publish token facts, counts, summaries, and metric schema.
- Modify `engine/crates/wax-contract/tests/schema_roundtrip.rs`: add contract and schema tests for token facts and invalid token references.
- Create `engine/crates/wax-lang-api/src/token_registry.rs`: shared registry token parser, exact key/alias resolver, and source text matcher utilities.
- Modify `engine/crates/wax-lang-api/src/lib.rs`: export token registry helpers.
- Modify `engine/crates/wax-lang-api/tests/token_registry.rs`: test registry token parsing and exact matching.
- Modify `engine/crates/wax-lang-basic/src/line_scan.rs`: load registry tokens and emit `token_sites[]` by exact string match.
- Modify `engine/crates/wax-lang-basic/src/lib.rs`: initialize new token fields for configured and scaffold facts.
- Modify `engine/crates/wax-lang-basic/tests/fixtures/small/`: add token registry entries and source references.
- Modify `engine/crates/wax-lang-basic/tests/golden_small.rs`: assert token references and no hard-coded style facts.
- Modify `engine/crates/wax-core/src/adoption_merge.rs`: sum token counts, compute `token_reference_ratio`, build merged token summaries.
- Modify `engine/crates/wax-core/tests/scan_output.rs`: assert merged token output.
- Modify `engine/crates/wax-core/tests/subprocess_protocol.rs`: add default-empty token fields to every canned `ScanFacts` JSON object that is manually serialized in the test file.
- Modify `engine/crates/wax-cli/src/commands/scan.rs`: print factual token counts and token reference ratio.
- Modify `engine/crates/wax-cli/tests/scan_command.rs`: assert CLI token summary output.
- Modify `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`: load tokens, emit token references, emit Compose hard-coded styling candidates, and reuse Compose parent attribution.
- Modify `engine/crates/wax-lang-compose/tests/fixtures/small/`: add token references and hard-coded styling examples.
- Modify `engine/crates/wax-lang-compose/tests/golden_small.rs`: assert token facts and hard-coded candidate facts.
- Modify `engine/crates/wax-lang-react/src/registry.rs`: load tokens into `ReactRegistryIndex`.
- Modify `engine/crates/wax-lang-react/src/extract.rs`: emit token references, JSX/CSS-in-JS hard-coded styling candidates, and React parent attribution.
- Modify `engine/crates/wax-lang-react/tests/fixtures/small/`: add token references and hard-coded styling examples.
- Modify `engine/crates/wax-lang-react/tests/golden_small.rs`: assert token facts and hard-coded candidate facts.
- Modify `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`: load tokens, emit token references, SwiftUI hard-coded styling candidates, and SwiftUI parent attribution.
- Modify `engine/crates/wax-lang-swift/tests/fixtures/small/`: add token references and hard-coded styling examples.
- Modify `engine/crates/wax-lang-swift/tests/golden_small.rs`: assert token facts and hard-coded candidate facts.
- Modify `docs/specs/2026-05-13-component-tracker-design.md`: link to the token scanning design as the concrete token fact implementation direction.
- Modify `docs/specs/2026-06-20-adoption-metrics-v2-design.md`: update future fact family note to point at this design.
- Modify `docs/specs/2026-07-03-token-scanning-design.md`: keep the implementation plan link current.

---

### Task 1: Extend the Shared Contract

**Files:**
- Modify: `engine/crates/wax-contract/src/lib.rs`
- Modify: `engine/crates/wax-contract/schemas/scan-facts.schema.json`
- Modify: `engine/crates/wax-contract/tests/schema_roundtrip.rs`

**Interfaces:**
- Produces:
  - `pub enum TokenCategory`
  - `pub struct DesignSystemToken`
  - `pub struct TokenSite`
  - `pub struct HardcodedStyleSite`
  - `pub struct TokenCategoryCounts`
  - `pub struct TokenCounts`
  - `pub struct TokenUsageSummary`
  - `ScanFacts.design_system_tokens`
  - `ScanFacts.token_sites`
  - `ScanFacts.hardcoded_style_sites`
  - `ScanFacts.token_usage_summary`
  - `MergedScan.token_usage_summary`
  - `Metrics.token_reference_ratio`
  - `CountSummary.tokens`
- Consumes: existing `SourceLocation`, `ParentScope`, `Metrics`, `CountSummary`, `ScanFacts::validate`, and `ScanFacts::recompute_counts`.

- [x] **Step 1: Add failing token contract tests**

Add this test to `engine/crates/wax-contract/tests/schema_roundtrip.rs`:

```rust
#[test]
fn token_facts_roundtrip_and_validate_against_schema() {
    let mut facts = minimal_facts();
    facts.design_system_tokens = vec![
        wax_contract::DesignSystemToken {
            id: "color.primary".into(),
            key: "Theme.colors.primary".into(),
            category: wax_contract::TokenCategory::Color,
            aliases: vec!["AppColors.Primary".into()],
        },
        wax_contract::DesignSystemToken {
            id: "space.medium".into(),
            key: "Spacing.Medium".into(),
            category: wax_contract::TokenCategory::Spacing,
            aliases: vec![],
        },
    ];
    facts.token_sites = vec![wax_contract::TokenSite {
        id: "token.compose:src/Screen.kt:8:13:color.primary".into(),
        location: SourceLocation {
            file: "src/Screen.kt".into(),
            line: 8,
            column: Some(13),
        },
        token_id: "color.primary".into(),
        key: "AppColors.Primary".into(),
        category: wax_contract::TokenCategory::Color,
        parent: Some(ParentScope {
            parent_id: "compose:composable:com.example.Screen".into(),
            symbol: "Screen".into(),
            qualified_symbol: Some("com.example.Screen".into()),
            scope_kind: "composable".into(),
            identity_basis: "package_qualified_symbol".into(),
            identity_stability: IdentityStability::Semantic,
            location: Some(SourceLocation {
                file: "src/Screen.kt".into(),
                line: 4,
                column: Some(1),
            }),
        }),
    }];
    facts.hardcoded_style_sites = vec![wax_contract::HardcodedStyleSite {
        id: "hardcoded.compose:src/Screen.kt:9:22:spacing".into(),
        location: SourceLocation {
            file: "src/Screen.kt".into(),
            line: 9,
            column: Some(22),
        },
        value: "8.dp".into(),
        category: wax_contract::TokenCategory::Spacing,
        parent: None,
    }];
    facts.recompute_counts().unwrap();

    let json = serde_json::to_string(&facts).unwrap();
    let back = wax_contract::scan_facts_from_json(&json).unwrap();
    assert_eq!(back.design_system_tokens.len(), 2);
    assert_eq!(back.token_sites.len(), 1);
    assert_eq!(back.hardcoded_style_sites.len(), 1);
    assert_eq!(back.counts.tokens.configured_token_count, 2);
    assert_eq!(back.counts.tokens.used_token_count, 1);
    assert_eq!(back.counts.tokens.token_reference_site_count, 1);
    assert_eq!(back.counts.tokens.hardcoded_style_candidate_count, 1);
    assert_eq!(back.counts.tokens.token_references_by_category.color, 1);
    assert_eq!(back.counts.tokens.hardcoded_by_category.spacing, 1);
    assert_eq!(back.metrics.token_reference_ratio, Some(0.5));

    let value = serde_json::to_value(&back).unwrap();
    assert!(scan_facts_schema().is_valid(&value));
}
```

Expected before implementation: compilation fails because the token types and fields do not exist.

- [x] **Step 2: Add invalid token validation tests**

Add these tests to `engine/crates/wax-contract/tests/schema_roundtrip.rs`:

```rust
#[test]
fn token_site_must_reference_known_token_id() {
    let mut facts = minimal_facts();
    facts.token_sites = vec![wax_contract::TokenSite {
        id: "token.react:src/App.tsx:1:1:missing".into(),
        location: SourceLocation {
            file: "src/App.tsx".into(),
            line: 1,
            column: Some(1),
        },
        token_id: "missing".into(),
        key: "theme.colors.missing".into(),
        category: wax_contract::TokenCategory::Color,
        parent: None,
    }];

    let err = facts.recompute_counts().expect_err("unknown token id must fail");
    assert!(err.to_string().contains("token_id"));
}

#[test]
fn token_site_key_must_match_key_or_alias() {
    let mut facts = minimal_facts();
    facts.design_system_tokens = vec![wax_contract::DesignSystemToken {
        id: "color.primary".into(),
        key: "theme.colors.primary".into(),
        category: wax_contract::TokenCategory::Color,
        aliases: vec!["colors.primary".into()],
    }];
    facts.token_sites = vec![wax_contract::TokenSite {
        id: "token.react:src/App.tsx:1:1:color.primary".into(),
        location: SourceLocation {
            file: "src/App.tsx".into(),
            line: 1,
            column: Some(1),
        },
        token_id: "color.primary".into(),
        key: "wrong.primary".into(),
        category: wax_contract::TokenCategory::Color,
        parent: None,
    }];

    let err = facts.recompute_counts().expect_err("wrong matched key must fail");
    assert!(err.to_string().contains("key"));
}

#[test]
fn hardcoded_style_site_requires_non_empty_value() {
    let mut facts = minimal_facts();
    facts.hardcoded_style_sites = vec![wax_contract::HardcodedStyleSite {
        id: "hardcoded.react:src/App.tsx:1:1:color".into(),
        location: SourceLocation {
            file: "src/App.tsx".into(),
            line: 1,
            column: Some(1),
        },
        value: "".into(),
        category: wax_contract::TokenCategory::Color,
        parent: None,
    }];

    let err = facts.recompute_counts().expect_err("empty hardcoded value must fail");
    assert!(err.to_string().contains("value"));
}
```

Expected before implementation: compilation fails because the token types and fields do not exist.

- [x] **Step 3: Add token contract types**

Modify `engine/crates/wax-contract/src/lib.rs` near the existing fact structs:

```rust
/// Design token category used for token references and hard-coded styling candidates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TokenCategory {
    /// Color token or hard-coded color candidate.
    Color,
    /// Spacing, sizing, padding, margin, or gap token or candidate.
    Spacing,
    /// Typography token or hard-coded typography candidate.
    Typography,
    /// Radius or shape token or candidate.
    Radius,
    /// Elevation, shadow, or z-depth token or candidate.
    Elevation,
    /// Known token whose category is not classified.
    Unknown,
}

/// Design-system token known to a language pack from its registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesignSystemToken {
    /// Stable token id within the language registry.
    pub id: String,
    /// Exact source-facing token key.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
    /// Exact source-facing aliases for the same token.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// Source reference to a known design token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TokenSite {
    /// Stable token reference site id.
    pub id: String,
    /// Source location where the token reference appears.
    pub location: SourceLocation,
    /// Referenced token id from `design_system_tokens`.
    pub token_id: String,
    /// Exact key or alias matched in source.
    pub key: String,
    /// Token category copied from the matched token.
    pub category: TokenCategory,
    /// Parent scope attribution when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentScope>,
}

/// Hard-coded styling literal detected in a styling context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HardcodedStyleSite {
    /// Stable hard-coded style site id.
    pub id: String,
    /// Source location where the literal appears.
    pub location: SourceLocation,
    /// Source text for the hard-coded styling value.
    pub value: String,
    /// Styling category inferred from context.
    pub category: TokenCategory,
    /// Parent scope attribution when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentScope>,
}
```

- [x] **Step 4: Add token fields to `ScanFacts`, `Metrics`, and `CountSummary`**

Modify `ScanFacts`:

```rust
/// Known design-system tokens loaded for this language.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub design_system_tokens: Vec<DesignSystemToken>,
/// Known token references discovered in source.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub token_sites: Vec<TokenSite>,
/// Hard-coded styling candidates discovered in source.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub hardcoded_style_sites: Vec<HardcodedStyleSite>,
/// Derived per-token summaries when emitted by the engine or pack.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub token_usage_summary: Vec<TokenUsageSummary>,
```

Modify `Metrics`:

```rust
/// Known token references divided by token references plus hard-coded styling candidates.
pub token_reference_ratio: Option<f64>,
```

Add count structs:

```rust
/// Token counts grouped by category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenCategoryCounts {
    /// Color token references or candidates.
    pub color: u32,
    /// Spacing token references or candidates.
    pub spacing: u32,
    /// Typography token references or candidates.
    pub typography: u32,
    /// Radius token references or candidates.
    pub radius: u32,
    /// Elevation token references or candidates.
    pub elevation: u32,
    /// Unknown-category token references or candidates.
    pub unknown: u32,
}

/// Design-token scan counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TokenCounts {
    /// Number of configured design-system tokens.
    pub configured_token_count: u32,
    /// Number of distinct configured tokens with at least one reference.
    pub used_token_count: u32,
    /// Number of known token reference sites.
    pub token_reference_site_count: u32,
    /// Number of hard-coded styling candidates.
    pub hardcoded_style_candidate_count: u32,
    /// Token reference counts grouped by category.
    pub token_references_by_category: TokenCategoryCounts,
    /// Hard-coded styling candidate counts grouped by category.
    pub hardcoded_by_category: TokenCategoryCounts,
    /// Number of parent scopes containing at least one token reference.
    pub parent_scopes_with_token_references: u32,
    /// Number of parent scopes containing at least one hard-coded styling candidate.
    pub parent_scopes_with_hardcoded_candidates: u32,
}
```

Modify `CountSummary`:

```rust
/// Design-token scan counters.
#[serde(default)]
pub tokens: TokenCounts,
```

When deserializing older scan JSON without `counts.tokens`, `TokenCounts::default()` must apply so additive fields remain backward-compatible at the contract boundary.

Add summary struct:

```rust
/// Derived per-token usage summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TokenUsageSummary {
    /// Language pack that owns this token summary row.
    pub language: String,
    /// Token id represented by this summary row.
    pub token_id: String,
    /// Exact registry key for the token.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
    /// Number of token references to this token.
    pub reference_count: u32,
    /// Number of files containing references to this token.
    pub file_count: u32,
    /// Number of parent scopes containing references to this token.
    pub parent_scope_count: u32,
}
```

Modify `MergedScan`:

```rust
/// Derived per-token summaries across merged languages.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub token_usage_summary: Vec<TokenUsageSummary>,
```

- [x] **Step 5: Update null field handling and defaults**

Add `&["metrics", "token_reference_ratio"]` to `NULLABLE_JSON_FIELDS`.

Update all test constructors in `engine/crates/wax-contract/tests/schema_roundtrip.rs` and in downstream crates that manually construct `Metrics` to include:

```rust
token_reference_ratio: None,
```

Update all manual `ScanFacts` constructors to include empty token vectors when serde defaults are not involved:

```rust
design_system_tokens: vec![],
token_sites: vec![],
hardcoded_style_sites: vec![],
token_usage_summary: vec![],
```

- [x] **Step 6: Implement token validation and recomputation**

In `engine/crates/wax-contract/src/lib.rs`, add helper functions:

```rust
fn increment_token_category(
    field: &str,
    counts: &mut TokenCategoryCounts,
    category: TokenCategory,
) -> Result<(), ScanFactsError> {
    let slot = match category {
        TokenCategory::Color => &mut counts.color,
        TokenCategory::Spacing => &mut counts.spacing,
        TokenCategory::Typography => &mut counts.typography,
        TokenCategory::Radius => &mut counts.radius,
        TokenCategory::Elevation => &mut counts.elevation,
        TokenCategory::Unknown => &mut counts.unknown,
    };
    increment_count(field, slot)
}
```

Extend `derive_counts_and_metrics` so it:

- indexes `facts.design_system_tokens` by `id`
- rejects duplicate token ids
- validates non-empty token `id`, `key`, and aliases
- validates every `TokenSite.token_id`
- validates `TokenSite.key` equals the token `key` or appears in `aliases`
- validates `TokenSite.category == DesignSystemToken.category`
- validates every hard-coded style `value` is non-empty
- counts used token ids, token reference categories, hard-coded categories, and token parent scopes
- computes `token_reference_ratio` as `token_reference_site_count / (token_reference_site_count + hardcoded_style_candidate_count)`

Use these exact validation messages for stable tests:

```rust
"token id must not be empty"
"token key must not be empty"
"token alias must not be empty"
"duplicate token id"
"token_id must reference design_system_tokens"
"key must match token key or alias"
"category must match referenced token category"
"value must not be empty"
```

- [x] **Step 7: Update JSON schema**

Modify `engine/crates/wax-contract/schemas/scan-facts.schema.json` to include:

- `TokenCategory` enum values: `color`, `spacing`, `typography`, `radius`, `elevation`, `unknown`
- `design_system_tokens[]`
- `token_sites[]`
- `hardcoded_style_sites[]`
- `counts.tokens`
- `metrics.token_reference_ratio`
- `token_usage_summary[]`

Keep token arrays default-empty from the Rust side but represented as arrays when present in JSON.

- [x] **Step 8: Run contract checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-contract
```

Expected: `cargo test -p wax-contract` passes.

- [x] **Step 9: Commit Task 1**

```bash
git add engine/crates/wax-contract/src/lib.rs engine/crates/wax-contract/schemas/scan-facts.schema.json engine/crates/wax-contract/tests/schema_roundtrip.rs
git commit -m "feat: add token scan contract"
```

---

### Task 2: Add Shared Token Registry Parsing

**Files:**
- Create: `engine/crates/wax-lang-api/src/token_registry.rs`
- Create: `engine/crates/wax-lang-api/tests/token_registry.rs`
- Modify: `engine/crates/wax-lang-api/src/lib.rs`

**Interfaces:**
- Consumes: `wax_contract::{DesignSystemToken, TokenCategory, TokenSite, SourceLocation}`
- Produces:
  - `pub struct RegistryTokenIndex`
  - `pub struct TokenMatch`
  - `pub fn parse_registry_tokens(value: &serde_json::Value) -> Result<Vec<DesignSystemToken>, TokenRegistryError>`
  - `pub fn token_index(tokens: &[DesignSystemToken]) -> Result<RegistryTokenIndex, TokenRegistryError>`
  - `pub fn find_token_matches(source: &str, file: &str, index: &RegistryTokenIndex, id_prefix: &str) -> Vec<TokenSite>`

- [x] **Step 1: Write failing tests for token registry parsing**

Create `engine/crates/wax-lang-api/tests/token_registry.rs`:

```rust
use wax_contract::TokenCategory;
use wax_lang_api::{find_token_matches, parse_registry_tokens, token_index};

#[test]
fn parses_tokens_with_aliases() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color",
                "aliases": ["AppColors.Primary"]
            }
        ]
    });

    let tokens = parse_registry_tokens(&value).expect("tokens should parse");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].id, "color.primary");
    assert_eq!(tokens[0].key, "Theme.colors.primary");
    assert_eq!(tokens[0].category, TokenCategory::Color);
    assert_eq!(tokens[0].aliases, vec!["AppColors.Primary"]);
}

#[test]
fn missing_tokens_key_is_empty() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}]
    });

    let tokens = parse_registry_tokens(&value).expect("missing tokens should be valid");
    assert!(tokens.is_empty());
}

#[test]
fn token_index_finds_key_and_alias_matches() {
    let value = serde_json::json!({
        "schema_version": 1,
        "components": [{"symbol": "Button"}],
        "tokens": [
            {
                "id": "color.primary",
                "key": "Theme.colors.primary",
                "category": "color",
                "aliases": ["AppColors.Primary"]
            }
        ]
    });
    let tokens = parse_registry_tokens(&value).unwrap();
    let index = token_index(&tokens).unwrap();

    let sites = find_token_matches(
        "val a = Theme.colors.primary\nval b = AppColors.Primary\n",
        "src/Screen.kt",
        &index,
        "token.compose",
    );

    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].token_id, "color.primary");
    assert_eq!(sites[0].key, "Theme.colors.primary");
    assert_eq!(sites[0].category, TokenCategory::Color);
    assert_eq!(sites[0].location.line, 1);
    assert_eq!(sites[1].key, "AppColors.Primary");
    assert_eq!(sites[1].location.line, 2);
}
```

Expected before implementation: compilation fails because token registry helpers do not exist.

- [x] **Step 2: Implement shared parser and matcher**

Create `engine/crates/wax-lang-api/src/token_registry.rs`:

```rust
//! Shared token registry parsing and exact token reference matching.

use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use wax_contract::{DesignSystemToken, SourceLocation, TokenCategory, TokenSite};

/// Errors returned while parsing registry token definitions.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TokenRegistryError {
    /// `tokens` exists but is not an array.
    #[error("tokens must be an array")]
    TokensNotArray,
    /// A token field is missing or invalid.
    #[error("tokens[{index}].{field} {reason}")]
    InvalidTokenField {
        /// Token array index.
        index: usize,
        /// Invalid field name.
        field: &'static str,
        /// Human-readable reason.
        reason: String,
    },
    /// Two token entries declare the same id.
    #[error("duplicate token id {id}")]
    DuplicateTokenId {
        /// Duplicate token id.
        id: String,
    },
    /// The same key or alias points at two different token ids.
    #[error("duplicate token match key {key}")]
    DuplicateMatchKey {
        /// Duplicate key or alias.
        key: String,
    },
}

/// Exact token reference match found in source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMatch {
    /// Matched token id.
    pub token_id: String,
    /// Exact matched key or alias.
    pub key: String,
    /// Token category.
    pub category: TokenCategory,
}

/// Lookup table for exact token key and alias matching.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistryTokenIndex {
    /// Tokens in deterministic registry order.
    pub tokens: Vec<DesignSystemToken>,
    /// Key or alias to token match.
    pub matches: BTreeMap<String, TokenMatch>,
}

/// Parses optional `tokens` from a registry JSON value.
pub fn parse_registry_tokens(
    value: &serde_json::Value,
) -> Result<Vec<DesignSystemToken>, TokenRegistryError> {
    let Some(tokens_value) = value.get("tokens") else {
        return Ok(Vec::new());
    };
    let tokens_array = tokens_value
        .as_array()
        .ok_or(TokenRegistryError::TokensNotArray)?;
    let mut tokens = Vec::with_capacity(tokens_array.len());
    let mut seen_ids = BTreeSet::new();
    for (index, token) in tokens_array.iter().enumerate() {
        let id = required_non_empty_string(token, index, "id")?;
        if !seen_ids.insert(id.to_owned()) {
            return Err(TokenRegistryError::DuplicateTokenId { id: id.to_owned() });
        }
        let key = required_non_empty_string(token, index, "key")?;
        let category = parse_category(required_non_empty_string(token, index, "category")?)
            .map_err(|reason| TokenRegistryError::InvalidTokenField {
                index,
                field: "category",
                reason,
            })?;
        let aliases = parse_aliases(token, index)?;
        tokens.push(DesignSystemToken {
            id: id.to_owned(),
            key: key.to_owned(),
            category,
            aliases,
        });
    }
    tokens.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(tokens)
}

/// Builds an exact key and alias lookup index.
pub fn token_index(tokens: &[DesignSystemToken]) -> Result<RegistryTokenIndex, TokenRegistryError> {
    let mut matches = BTreeMap::new();
    for token in tokens {
        insert_match(&mut matches, token, &token.key)?;
        for alias in &token.aliases {
            insert_match(&mut matches, token, alias)?;
        }
    }
    Ok(RegistryTokenIndex {
        tokens: tokens.to_vec(),
        matches,
    })
}

/// Finds exact token key and alias references in source text.
///
/// Used by the basic pack only. Matching is naive substring search per line;
/// false positives inside longer identifiers are a known v1 limitation.
pub fn find_token_matches(
    source: &str,
    file: &str,
    index: &RegistryTokenIndex,
    id_prefix: &str,
) -> Vec<TokenSite> {
    let mut sites = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
        for (key, token_match) in &index.matches {
            let mut search_from = 0;
            while let Some(offset) = line[search_from..].find(key) {
                let start = search_from + offset;
                let line_number = u32::try_from(line_index + 1).unwrap_or(u32::MAX);
                let column = u32::try_from(start + 1).unwrap_or(u32::MAX);
                sites.push(TokenSite {
                    id: format!(
                        "{id_prefix}:{file}:{line_number}:{column}:{}",
                        token_match.token_id
                    ),
                    location: SourceLocation {
                        file: file.to_owned(),
                        line: line_number,
                        column: Some(column),
                    },
                    token_id: token_match.token_id.clone(),
                    key: key.clone(),
                    category: token_match.category,
                    parent: None,
                });
                search_from = start + key.len();
            }
        }
    }
    sites.sort_by(|left, right| {
        left.location
            .file
            .cmp(&right.location.file)
            .then(left.location.line.cmp(&right.location.line))
            .then(left.location.column.cmp(&right.location.column))
            .then(left.token_id.cmp(&right.token_id))
    });
    sites
}
```

Also include private helper functions in the same file:

```rust
fn required_non_empty_string<'a>(
    token: &'a serde_json::Value,
    index: usize,
    field: &'static str,
) -> Result<&'a str, TokenRegistryError> {
    let value = token
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| TokenRegistryError::InvalidTokenField {
            index,
            field,
            reason: "must be a string".to_owned(),
        })?;
    if value.is_empty() {
        return Err(TokenRegistryError::InvalidTokenField {
            index,
            field,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(value)
}

fn parse_aliases(
    token: &serde_json::Value,
    index: usize,
) -> Result<Vec<String>, TokenRegistryError> {
    let Some(aliases_value) = token.get("aliases") else {
        return Ok(Vec::new());
    };
    let aliases = aliases_value
        .as_array()
        .ok_or_else(|| TokenRegistryError::InvalidTokenField {
            index,
            field: "aliases",
            reason: "must be an array".to_owned(),
        })?;
    let mut parsed = Vec::with_capacity(aliases.len());
    for (alias_index, alias) in aliases.iter().enumerate() {
        let alias = alias.as_str().ok_or_else(|| TokenRegistryError::InvalidTokenField {
            index,
            field: "aliases",
            reason: format!("aliases[{alias_index}] must be a string"),
        })?;
        if alias.is_empty() {
            return Err(TokenRegistryError::InvalidTokenField {
                index,
                field: "aliases",
                reason: format!("aliases[{alias_index}] must not be empty"),
            });
        }
        parsed.push(alias.to_owned());
    }
    Ok(parsed)
}

fn parse_category(value: &str) -> Result<TokenCategory, String> {
    match value {
        "color" => Ok(TokenCategory::Color),
        "spacing" => Ok(TokenCategory::Spacing),
        "typography" => Ok(TokenCategory::Typography),
        "radius" => Ok(TokenCategory::Radius),
        "elevation" => Ok(TokenCategory::Elevation),
        "unknown" => Ok(TokenCategory::Unknown),
        other => Err(format!("unsupported category {other:?}")),
    }
}

fn insert_match(
    matches: &mut BTreeMap<String, TokenMatch>,
    token: &DesignSystemToken,
    key: &str,
) -> Result<(), TokenRegistryError> {
    if matches.contains_key(key) {
        return Err(TokenRegistryError::DuplicateMatchKey { key: key.to_owned() });
    }
    matches.insert(
        key.to_owned(),
        TokenMatch {
            token_id: token.id.clone(),
            key: key.to_owned(),
            category: token.category,
        },
    );
    Ok(())
}
```

- [x] **Step 3: Export helper APIs**

Modify `engine/crates/wax-lang-api/src/lib.rs`:

```rust
pub mod token_registry;
```

and:

```rust
pub use token_registry::{
    RegistryTokenIndex, TokenMatch, TokenRegistryError, find_token_matches, parse_registry_tokens,
    token_index,
};
```

- [x] **Step 4: Run shared helper tests**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-api
```

Expected: `cargo test -p wax-lang-api` passes.

- [x] **Step 5: Commit Task 2**

```bash
git add engine/crates/wax-lang-api/src/lib.rs engine/crates/wax-lang-api/src/token_registry.rs engine/crates/wax-lang-api/tests/token_registry.rs
git commit -m "feat: parse registry tokens"
```

---

### Task 3: Emit Basic Pack Token References

**Files:**
- Modify: `engine/crates/wax-lang-basic/src/line_scan.rs`
- Modify: `engine/crates/wax-lang-basic/src/lib.rs`
- Modify: `engine/crates/wax-lang-basic/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-basic/tests/fixtures/small/golden.json`
- Modify: `engine/crates/wax-lang-basic/tests/golden_small.rs`

**Interfaces:**
- Consumes: `wax_lang_api::{find_token_matches, parse_registry_tokens, token_index, RegistryTokenIndex}`
- Produces: `LineScanResult.design_system_tokens`, `LineScanResult.token_sites`, no `hardcoded_style_sites`

- [x] **Step 1: Write failing basic pack assertions**

In `engine/crates/wax-lang-basic/tests/golden_small.rs`, add assertions after the scan result is loaded:

```rust
assert_eq!(facts.design_system_tokens.len(), 2);
assert!(
    facts
        .token_sites
        .iter()
        .any(|site| site.token_id == "color.primary" && site.key == "Theme.colors.primary"),
    "basic scanner should find exact token key references"
);
assert!(
    facts
        .token_sites
        .iter()
        .any(|site| site.token_id == "space.medium" && site.key == "Spacing.Medium"),
    "basic scanner should find exact token references"
);
assert!(
    facts.hardcoded_style_sites.is_empty(),
    "basic scanner must not emit hard-coded styling candidates"
);
```

Expected before implementation: compilation fails or assertions fail because basic facts do not carry token data.

- [x] **Step 2: Add token fixtures**

Modify `engine/crates/wax-lang-basic/tests/fixtures/small/design-system/registry.json` so the top-level object contains:

```json
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
```

Add source references in `engine/crates/wax-lang-basic/tests/fixtures/small/app/src/Sample.src`:

```text
Theme.colors.primary
Spacing.Medium
```

Do not add hard-coded color or spacing values to the basic golden fixture.

- [x] **Step 3: Extend `LineScanResult` and registry index**

Modify imports in `engine/crates/wax-lang-basic/src/line_scan.rs`:

```rust
use wax_contract::{
    DesignSystemComponent, DesignSystemToken, Diagnostic, DiagnosticSeverity, MatchStatus,
    ScanStatus, SourceLocation, TokenSite, UsageSite,
};
use wax_lang_api::{
    RegistryTokenIndex, RootPatternKind, RootResolutionError, ScanConfig, find_token_matches,
    parse_registry_tokens, resolve_source_roots, token_index,
};
```

Modify `LineScanResult`:

```rust
/// Known design-system tokens from the registry file.
pub design_system_tokens: Vec<DesignSystemToken>,
/// Known token references matched in source.
pub token_sites: Vec<TokenSite>,
```

Modify `RegistryIndex`:

```rust
tokens: Vec<DesignSystemToken>,
token_index: RegistryTokenIndex,
```

- [x] **Step 4: Parse tokens in `load_registry`**

After parsing `value`, add:

```rust
let tokens = parse_registry_tokens(&value).map_err(|err| LineScanError::RegistryInvalid {
    path: path.to_path_buf(),
    reason: err.to_string(),
})?;
let token_index = token_index(&tokens).map_err(|err| LineScanError::RegistryInvalid {
    path: path.to_path_buf(),
    reason: err.to_string(),
})?;
```

Return the token fields:

```rust
Ok(RegistryIndex {
    canonical_symbols,
    resolve_targets,
    tokens,
    token_index,
})
```

- [x] **Step 5: Emit token sites during scan**

In `scan_repository`, initialize:

```rust
let design_system_tokens = registry.tokens.clone();
let mut token_sites = Vec::new();
```

After `extract_usage_sites(...)`, add:

```rust
token_sites.extend(find_token_matches(
    &source,
    &relative_file,
    &registry.token_index,
    "token.basic",
));
```

The basic pack uses naive substring matching only; false positives inside longer identifiers are an accepted v1 limitation.

Sort `token_sites` by file, line, column, and token id before returning.

Return:

```rust
design_system_tokens,
token_sites,
```

- [x] **Step 6: Populate `ScanFacts` from basic scans**

Modify `engine/crates/wax-lang-basic/src/lib.rs` in `facts_from_scan`:

```rust
design_system_tokens: scan.design_system_tokens,
token_sites: scan.token_sites,
hardcoded_style_sites: vec![],
token_usage_summary: vec![],
```

Modify scaffold facts similarly:

```rust
design_system_tokens: Vec::new(),
token_sites: Vec::new(),
hardcoded_style_sites: Vec::new(),
token_usage_summary: Vec::new(),
```

- [x] **Step 7: Update golden fixture**

Run:

```bash
cd engine
cargo test -p wax-lang-basic golden_small -- --nocapture
```

Expected: test fails with a golden diff that includes token fields.

Update `engine/crates/wax-lang-basic/tests/fixtures/small/golden.json` with the new deterministic token fields from the failing output. Keep `hardcoded_style_sites` absent or empty according to the serializer behavior.

- [x] **Step 8: Run focused basic checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-basic
```

Expected: `cargo test -p wax-lang-basic` passes.

- [x] **Step 9: Commit Task 3**

```bash
git add engine/crates/wax-lang-basic/src/line_scan.rs engine/crates/wax-lang-basic/src/lib.rs engine/crates/wax-lang-basic/tests/fixtures/small engine/crates/wax-lang-basic/tests/golden_small.rs
git commit -m "feat: scan basic token references"
```

---

### Task 4: Aggregate Tokens In Core And CLI

**Scope note:** v1 stops at merged scan output and `wax scan` terminal summary. wax-scan skill HTML reporting and fixture updates are follow-on work tracked separately from this plan.

**Files:**
- Modify: `engine/crates/wax-core/src/adoption_merge.rs`
- Modify: `engine/crates/wax-core/tests/scan_output.rs`
- Modify: `engine/crates/wax-core/tests/subprocess_protocol.rs`
- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-cli/tests/scan_command.rs`

**Interfaces:**
- Consumes: token fields and counts from Task 1
- Produces: per-language token usage summaries, merged repo token counts, merged token usage summaries, CLI token summary lines

- [x] **Step 1: Write failing core merge test**

Add a test to `engine/crates/wax-core/tests/scan_output.rs` that runs a scan fixture with one language returning:

```json
"design_system_tokens": [
  {"id":"color.primary","key":"Theme.colors.primary","category":"color"}
],
"token_sites": [
  {
    "id":"token.compose:src/Screen.kt:1:1:color.primary",
    "location":{"file":"src/Screen.kt","line":1,"column":1},
    "token_id":"color.primary",
    "key":"Theme.colors.primary",
    "category":"color"
  }
],
"hardcoded_style_sites": [
  {
    "id":"hardcoded.compose:src/Screen.kt:2:12:spacing",
    "location":{"file":"src/Screen.kt","line":2,"column":12},
    "value":"8.dp",
    "category":"spacing"
  }
]
```

Assert the merged output contains:

```rust
assert_eq!(merged.repo_summary.counts.tokens.configured_token_count, 1);
assert_eq!(merged.repo_summary.counts.tokens.used_token_count, 1);
assert_eq!(merged.repo_summary.counts.tokens.token_reference_site_count, 1);
assert_eq!(merged.repo_summary.counts.tokens.hardcoded_style_candidate_count, 1);
assert_eq!(merged.repo_summary.metrics.token_reference_ratio, Some(0.5));
assert_eq!(merged.token_usage_summary.len(), 1);
assert_eq!(
    merged.languages[&LanguageId::try_from("compose").unwrap()]
        .token_usage_summary
        .len(),
    1
);
```

Expected before implementation: merge does not expose token summaries or ratio.

- [x] **Step 2: Sum token counts in `sum_count_summaries`**

In `engine/crates/wax-core/src/adoption_merge.rs`, add token count summing:

```rust
total.tokens.configured_token_count = total
    .tokens
    .configured_token_count
    .saturating_add(counts.tokens.configured_token_count);
total.tokens.used_token_count = total
    .tokens
    .used_token_count
    .saturating_add(counts.tokens.used_token_count);
total.tokens.token_reference_site_count = total
    .tokens
    .token_reference_site_count
    .saturating_add(counts.tokens.token_reference_site_count);
total.tokens.hardcoded_style_candidate_count = total
    .tokens
    .hardcoded_style_candidate_count
    .saturating_add(counts.tokens.hardcoded_style_candidate_count);
total.tokens.parent_scopes_with_token_references = total
    .tokens
    .parent_scopes_with_token_references
    .saturating_add(counts.tokens.parent_scopes_with_token_references);
total.tokens.parent_scopes_with_hardcoded_candidates = total
    .tokens
    .parent_scopes_with_hardcoded_candidates
    .saturating_add(counts.tokens.parent_scopes_with_hardcoded_candidates);
```

Add a helper for category counts:

```rust
fn add_token_category_counts(
    total: &mut wax_contract::TokenCategoryCounts,
    counts: &wax_contract::TokenCategoryCounts,
) {
    total.color = total.color.saturating_add(counts.color);
    total.spacing = total.spacing.saturating_add(counts.spacing);
    total.typography = total.typography.saturating_add(counts.typography);
    total.radius = total.radius.saturating_add(counts.radius);
    total.elevation = total.elevation.saturating_add(counts.elevation);
    total.unknown = total.unknown.saturating_add(counts.unknown);
}
```

Call it for `token_references_by_category` and `hardcoded_by_category`.

- [x] **Step 3: Compute token ratio in `metrics_from_counts`**

Add:

```rust
let token_denominator = counts
    .tokens
    .token_reference_site_count
    .saturating_add(counts.tokens.hardcoded_style_candidate_count);
let token_reference_ratio = if token_denominator == 0 {
    None
} else {
    Some(f64::from(counts.tokens.token_reference_site_count) / f64::from(token_denominator))
};
```

Return:

```rust
Metrics {
    invocation_adoption_ratio,
    registry_resolution_ratio,
    token_reference_ratio,
    parse_extract_ms,
    files_scanned,
}
```

- [x] **Step 4: Build per-language token summaries**

In `recompute_derived_scan_facts_with_parent_scope_limit`, after `build_symbol_usage_summaries`, add:

```rust
facts.token_usage_summary = build_token_usage_summaries(facts);
```

Implement:

```rust
fn build_token_usage_summaries(facts: &ScanFacts) -> Vec<wax_contract::TokenUsageSummary> {
    let language = facts.language.id.as_str().to_owned();
    let tokens = facts
        .design_system_tokens
        .iter()
        .map(|token| (token.id.clone(), token.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut grouped = BTreeMap::<String, (wax_contract::DesignSystemToken, u32, BTreeSet<String>, BTreeSet<String>)>::new();

    for site in &facts.token_sites {
        let Some(token) = tokens.get(&site.token_id) else {
            continue;
        };
        let entry = grouped
            .entry(site.token_id.clone())
            .or_insert_with(|| (token.clone(), 0, BTreeSet::new(), BTreeSet::new()));
        entry.1 = entry.1.saturating_add(1);
        entry.2.insert(site.location.file.clone());
        if let Some(parent) = &site.parent {
            entry.3.insert(parent.parent_id.clone());
        }
    }

    grouped
        .into_values()
        .map(|(token, reference_count, files, parents)| wax_contract::TokenUsageSummary {
            language: language.clone(),
            token_id: token.id,
            key: token.key,
            category: token.category,
            reference_count,
            file_count: u32::try_from(files.len()).unwrap_or(u32::MAX),
            parent_scope_count: u32::try_from(parents.len()).unwrap_or(u32::MAX),
        })
        .collect()
}
```

- [x] **Step 5: Build merged token summaries**

Add a `token_usage_summary` field to merged scan construction:

```rust
let token_usage_summary = merge_token_usage_summaries(&merged_languages);
```

Implement:

```rust
fn merge_token_usage_summaries(
    languages: &BTreeMap<LanguageId, ScanFacts>,
) -> Vec<wax_contract::TokenUsageSummary> {
    // Token ids are language-local. Keep one row per language summary and do not
    // collapse same-looking ids across packs (mirrors symbol summary merge).
    let mut merged = Vec::new();
    for facts in languages.values() {
        merged.extend(facts.token_usage_summary.clone());
    }
    merged.sort_by(|left, right| {
        left.language
            .cmp(&right.language)
            .then_with(|| left.token_id.cmp(&right.token_id))
            .then_with(|| left.key.cmp(&right.key))
    });
    merged
}
```

Add a two-language unit test that asserts exact language identity, counts, and deterministic ordering when both packs emit the same local `token_id`.

- [x] **Step 6: Print token metrics in CLI summary**

Modify `engine/crates/wax-cli/src/commands/scan.rs` in `write_scan_summary` after invocation metrics:

```rust
writeln!(writer, "token metrics:").map_err(write_error)?;
if let Some(ratio) = repo.metrics.token_reference_ratio {
    writeln!(writer, "  Token reference ratio: {:.1}%", ratio * 100.0)
        .map_err(write_error)?;
} else {
    writeln!(writer, "  Token reference ratio: n/a").map_err(write_error)?;
}
writeln!(
    writer,
    "  Token references: {}",
    repo.counts.tokens.token_reference_site_count
)
.map_err(write_error)?;
writeln!(
    writer,
    "  Hard-coded style candidates: {}",
    repo.counts.tokens.hardcoded_style_candidate_count
)
.map_err(write_error)?;
```

- [x] **Step 7: Update CLI tests**

In `engine/crates/wax-cli/src/commands/scan.rs` test helper `sample_repo_counts`, set token counts to:

```rust
tokens: wax_contract::TokenCounts {
    configured_token_count: 2,
    used_token_count: 1,
    token_reference_site_count: 3,
    hardcoded_style_candidate_count: 1,
    token_references_by_category: wax_contract::TokenCategoryCounts {
        color: 2,
        spacing: 1,
        ..Default::default()
    },
    hardcoded_by_category: wax_contract::TokenCategoryCounts {
        spacing: 1,
        ..Default::default()
    },
    parent_scopes_with_token_references: 1,
    parent_scopes_with_hardcoded_candidates: 1,
},
```

Set `Metrics.token_reference_ratio: Some(0.75)` in the same test and assert:

```rust
assert!(stdout.contains("token metrics:"));
assert!(stdout.contains("Token reference ratio: 75.0%"));
assert!(stdout.contains("Token references: 3"));
assert!(stdout.contains("Hard-coded style candidates: 1"));
```

- [x] **Step 8: Run core and CLI checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-core
cargo test -p wax-cli
```

Expected: both package test suites pass.

- [x] **Step 9: Commit Task 4**

```bash
git add engine/crates/wax-core/src/adoption_merge.rs engine/crates/wax-core/tests/scan_output.rs engine/crates/wax-core/tests/subprocess_protocol.rs engine/crates/wax-cli/src/commands/scan.rs engine/crates/wax-cli/tests/scan_command.rs
git commit -m "feat: report token scan metrics"
```

---

### Task 5: Add Compose Token Scanning

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/app/src/main/kotlin/Sample.kt`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/golden.json`
- Modify: `engine/crates/wax-lang-compose/tests/golden_small.rs`

**Interfaces:**
- Consumes: shared token registry parser from Task 2 and token contract types from Task 1
- Produces: Compose `design_system_tokens`, `token_sites`, and `hardcoded_style_sites`

- [x] **Step 1: Write failing Compose tests**

Add assertions in `engine/crates/wax-lang-compose/tests/golden_small.rs`:

```rust
assert!(
    facts
        .token_sites
        .iter()
        .any(|site| site.token_id == "color.primary" && site.parent.is_some()),
    "Compose token references should include parent attribution when inside a composable"
);
assert!(
    facts
        .hardcoded_style_sites
        .iter()
        .any(|site| site.category == wax_contract::TokenCategory::Spacing && site.value == "8.dp"),
    "Modifier.padding(8.dp) should be a spacing hard-coded candidate"
);
assert!(
    facts
        .hardcoded_style_sites
        .iter()
        .any(|site| site.category == wax_contract::TokenCategory::Color && site.value.contains("0x")),
    "Color(0x...) should be a color hard-coded candidate"
);
```

Expected before implementation: assertions fail because Compose does not emit token facts.

- [x] **Step 2: Add Compose fixture registry tokens**

In `engine/crates/wax-lang-compose/tests/fixtures/small/design-system/registry.json`, add:

```json
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
```

In the small fixture Kotlin source, add inside a non-preview composable:

```kotlin
val primary = Theme.colors.primary
Box(Modifier.padding(8.dp).background(Color(0xFF336699)))
```

- [x] **Step 3: Load tokens in Compose registry**

Modify imports in `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs` to include:

```rust
DesignSystemToken, HardcodedStyleSite, TokenCategory, TokenSite,
```

and from `wax_lang_api`:

```rust
RegistryTokenIndex, find_token_matches, parse_registry_tokens, token_index,
```

Extend `TreeSitterScanResult`:

```rust
pub design_system_tokens: Vec<DesignSystemToken>,
pub token_sites: Vec<TokenSite>,
pub hardcoded_style_sites: Vec<HardcodedStyleSite>,
```

Extend `RegistryIndex`:

```rust
tokens: Vec<DesignSystemToken>,
token_index: RegistryTokenIndex,
```

In `load_registry`, after parsing `value`, add:

```rust
let tokens = parse_registry_tokens(&value).map_err(|err| TreeSitterScanError::RegistryInvalid {
    path: path.to_path_buf(),
    reason: err.to_string(),
})?;
let token_index = token_index(&tokens).map_err(|err| TreeSitterScanError::RegistryInvalid {
    path: path.to_path_buf(),
    reason: err.to_string(),
})?;
```

Return the token fields:

```rust
Ok(RegistryIndex {
    canonical_symbols,
    resolve_targets,
    component_packages,
    tokens,
    token_index,
})
```

- [x] **Step 4: Add Compose hard-coded candidate extraction**

Add a helper:

```rust
fn extract_hardcoded_style_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    out: &mut Vec<HardcodedStyleSite>,
) {
    let package = package_name_from_source(root, source);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression"
            && let Some((call_symbol, pos)) = call_simple_callee(node, source)
            && let Some(category) = compose_style_category(&call_symbol)
            && let Some(value) = first_style_literal(node, source, category)
        {
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let parent =
                nearest_enclosing_composable(node, source).map(|(name, parent_pos)| {
                    parent_scope_for_composable(file, package.as_deref(), &name, parent_pos)
                });
            out.push(HardcodedStyleSite {
                id: format!("hardcoded.compose:{file}:{line}:{column}:{category:?}"),
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
                value,
                category,
                parent,
            });
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}
```

Implement category and literal helpers:

```rust
fn compose_style_category(call_symbol: &str) -> Option<TokenCategory> {
    match call_symbol {
        "Color" | "background" | "border" => Some(TokenCategory::Color),
        "padding" | "size" | "width" | "height" | "spacedBy" => Some(TokenCategory::Spacing),
        "fontSize" | "TextStyle" => Some(TokenCategory::Typography),
        "clip" | "cornerRadius" | "RoundedCornerShape" => Some(TokenCategory::Radius),
        "shadow" | "elevation" => Some(TokenCategory::Elevation),
        _ => None,
    }
}
```

`first_style_literal` must return source text for literals matching:

- hex color text containing `0x`
- dp literals ending in `.dp`
- sp literals ending in `.sp`
- integer or float literals inside a known style call

Use `node.utf8_text(source).ok()` to inspect the call expression text and select the first matching token by whitespace splitting and punctuation trimming.

- [x] **Step 5: Emit Compose token facts during scan**

In the per-file scan loop, after reading `source` and obtaining `root`, call:

```rust
extract_hardcoded_style_from_source(root, source.as_bytes(), &relative_file, &mut hardcoded_style_sites);
extract_token_sites_from_source(
    root,
    source.as_bytes(),
    &relative_file,
    &registry.token_index,
    &mut token_sites,
);
```

For token sites, implement a parser-backed helper so parent attribution is available:

```rust
fn extract_token_sites_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    token_index: &RegistryTokenIndex,
    out: &mut Vec<TokenSite>,
) {
    let package = package_name_from_source(root, source);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "identifier" | "navigation_expression" | "call_expression")
            && let Ok(text) = node.utf8_text(source)
            && let Some(token_match) = token_index.matches.get(text)
        {
            let pos = node.start_position();
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let parent =
                nearest_enclosing_composable(node, source).map(|(name, parent_pos)| {
                    parent_scope_for_composable(file, package.as_deref(), &name, parent_pos)
                });
            out.push(TokenSite {
                id: format!("token.compose:{file}:{line}:{column}:{}", token_match.token_id),
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
                token_id: token_match.token_id.clone(),
                key: text.to_owned(),
                category: token_match.category,
                parent,
            });
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}
```

Do not call `find_token_matches` for Compose; parser-backed extraction is required so parent assertions from Step 1 pass.

- [x] **Step 6: Populate Compose `ScanFacts`**

Modify the Compose language wrapper that turns `TreeSitterScanResult` into `ScanFacts` to include:

```rust
design_system_tokens: scan.design_system_tokens,
token_sites: scan.token_sites,
hardcoded_style_sites: scan.hardcoded_style_sites,
token_usage_summary: vec![],
```

Modify scaffold facts to use empty token vectors.

- [x] **Step 7: Update Compose golden fixture**

Run:

```bash
cd engine
cargo test -p wax-lang-compose golden_small -- --nocapture
```

Expected: test fails with a golden diff containing Compose token fields.

Update `engine/crates/wax-lang-compose/tests/fixtures/small/golden.json` with deterministic output.

- [x] **Step 8: Run focused Compose checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose
```

Expected: `cargo test -p wax-lang-compose` passes.

- [x] **Step 9: Commit Task 5**

```bash
git add engine/crates/wax-lang-compose/src/tree_sitter_scan.rs engine/crates/wax-lang-compose/tests/fixtures/small engine/crates/wax-lang-compose/tests/golden_small.rs
git commit -m "feat: scan compose token facts"
```

---

### Task 6: Add React Token Scanning

**Files:**
- Modify: `engine/crates/wax-lang-react/src/registry.rs`
- Modify: `engine/crates/wax-lang-react/src/extract.rs`
- Modify: `engine/crates/wax-lang-react/src/facts.rs`
- Modify: `engine/crates/wax-lang-react/src/lib.rs`
- Modify: `engine/crates/wax-lang-react/src/swc_parse.rs`
- Create: `engine/crates/wax-lang-react/src/component_scope.rs`
- Create: `engine/crates/wax-lang-react/src/token_extract.rs`
- Create: `engine/crates/wax-lang-react/src/style_extract.rs`
- Modify: `engine/crates/wax-lang-react/Cargo.toml` (add `swc_ecma_visit`)
- Modify: `engine/Cargo.toml` / `engine/Cargo.lock` (workspace `swc_ecma_visit`)
- Modify: `engine/crates/wax-lang-react/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-react/tests/fixtures/small/src/Sample.tsx`
- Modify: `engine/crates/wax-lang-react/tests/fixtures/small/golden.json`
- Modify: `engine/crates/wax-lang-react/tests/golden_small.rs`

**Interfaces:**
- Consumes: shared token registry parser from Task 2 and React parent attribution via `component_scope`
- Produces: React `design_system_tokens`, `token_sites`, and `hardcoded_style_sites`

- [x] **Step 1: Write failing React tests**

Add assertions in `engine/crates/wax-lang-react/tests/golden_small.rs`:

```rust
assert!(
    facts
        .token_sites
        .iter()
        .any(|site| site.token_id == "color.primary" && site.parent.is_some()),
    "React token references should include parent attribution inside components"
);
assert!(
    facts
        .hardcoded_style_sites
        .iter()
        .any(|site| site.category == wax_contract::TokenCategory::Color && site.value == "\"#336699\""),
    "inline style color hex should be a color hard-coded candidate"
);
assert!(
    facts
        .hardcoded_style_sites
        .iter()
        .any(|site| site.category == wax_contract::TokenCategory::Spacing && site.value == "8"),
    "inline padding number should be a spacing hard-coded candidate"
);
```

- [x] **Step 2: Add React fixture registry tokens and source examples**

Add to the React small registry:

```json
"tokens": [
  {
    "id": "color.primary",
    "key": "theme.colors.primary",
    "category": "color",
    "aliases": ["tokens.color.primary"]
  },
  {
    "id": "space.medium",
    "key": "theme.space.medium",
    "category": "spacing"
  }
]
```

Add inside an existing React component:

```tsx
const color = theme.colors.primary;
return <div style={{ color: "#336699", padding: 8, borderRadius: 4 }}>{color}</div>;
```

- [x] **Step 3: Load tokens in React registry**

Modify `ReactRegistryIndex` in `engine/crates/wax-lang-react/src/registry.rs`:

```rust
pub design_system_tokens: Vec<wax_contract::DesignSystemToken>,
pub token_index: wax_lang_api::RegistryTokenIndex,
```

In `load_react_registry`, after parsing `value`, call `parse_registry_tokens` and `token_index`, mapping errors into `RegistryError::invalid`.

- [x] **Step 4: Extend React extraction output**

Modify `ReactUsageExtraction`:

```rust
pub token_sites: Vec<wax_contract::TokenSite>,
pub hardcoded_style_sites: Vec<wax_contract::HardcodedStyleSite>,
```

In `collect_usage_sites`, for each `ParsedReactModule`:

1. Build a component-definition span index (including `forwardRef` / `memo` wrappers) and attribute parents with the smallest span that fully contains each fact.
2. Collect token sites with an SWC visitor that looks up the **exact source slice** of peeled member/ident expressions in the registry token index (not reconstructed dotted paths, and not the basic-pack `find_token_matches` helper).
3. Collect hard-coded style sites with an SWC visitor over JSX `style` attributes whose value peels to an object literal.

Parent attribution and token/style extraction live in focused modules (`component_scope`, `token_extract`, `style_extract`) rather than additional hand-written walkers in `extract.rs`.

- [x] **Step 5: Detect React hard-coded style candidates**

In React style extraction, scan JSX attributes named `style` whose expression peels to an object literal (including TypeScript assertion wrappers). For object literal properties, emit:

```rust
color | backgroundColor | borderColor -> TokenCategory::Color
padding | margin | gap | width | height -> TokenCategory::Spacing
fontSize | fontWeight | lineHeight -> TokenCategory::Typography
borderRadius -> TokenCategory::Radius
boxShadow | shadow -> TokenCategory::Elevation
```

Emit `HardcodedStyleSite` when the property value is:

- string literal
- numeric literal
- template literal with no expressions

Use the property's span for location and source slice for `value`.

- [x] **Step 6: Populate React `ScanFacts`**

Modify the React language wrapper to include:

```rust
design_system_tokens: registry.design_system_tokens,
token_sites: extraction.token_sites,
hardcoded_style_sites: extraction.hardcoded_style_sites,
token_usage_summary: vec![],
```

Modify scaffold facts to use empty token vectors.

- [x] **Step 7: Update React golden fixture**

Run:

```bash
cd engine
cargo test -p wax-lang-react golden_small -- --nocapture
```

Expected: golden diff includes React token fields.

Update `engine/crates/wax-lang-react/tests/fixtures/small/golden.json`.

- [x] **Step 8: Run focused React checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-react
```

Expected: `cargo test -p wax-lang-react` passes.

- [x] **Step 9: Commit Task 6**

```bash
git add engine/Cargo.toml engine/Cargo.lock \
  engine/crates/wax-lang-react/Cargo.toml \
  engine/crates/wax-lang-react/src \
  engine/crates/wax-lang-react/tests/fixtures/small \
  engine/crates/wax-lang-react/tests/golden_small.rs \
  docs/plans/2026-07-03-token-scanning-plan.md \
  docs/specs/2026-07-03-token-scanning-design.md
git commit -m "feat: scan react token facts"
```

---

### Task 7: Add SwiftUI Token Scanning

**Files:**
- Modify: `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-swift/tests/fixtures/small/design-system/registry.json`
- Modify: `engine/crates/wax-lang-swift/tests/fixtures/small/app/Sources/App/Sample.swift`
- Modify: `engine/crates/wax-lang-swift/tests/fixtures/small/golden.json`
- Modify: `engine/crates/wax-lang-swift/tests/golden_small.rs`

**Interfaces:**
- Consumes: shared token registry parser from Task 2 and SwiftUI parent attribution helpers in `tree_sitter_scan.rs`
- Produces: Swift `design_system_tokens`, `token_sites`, and `hardcoded_style_sites`

- [x] **Step 1: Write failing Swift tests**

Add assertions in `engine/crates/wax-lang-swift/tests/golden_small.rs`:

```rust
assert!(
    facts
        .token_sites
        .iter()
        .any(|site| site.token_id == "color.primary" && site.parent.is_some()),
    "SwiftUI token references should include parent attribution inside views"
);
assert!(
    facts
        .hardcoded_style_sites
        .iter()
        .any(|site| site.category == wax_contract::TokenCategory::Color && site.value.contains("Color")),
    "SwiftUI Color(...) should be a color hard-coded candidate"
);
assert!(
    facts
        .hardcoded_style_sites
        .iter()
        .any(|site| site.category == wax_contract::TokenCategory::Radius && site.value == "8"),
    "cornerRadius(8) should be a radius hard-coded candidate"
);
```

- [x] **Step 2: Add Swift fixture registry tokens and source examples**

Add to the Swift small registry:

```json
"tokens": [
  {
    "id": "color.primary",
    "key": "Theme.colors.primary",
    "category": "color",
    "aliases": ["Color.dsPrimary"]
  },
  {
    "id": "space.medium",
    "key": "Theme.spacing.medium",
    "category": "spacing"
  }
]
```

Add inside an existing SwiftUI view body:

```swift
let primary = Theme.colors.primary
Text("Token")
  .foregroundStyle(Color(red: 0.2, green: 0.3, blue: 0.4))
  .cornerRadius(8)
```

- [x] **Step 3: Load tokens in Swift registry**

Modify `RegistryIndex` in `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs`:

```rust
tokens: Vec<DesignSystemToken>,
token_index: RegistryTokenIndex,
```

Parse tokens in `load_registry` with `parse_registry_tokens` and `token_index`, mapping errors into `TreeSitterScanError::RegistryInvalid`.

- [x] **Step 4: Add Swift hard-coded candidate extraction**

Add category mapping:

```rust
fn swift_style_category(call_symbol: &str) -> Option<TokenCategory> {
    match call_symbol {
        "Color" | "foregroundStyle" | "foregroundColor" | "background" => Some(TokenCategory::Color),
        "padding" | "frame" | "spacing" => Some(TokenCategory::Spacing),
        "font" | "fontWeight" => Some(TokenCategory::Typography),
        "cornerRadius" | "clipShape" => Some(TokenCategory::Radius),
        "shadow" => Some(TokenCategory::Elevation),
        _ => None,
    }
}
```

When `extract_usage_from_source` visits call expressions, if `swift_style_category(&call_site.symbol)` returns a category and the call text contains a literal string, numeric literal, or `Color(...)`, emit `HardcodedStyleSite` with existing parent attribution.

- [x] **Step 5: Emit Swift token facts during scan**

During each source file scan, after obtaining `root`, call:

```rust
extract_token_sites_from_source(
    root,
    source.as_bytes(),
    &relative_file,
    module_identity,
    semantic_module.as_deref(),
    &registry.token_index,
    &mut token_sites,
);
```

For token sites, implement a parser-backed helper so parent attribution is available:

```rust
fn extract_token_sites_from_source(
    root: tree_sitter::Node<'_>,
    source: &[u8],
    file: &str,
    module_identity: &str,
    semantic_module: Option<&str>,
    token_index: &RegistryTokenIndex,
    out: &mut Vec<TokenSite>,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "simple_identifier" | "navigation_expression" | "call_expression")
            && let Ok(text) = node.utf8_text(source)
            && let Some(token_match) = token_index.matches.get(text)
        {
            let pos = node.start_position();
            let line = pos.row as u32 + 1;
            let column = pos.column as u32 + 1;
            let parent = nearest_enclosing_view(node, source).map(|(name, parent_pos)| {
                parent_scope_for_view(file, module_identity, semantic_module, &name, parent_pos)
            });
            out.push(TokenSite {
                id: format!("token.swift:{file}:{line}:{column}:{}", token_match.token_id),
                location: SourceLocation {
                    file: file.to_owned(),
                    line,
                    column: Some(column),
                },
                token_id: token_match.token_id.clone(),
                key: text.to_owned(),
                category: token_match.category,
                parent,
            });
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
    }
}
```

Do not call `find_token_matches` for Swift; parser-backed extraction is required so parent assertions from Step 1 pass.

- [x] **Step 6: Populate Swift `ScanFacts`**

Modify the Swift language wrapper to include:

```rust
design_system_tokens: scan.design_system_tokens,
token_sites: scan.token_sites,
hardcoded_style_sites: scan.hardcoded_style_sites,
token_usage_summary: vec![],
```

Modify scaffold facts to use empty token vectors.

- [x] **Step 7: Update Swift golden fixture**

Run:

```bash
cd engine
cargo test -p wax-lang-swift golden_small -- --nocapture
```

Expected: golden diff includes Swift token fields.

Update `engine/crates/wax-lang-swift/tests/fixtures/small/golden.json`.

- [x] **Step 8: Run focused Swift checks**

Run:

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-swift
```

Expected: `cargo test -p wax-lang-swift` passes.

- [x] **Step 9: Commit Task 7**

```bash
git add engine/crates/wax-lang-swift/src/tree_sitter_scan.rs engine/crates/wax-lang-swift/tests/fixtures/small engine/crates/wax-lang-swift/tests/golden_small.rs
git commit -m "feat: scan swift token facts"
```

---

### Task 8: Update Documentation And Run Workspace Verification

**Files:**
- Modify: `docs/specs/2026-05-13-component-tracker-design.md`
- Modify: `docs/specs/2026-06-20-adoption-metrics-v2-design.md`
- Modify: `docs/specs/2026-07-03-token-scanning-design.md`
- Modify: `docs/plans/2026-07-03-token-scanning-plan.md`

**Interfaces:**
- Consumes: completed implementation from Tasks 1-7
- Produces: plan checkboxes, design links, and broad verification evidence

- [x] **Step 1: Update design cross-links**

In `docs/specs/2026-05-13-component-tracker-design.md`, add a sentence after the token definition paragraph:

```markdown
The concrete alpha design for additive token facts is tracked in [Token Scanning Design](./2026-07-03-token-scanning-design.md).
```

In `docs/specs/2026-06-20-adoption-metrics-v2-design.md`, update the future fact family `Tokens` row to:

```markdown
| Tokens | Token usage sites, hard-coded value candidates, token category. See [Token Scanning Design](./2026-07-03-token-scanning-design.md). |
```

In `docs/specs/2026-07-03-token-scanning-design.md`, keep this implementation plan link present:

```markdown
**Implementation plan:** [`docs/plans/2026-07-03-token-scanning-plan.md`](../plans/2026-07-03-token-scanning-plan.md)
```

- [x] **Step 2: Tick completed plan checkboxes**

In `docs/plans/2026-07-03-token-scanning-plan.md`, change each completed implementation step from `- [ ]` to `- [x]`.

- [x] **Step 3: Run broad workspace verification**

Run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all three commands pass.

- [x] **Step 4: Inspect git status**

Run:

```bash
git status --short
```

Expected: only files intentionally modified by the token scanning work are listed. Existing unrelated untracked files such as `docs/icon-explorations/` must remain unstaged unless the user explicitly asks to include them.

- [x] **Step 5: Commit Task 8**

```bash
git add docs/specs/2026-05-13-component-tracker-design.md docs/specs/2026-06-20-adoption-metrics-v2-design.md docs/specs/2026-07-03-token-scanning-design.md docs/plans/2026-07-03-token-scanning-plan.md
git commit -m "docs: finalize token scanning plan"
```

- [ ] **Step 6: Archive plan and add ADR when implementation completes**

After all Tasks 1-8 implementation PRs merge:

1. Move `docs/plans/2026-07-03-token-scanning-plan.md` to `docs/plans/archive/`.
2. Add an ADR under `docs/adr/` describing the shipped token fact family.
3. Update `docs/plans/README.md`:
   - set Token Scanning doc status to `merged` and implementation status to `complete`
   - add the ADR link in the roadmap row
   - update the active plan pointer to the next roadmap item

---

## Final Verification

After Task 8, run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all commands pass.

Then run:

```bash
git log --oneline -8
git status --short
```

Expected: recent commits show the task commits from this plan. `git status --short` contains no unstaged or staged changes from the implementation work. Unrelated pre-existing untracked files may still appear and must not be included without explicit user approval.
