# Compose Parse Recovery and UI Scoping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Compose scanning recover from valid and malformed Kotlin syntax without abandoning later UI facts, while restricting component and unresolved-call metrics to UI-bearing scopes.

**Architecture:** Keep `tree-sitter-kotlin-ng` 1.1.0 and add a byte-preserving Kotlin recovery layer between source loading and extraction. The layer records known syntax regions, parses a normalized primary buffer, reparses bounded later-source islands for remaining broad errors, and exposes clean source ranges to scope-aware extractors. Existing fact ids provide the merge key. Diagnostics and scan status are derived from unresolved syntax problems, and the CLI reports the full diagnostic count before showing five rows.

**Tech Stack:** Rust 1.95 workspace under `engine/`, `tree-sitter` 0.25, `tree-sitter-kotlin-ng` 1.1.0, Kotlin compilers 2.1.0–2.4.0 for fixture validation, shell fixture tooling, GitHub Actions.

## Global Constraints

- Do not replace or fork the Kotlin grammar in this plan.
- Do not change `ScanFacts`, JSON schemas, fact ids, registry resolution rules, or parser metadata.
- Every normalization must preserve byte length and newline byte positions. All locations continue to index the original source.
- Parsing a source file may return a partial result but must not panic, fail the language pack, or stop later files from scanning.
- An accepted recovery pass must advance its clean range beyond the preceding pass. Cap attempts at `min(lexical_boundary_count, 64)` per file.
- Emit local components, registry usages, local usages, and unresolved PascalCase calls only inside an `@Composable` declaration or a statically annotated composable lambda.
- Nested ordinary lambdas inherit the surrounding UI scope. A recorded suspend-lambda body or explicit-backing-field initializer forces component extraction to `NonUi`.
- Continue token-reference and hard-coded-style traversal independently. Exclude error-containing nodes and regions intentionally removed from analysis, but do not require an enclosing composable for token references.
- Known, fully recovered valid syntax does not emit `parse_failed` and does not make the scan `Partial`.
- Unknown or malformed skipped syntax emits `parse_failed` and keeps the scan `Partial`, even when later facts are recovered.
- Full diagnostics remain in JSON. Terminal output shows at most five failure diagnostics and prints the actual total and omitted count.
- Normal Rust tests must not require a globally installed Kotlin compiler or network access.
- The current roadmap gate remains authoritative: implementation starts only after this plan is promoted or the maintainer explicitly grants an exception.
- One task is one focused PR unless the maintainer explicitly batches adjacent tasks. Run `cargo fmt --all` before every Rust commit.

---

## Execution Model

- Start each task from the merged predecessor because Tasks 2–5 depend on recovery types introduced earlier.
- Use branches such as `dai/compose-recovery-model`, `dai/compose-known-syntax`, `dai/compose-ui-scopes`, `dai/compose-recovery-islands`, and `dai/compose-recovery-reporting`.
- Tick the task heading and every completed step in the same implementation PR.
- Preserve unrelated worktree changes and stage only files named by the task.
- The task's **Files** list is authoritative. If the inventory step finds a mechanically affected test helper, add that exact file to the task list before editing.

## Reference Design

- [Compose parse recovery and UI scope design](./2026-07-22-compose-parse-recovery-design.md)
- [Wax implementation roadmap](./README.md)

## Final File Structure

- Create `engine/crates/wax-lang-compose/src/kotlin_recovery.rs` — lexical regions, byte-preserving normalizers, syntax-problem selection, clean passes, and bounded island recovery.
- Modify `engine/crates/wax-lang-compose/src/kotlin_ast.rs` — parsed-file model, parser entry points, AST helpers, and diagnostics backed by recovery metadata.
- Modify `engine/crates/wax-lang-compose/src/discover.rs` — migrate registry discovery to recovery metadata, diagnostics, and recovered parse passes.
- Modify `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs` — clean-pass iteration, UI-scope traversal, extraction filtering, deterministic merge/deduplication, and status.
- Modify `engine/crates/wax-lang-compose/src/lib.rs` — register the private recovery module.
- Create `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/` — reduced valid and malformed Kotlin cases plus the compiler matrix.
- Create `engine/crates/wax-lang-compose/tests/parse_recovery.rs` — public scanner integration coverage over the committed fixture repository.
- Modify `engine/crates/wax-cli/src/commands/scan.rs` and `engine/crates/wax-cli/tests/scan_command.rs` — truthful diagnostic totals and truncation.
- Create `scripts/verify-compose-kotlin-fixtures.sh` — opt-in compiler validation for one explicit compiler version/path.
- Create `scripts/test-verify-compose-kotlin-fixtures.sh` — offline shell behavior tests with a fake compiler.
- Create `scripts/replay-compose-corpus.sh` and `scripts/test-replay-compose-corpus.sh` — maintainer-supplied corpus comparison and offline harness tests.
- Modify `.github/workflows/build_engine.yml` — Rust/shell regression coverage and a pinned Kotlin compiler matrix.
- Modify `docs/plans/README.md`, this plan, and the design document during closeout.

---

### Task 1: Introduce Recovery Metadata and Precise Syntax Diagnostics

- [ ] **Task 1 complete**

**Files:**
- Create: `engine/crates/wax-lang-compose/src/kotlin_recovery.rs`
- Modify: `engine/crates/wax-lang-compose/src/lib.rs`
- Modify: `engine/crates/wax-lang-compose/src/kotlin_ast.rs`
- Modify: `engine/crates/wax-lang-compose/src/discover.rs`
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Modify: `docs/plans/2026-07-22-compose-parse-recovery-plan.md`

**Interfaces:**
- Consumes: original Kotlin source, `tree_sitter::Tree`, existing `parse_kotlin_file_permissive`, and existing `parse_failed` diagnostics.
- Produces: `ByteRange`, `SyntaxFamily`, `ComponentScopePolicy`, `SyntaxRegion`, `SyntaxProblem`, `ParsePass`, and an expanded `ParsedKotlinFile` used by Tasks 2–4.

- [ ] **Step 1: Lock the current failure shape with red tests**

In `kotlin_ast.rs`, add `smallest_problem_prefers_nested_missing_or_error` using a hand-parsed malformed annotated function type. Assert that the selected range starts at the inner missing/error node rather than the containing `function_declaration`. Add `known_recovery_metadata_defaults_to_one_primary_pass` asserting a valid file has one full-file pass and no unresolved problems.

In `tree_sitter_scan.rs`, rename `partial_parse_still_extracts_symbols_during_scan` to `partial_parse_reports_the_smallest_problem_and_keeps_prior_facts`. Add assertions that the diagnostic line is the malformed `fun Broken(` line and its message contains `file scanned with gaps`. In `discover.rs`, add `partial_discovery_uses_recovery_diagnostics_and_keeps_prior_symbols` to lock the same diagnostic migration for registry discovery.

Run:

```bash
cd engine
cargo test -p wax-lang-compose kotlin_ast::tests::smallest_problem_prefers_nested_missing_or_error -- --exact
cargo test -p wax-lang-compose kotlin_ast::tests::known_recovery_metadata_defaults_to_one_primary_pass -- --exact
```

Expected: FAIL because the recovery metadata and smallest-problem selector do not exist.

- [ ] **Step 2: Add the private recovery module and exact data model**

Add `mod kotlin_recovery;` beside `mod kotlin_ast;` in `lib.rs`. Define these crate-private types in `kotlin_recovery.rs`:

```rust
pub(crate) const MAX_RECOVERY_ATTEMPTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ByteRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl ByteRange {
    pub(crate) fn new(start: usize, end: usize) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    pub(crate) fn contains(self, byte: usize) -> bool {
        self.start <= byte && byte < self.end
    }

    pub(crate) fn contains_node(self, node: tree_sitter::Node<'_>) -> bool {
        self.start <= node.start_byte() && node.end_byte() <= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SyntaxFamily {
    SuspendLambda,
    WhenGuard,
    AnnotatedFunctionType,
    ExplicitBackingField,
    ContextParameter,
    ContextReceiver,
    AnnotatedTypeArgument,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentScopePolicy {
    Inherit,
    ComposableLambda,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxRegion {
    pub(crate) source: ByteRange,
    pub(crate) body: Option<ByteRange>,
    pub(crate) family: SyntaxFamily,
    pub(crate) component_scope: ComponentScopePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxProblem {
    pub(crate) range: ByteRange,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) family: SyntaxFamily,
    pub(crate) recovered_later_source: bool,
}

#[derive(Debug)]
pub(crate) struct ParsePass {
    pub(crate) tree: tree_sitter::Tree,
    pub(crate) clean: Vec<ByteRange>,
    pub(crate) priority: u16,
}
```

Add `merge_clean_ranges(Vec<ByteRange>) -> Vec<ByteRange>` that sorts by `(start, end)` and combines overlapping or touching entries. Sort regions by `(source.start, source.end, family)`. Keep these types private to the Compose crate; they are not contract types.

- [ ] **Step 3: Expand `ParsedKotlinFile` without exposing normalized text**

Replace the current `source/tree` struct with:

```rust
#[derive(Debug)]
pub(crate) struct ParsedKotlinFile {
    pub(crate) source: String,
    pub(crate) primary: ParsePass,
    pub(crate) recovered: Vec<ParsePass>,
    pub(crate) syntax_regions: Vec<SyntaxRegion>,
    pub(crate) unresolved_problems: Vec<SyntaxProblem>,
}

impl ParsedKotlinFile {
    pub(crate) fn passes(&self) -> impl Iterator<Item = &ParsePass> {
        std::iter::once(&self.primary).chain(&self.recovered)
    }

    pub(crate) fn primary_tree(&self) -> &tree_sitter::Tree {
        &self.primary.tree
    }

    pub(crate) fn is_partial(&self) -> bool {
        !self.unresolved_problems.is_empty()
    }
}
```

For this task, `parse_kotlin_file_permissive` still applies the existing annotated-parameter normalization, creates `primary` with `clean = [0..source.len()]`, leaves `recovered` empty, records no known regions, and collects unresolved problems from the returned tree. Keeping the primary pass in its own field makes an empty parsed-file state unrepresentable. Update `parse_kotlin_file_strict` to use `parsed.is_partial()`.

- [ ] **Step 4: Collect every error before selecting the narrowest useful node**

Replace `first_error_node` with a depth-first `collect_syntax_problem_nodes`. Include both `node.is_error()` and `node.is_missing()`. Group candidates whose byte ranges overlap or whose zero-width position lies inside the same outer error range, then keep one candidate per group using this sort key:

```rust
(
    node.end_byte().saturating_sub(node.start_byte()),
    !node.is_missing(),
    node.start_byte(),
)
```

This collapses nested recovery nodes into one diagnostic without merging disjoint failures. Discard a zero-width outer candidate only when a same-position missing descendant exists. Convert each selected node to one-based line/column and classify it as `SyntaxFamily::Unknown` in Task 1. Change `partial_tree_parse_diagnostic` to accept `&SyntaxProblem` instead of a tree root and format:

```text
tree-sitter could not fully parse <file> near <line>:<column>; file scanned with gaps
```

Keep diagnostic severity `Error` and code `parse_failed`.

- [ ] **Step 5: Wire scan status to unresolved problems**

In `scan_repository`, replace direct `tree_has_syntax_errors(&parsed.tree)` checks with `parsed.is_partial()`. Push one `parse_failed` diagnostic per unresolved problem, not one per outer tree. Continue to count a file once for status purposes. For Task 1 extraction, use `parsed.primary_tree().root_node()` so output is otherwise unchanged.

Migrate `discover_registry_symbols` in the same task: replace `parsed.tree` with `parsed.primary_tree()`, emit diagnostics from `parsed.unresolved_problems`, and keep primary-only symbol collection until Task 4 introduces clean recovered passes. This migration is required because `discover.rs` calls both changed interfaces.

Delete `tree_has_syntax_errors` only after `rg` shows no remaining callers; otherwise leave it for strict parser tests.

- [ ] **Step 6: Make malformed input panic-proof**

Add unit cases for empty source, unclosed block comment, unclosed triple-quoted string, unbalanced braces, and a parser returning a partial tree. Each test must assert a normal `ParsedKotlinFile` or `ParseKotlinFileError::ParseFailed`, never a panic. Add a repository test with one malformed file and one valid file; assert two files scanned and the valid file's UI fact is present.

- [ ] **Step 7: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose
cargo clippy -p wax-lang-compose --all-targets -- -D warnings
cd ..
git add engine/crates/wax-lang-compose docs/plans/2026-07-22-compose-parse-recovery-plan.md
git commit -m "refactor: model compose parse recovery"
```

Expected: all commands exit `0`; successfully parsed fixture facts remain unchanged.

---

### Task 2: Normalize the Seven Known Kotlin Syntax Families

- [ ] **Task 2 complete**

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/kotlin_recovery.rs`
- Modify: `engine/crates/wax-lang-compose/src/kotlin_ast.rs`
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/design-system/registry.json`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/SuspendLambda.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/WhenGuard.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/AnnotatedFunctionType.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/ExplicitBackingField.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/ContextParameter.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/ContextReceiver.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/WhenTrailingComma.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/AnnotatedTypeArgument.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/compiler-matrix.tsv`
- Create: `engine/crates/wax-lang-compose/tests/parse_recovery.rs`
- Modify: `docs/plans/2026-07-22-compose-parse-recovery-plan.md`

**Interfaces:**
- Consumes: Task 1 recovery metadata and the existing lexical helpers for comments, strings, identifiers, annotations, and matching delimiters.
- Produces: `normalize_kotlin_for_parse(&str) -> NormalizedKotlinSource`, known `SyntaxRegion` metadata, and a clean primary tree for every valid fixture.

- [ ] **Step 1: Commit reduced fixture sources and the compiler matrix**

Each Kotlin file must contain a `Before...` composable, the target syntax, and an `After...` composable. Use `PrimaryButton`, `Spacing.small`, and `Modifier.padding(7.dp)` as the common known facts. Keep tolerance-only infrastructure calls (`FetchRepository`, `MutableStateFlow`) PascalCase so scope tests can prove they are not UI facts later.

The exact target snippets are:

```kotlin
val loader = suspend { FetchRepository() }

when (item) {
    is VisibleItem if featureEnabled -> PrimaryButton(onClick = {})
    else -> Unit
}

val content: @Composable (() -> Unit) = { PrimaryButton(onClick = {}) }
val receiverContent: @Composable (Scope.(Item) -> Unit) = { PrimaryButton(onClick = {}) }

val state: StateFlow<List<Item>>
    field = MutableStateFlow(emptyList())

context(itemScope: ItemScope)
@Composable
fun ContextScreen() { PrimaryButton(onClick = {}) }

context(ItemScope)
@Composable
fun LegacyContextScreen() { PrimaryButton(onClick = {}) }

when (status) {
    Status.Starting,
    Status.Running,
    -> PrimaryButton(onClick = {})
    else -> Unit
}

val items: List<
    @Serializable(with = ItemSerializer::class)
    Item,
> = emptyList()
```

`compiler-matrix.tsv` has no header and exactly these tab-separated rows:

```text
2.1.0	-Xwhen-guards	app/src/main/kotlin/WhenGuard.kt
2.2.0	-Xcontext-parameters	app/src/main/kotlin/ContextParameter.kt
2.2.0	-Xcontext-receivers	app/src/main/kotlin/ContextReceiver.kt
2.3.0	-Xexplicit-backing-fields	app/src/main/kotlin/ExplicitBackingField.kt
2.4.0	-	app/src/main/kotlin/SuspendLambda.kt
2.4.0	-	app/src/main/kotlin/WhenGuard.kt
2.4.0	-	app/src/main/kotlin/AnnotatedFunctionType.kt
2.4.0	-	app/src/main/kotlin/ExplicitBackingField.kt
2.4.0	-	app/src/main/kotlin/ContextParameter.kt
2.4.0	-	app/src/main/kotlin/WhenTrailingComma.kt
2.4.0	-	app/src/main/kotlin/AnnotatedTypeArgument.kt
```

Provide minimal local stubs in each file so it compiles independently without Compose or coroutine dependencies. `@Target(AnnotationTarget.TYPE, AnnotationTarget.FUNCTION)` defines `Composable`; simple classes/interfaces define the other names.

- [ ] **Step 2: Add failing byte-preservation and parser tests**

In `kotlin_recovery.rs` unit tests, load every fixture with `include_str!`, call `normalize_kotlin_for_parse`, and assert:

```rust
assert_eq!(normalized.bytes.len(), source.len());
assert_eq!(newline_offsets(&normalized.bytes), newline_offsets(source.as_bytes()));
assert!(!parse(&normalized.bytes).root_node().has_error());
assert!(normalized.regions.windows(2).all(|pair| pair[0].source.start <= pair[1].source.start));
```

Also assert the original source text is retained by `ParsedKotlinFile`, and that an `After...` function node begins at the same byte, line, and column in the normalized tree. The trailing-comma fixture must report no normalized region, proving this grammar already handles it.

Run:

```bash
cd engine
cargo test -p wax-lang-compose --test parse_recovery known_valid_syntax_is_byte_preserving -- --exact
```

Expected: FAIL because the normalizer and fixtures are new.

- [ ] **Step 3: Generalize the lexical pass**

Move the existing comment/string/delimiter helpers from `kotlin_ast.rs` into `kotlin_recovery.rs`. Add one `lex_kotlin` pass that emits token starts and balanced `()`, `{}`, `[]`, and `<>` ranges while skipping nested block comments, line comments, quoted chars/strings, and triple-quoted strings. Do not use regex replacements.

Define:

```rust
pub(crate) struct NormalizedKotlinSource {
    pub(crate) bytes: Vec<u8>,
    pub(crate) regions: Vec<SyntaxRegion>,
}

pub(crate) fn normalize_kotlin_for_parse(source: &str) -> NormalizedKotlinSource;
```

Initialize `bytes` from `source.as_bytes().to_vec()`. Every helper mutates only non-newline bytes through:

```rust
fn mask_preserving_lines(bytes: &mut [u8], range: ByteRange) {
    for byte in &mut bytes[range.start..range.end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}
```

Require balanced candidate delimiters before applying a transform; otherwise record nothing and leave generic recovery for Task 4.

- [ ] **Step 4: Implement the known transforms in source order**

Apply non-overlapping transforms from highest byte offset to lowest so recorded original ranges remain stable:

1. **Suspend lambda:** recognize the `suspend` keyword followed only by whitespace/comments and `{`; mask `suspend`, record the balanced lambda block as `SuspendLambda`, `body = Some(block)`, `component_scope = Exclude`.
2. **When guard:** only inside a balanced `when` body, recognize a branch-level `if` between the condition and `->`; mask from `if` through the byte before `->`, record the complete entry as `WhenGuard`, and set `body` to the expression/block after `->`, `component_scope = Inherit`.
3. **Annotated function type:** find `@Composable` followed by a balanced function-type range containing a top-level `->`; mask only a redundant outer parenthesis pair when present. Cover parameter, property, constructor-property, return, nullable, and receiver forms. If the declaration initializer is a lambda, record that lambda body with `component_scope = ComposableLambda`.
4. **Explicit backing field:** inside a class/object body, recognize a line beginning with `field` after a property declaration and require a balanced initializer plus a safe lexical statement boundary. Mask the `field` keyword and any optional `: FieldType` suffix only through the byte before `=`, leaving `= <initializer>` parseable as the property's initializer. Record the complete field declaration as the source region and the preserved initializer expression as its body with `component_scope = Exclude`.
5. **Context parameter:** inside `context(...)`, recognize `identifier : Type`; mask only `identifier` and `:`, leaving the type list parseable as legacy context syntax. Record `ContextParameter`, `component_scope = Inherit`. A type-only context list is recorded as `ContextReceiver` without modification.
6. **Annotated type argument:** inside balanced `<...>`, recognize a type-use annotation before a type; mask the annotation name and optional balanced argument list, leaving the type and comma intact. Record `AnnotatedTypeArgument`, `component_scope = Exclude` for the annotation range only.

If two candidates overlap, keep the narrower recognized construct and leave the outer range untouched. Never modify comments, strings, character literals, annotation string arguments, or malformed unbalanced candidates.

- [ ] **Step 5: Use the unified normalizer in the parser**

Replace `normalize_annotated_parenthesized_function_types_for_parse` with `normalize_kotlin_for_parse`. Parse `normalized.bytes`, copy `normalized.regions` into `ParsedKotlinFile`, and collect syntax problems only from the normalized tree. Delete the parameter-only helper and invert its existing negative property/return test: all supported annotated function-type positions must now parse without errors.

Known regions are metadata, not diagnostics. If the normalized tree is clean, leave `unresolved_problems` empty and report `Complete`.

- [ ] **Step 6: Add scanner assertions for facts before and after every construct**

In `parse_recovery.rs`, scan the full fixture repository and assert:

- status is `Complete` and no diagnostic has code `parse_failed`;
- every `Before...` and `After...` local component is present;
- `PrimaryButton`, `Spacing.small`, and `7.dp` facts after every syntax construct retain their one-based fixture locations;
- `Spacing.small` and `Modifier.padding(7.dp)` inside an explicit backing-field initializer remain token/style facts while `MutableStateFlow(...)` remains excluded from component facts;
- guarded branch and context composable-body calls are present;
- trailing-comma behavior is unchanged.

Do not assert final suspend/backing-field/slot scope decisions until Task 3; Task 2 only proves parse reachability.

- [ ] **Step 7: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose --test parse_recovery
cargo test -p wax-lang-compose
cargo clippy -p wax-lang-compose --all-targets -- -D warnings
cd ..
git add engine/crates/wax-lang-compose docs/plans/2026-07-22-compose-parse-recovery-plan.md
git commit -m "fix: recover known kotlin syntax in compose scans"
```

Expected: all commands exit `0`; every valid known fixture is `Complete`.

---

### Task 3: Restrict Component Facts to UI-Bearing Scopes

- [ ] **Task 3 complete**

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/kotlin_ast.rs`
- Modify: `engine/crates/wax-lang-compose/src/kotlin_recovery.rs`
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/SuspendLambda.kt`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/AnnotatedFunctionType.kt`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/app/src/main/kotlin/ExplicitBackingField.kt`
- Modify: `engine/crates/wax-lang-compose/tests/parse_recovery.rs`
- Modify: `engine/crates/wax-lang-compose/tests/golden_small.rs`
- Modify: `engine/crates/wax-lang-compose/tests/fixtures/small/golden.json`
- Modify: `docs/plans/2026-07-22-compose-parse-recovery-plan.md`

**Interfaces:**
- Consumes: syntax-region `ComponentScopePolicy`, composable annotations, preview exclusion, registry/import resolution, and local-component index.
- Produces: explicit `UiScope` traversal and component metrics containing only UI-bearing calls.

- [ ] **Step 1: Write scope-policy tests before changing extraction**

Add these focused cases to `tree_sitter_scan.rs`:

```text
ordinary function { PrimaryButton(); UnknownCard() }           -> no usages
top-level property initializer { PrimaryButton() }             -> no usages
@Composable fun Screen() { PrimaryButton(); UnknownCard() }    -> both usages
@Composable fun Screen() { list.forEach { PrimaryButton() } }  -> PrimaryButton
@Composable fun Screen() { fun load() { UnknownCard() } }       -> no usages
@Composable function-type property lambda                      -> PrimaryButton
suspend { FetchRepository(); PrimaryButton() }                 -> neither call
explicit field initializer MutableStateFlow(...)               -> no unresolved call
when guard branch inside @Composable function                  -> PrimaryButton
context-parameter @Composable body                             -> PrimaryButton
```

Add token/style independence cases outside a composable and inside the preserved explicit-field initializer: `val color = AppTokens.color.primary` and the field's `Spacing.small` remain token sites, while a `Modifier.padding(7.dp)` expression that passes existing style-syntax predicates remains a hard-coded style site. Assert none creates a component usage.

Run:

```bash
cd engine
cargo test -p wax-lang-compose tree_sitter_scan::tests::component_calls_require_ui_scope -- --exact
```

Expected: FAIL because current extraction scans every PascalCase call.

- [ ] **Step 2: Add explicit UI scope and a single component walker**

Define beside usage extraction:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiScope {
    NonUi,
    Composable,
    ComposableLambda,
}

impl UiScope {
    fn is_ui(self) -> bool {
        !matches!(self, Self::NonUi)
    }
}
```

Replace the flat stack in `extract_usage_from_source` with recursive `visit_component_usage(node, inherited_scope, ...)`. Compute child scope in this order:

1. Any `SyntaxRegion.body` with `Exclude` containing the child sets `NonUi`.
2. Any `SyntaxRegion.body` with `ComposableLambda` containing the child sets `ComposableLambda`.
3. A non-preview `function_declaration` carrying `@Composable` sets `Composable` for its body.
4. Any other named `function_declaration` sets `NonUi`; a function never inherits composable scope solely because it is lexically nested in a composable.
5. Ordinary lambda descendants and other expression nodes inherit the parent scope.

Emit a call only when `scope.is_ui()`, existing preview/scaffolding checks pass, and the node belongs to a clean parse range. Preserve registry/local/unresolved classification and existing ids exactly.

- [ ] **Step 3: Make parent attribution follow the active scope**

For declaration-backed `Composable`, keep `nearest_enclosing_composable` and its current semantic `ParentScope`. For a top-level annotated composable property lambda, use `parent = None`; do not invent a local component or synthetic parent id. Nested ordinary lambdas continue attributing calls to the nearest enclosing composable declaration.

Add assertions for these three cases so scope admission and parent identity cannot drift together.

- [ ] **Step 4: Keep local indexing strict and preview-safe**

Continue indexing only PascalCase `@Composable fun` declarations. Do not index composable function-type properties. Filter local declarations to clean parse ranges and keep the existing preview/provider/effect exclusions. Add a regression proving a malformed non-UI declaration cannot create a local component after normalization.

- [ ] **Step 5: Separate token/style filters from component scope**

Introduce a shared predicate:

```rust
fn node_is_extractable(node: tree_sitter::Node<'_>, clean: &[ByteRange]) -> bool {
    clean.iter().any(|range| range.contains_node(node))
        && !node_has_error_ancestor_within(node, clean)
}
```

Use it in local, usage, token, and hard-coded-style traversals. Only usage traversal consults `UiScope`. Token/style traversals skip type-annotation ranges, but continue through the preserved initializer body of an explicit backing field; `ComponentScopePolicy::Exclude` applies only to component/unresolved-call extraction. Otherwise retain the existing syntactic candidate rules.

- [ ] **Step 6: Update golden counts only for proven false positives**

Run the small golden test. If counts change, list every removed usage id and its enclosing non-UI syntax in the PR description. Update `golden.json` only for calls outside UI-bearing scopes. Do not accept token/style or resolved/candidate changes without a separate root-cause test.

- [ ] **Step 7: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose
cargo clippy -p wax-lang-compose --all-targets -- -D warnings
cd ..
git add engine/crates/wax-lang-compose docs/plans/2026-07-22-compose-parse-recovery-plan.md
git commit -m "fix: scope compose usage facts to ui code"
```

Expected: all commands exit `0`; infrastructure constructors no longer contribute unresolved UI calls.

---

### Task 4: Recover Later Clean Islands and Deduplicate Facts

- [ ] **Task 4 complete**

**Files:**
- Modify: `engine/crates/wax-lang-compose/src/kotlin_recovery.rs`
- Modify: `engine/crates/wax-lang-compose/src/kotlin_ast.rs`
- Modify: `engine/crates/wax-lang-compose/src/discover.rs`
- Modify: `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/malformed/BroadTopLevelError.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/malformed/BroadMemberError.kt`
- Create: `engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/malformed/UnbalancedDelimiters.kt`
- Modify: `engine/crates/wax-lang-compose/tests/parse_recovery.rs`
- Modify: `docs/plans/2026-07-22-compose-parse-recovery-plan.md`

**Interfaces:**
- Consumes: Task 1 `ParsePass`/`SyntaxProblem`, Task 2 lexer boundaries, Task 3 clean-range extraction.
- Produces: bounded later-source passes, progress/cap guarantees, deterministic fact merging, and `Partial` output with useful recovered facts.

- [ ] **Step 1: Add red recovery and overlap tests**

The malformed top-level fixture must place a valid composable before a broad malformed declaration and another valid composable after it. The member fixture does the same inside an object/class. `UnbalancedDelimiters.kt` must never provide a safe later boundary.

Assert:

- facts after broad top-level and member errors are recovered;
- the scan remains `Partial` with `parse_failed`;
- `recovered_later_source` is true in the selected problem;
- no fact id appears twice when primary and island passes overlap;
- unbalanced input stops normally;
- attempt offsets are strictly increasing and never exceed 64.

Run:

```bash
cd engine
cargo test -p wax-lang-compose --test parse_recovery broad_error_recovers_later_declaration -- --exact
cargo test -p wax-lang-compose kotlin_recovery::tests::recovery_attempts_are_bounded_and_monotonic -- --exact
```

Expected: FAIL because only the primary pass exists.

- [ ] **Step 2: Produce safe lexical recovery boundaries**

From the Task 2 lexer, emit boundaries at:

- top-level declaration keywords `class`, `data`, `enum`, `fun`, `interface`, `object`, `typealias`, `val`, and `var` at brace depth zero;
- class/object member declarations at the containing body's immediate brace depth;
- the next statement after a newline or semicolon at the same or shallower delimiter depth.

Never emit a boundary inside a comment, string, character literal, annotation argument, or unmatched deeper delimiter. Sort and deduplicate byte offsets.

- [ ] **Step 3: Implement bounded blank-and-reparse recovery**

Add a result type that keeps the always-present primary pass separate from zero or more later passes:

```rust
pub(crate) struct ParseRecovery {
    pub(crate) primary_clean: Vec<ByteRange>,
    pub(crate) recovered: Vec<ParsePass>,
    pub(crate) unresolved_problems: Vec<SyntaxProblem>,
}

pub(crate) fn recover_parse_passes(
    parser: &mut tree_sitter::Parser,
    normalized: &[u8],
    primary: &tree_sitter::Tree,
) -> ParseRecovery;
```

For each broad primary problem, choose the first safe boundary strictly after the problem start. Clone the normalized bytes, blank the half-open range `[problem.start, boundary)` with `mask_preserving_lines`, and reparse. The boundary token itself must remain intact. Accept a pass only when:

- parser returns a tree;
- its clean suffix starts at or after the chosen boundary;
- the first remaining problem begins after the prior accepted offset, or no problem remains;
- the suffix contains at least one named declaration or statement node.

Store `clean = [boundary..next_problem_start]` and increment priority. Continue from the next problem/boundary until EOF, no progress, or `MAX_RECOVERY_ATTEMPTS`. Track tried `(problem_start, boundary)` pairs in a `BTreeSet`. On cap/no-progress, retain the last unresolved problem. Never unwrap parser output or delimiter matches in this path.

Before returning, set `primary_clean` to the complement of the unresolved problem ranges. The caller assigns it to `ParsedKotlinFile.primary.clean` and stores only later passes in `ParsedKotlinFile.recovered`. This preserves valid primary facts before and between errors while preventing extraction from a broad error node without representing the primary pass as an optional vector element.

- [ ] **Step 4: Iterate every pass through one extraction pipeline**

In `scan_repository`, first index locals across `parsed.passes()`, then extract usages/tokens/styles across the same iterator. Pass each pass's `clean` ranges and the file's syntax regions into the existing extractors. Primary priority is `0`; recovered pass priority increases with source progress.

Update `discover_registry_symbols` to collect declarations across `parsed.passes()` with the same clean-range predicate. Its existing `BTreeMap` remains the semantic deduplication boundary, so overlapping primary and recovered declarations cannot duplicate discovered symbols. Emit the same unresolved-problem diagnostics as repository scanning.

Collect only facts admitted by `node_is_extractable` as `(priority, fact)` and resolve duplicate ids with a `BTreeMap`. Insert passes in ascending priority and retain the first fact for each id. Because error-containing nodes are rejected before insertion, no conflict rule needs to inspect an AST node after fact construction. Strip priority before returning contract facts.

Use these stable keys:

```text
LocalComponent.id
UsageSite.id
TokenSite.id
HardcodedStyleSite.id
```

Retain the existing final sort comparators after deduplication.

- [ ] **Step 5: Improve recovery diagnostic text without changing its code**

When later source was recovered, emit:

```text
tree-sitter could not fully parse <family> syntax in <file> near <line>:<column>; skipped the uncertain region and continued scanning later source; component, token, local-definition, or hard-coded-style facts in the skipped region may be incomplete
```

Use `unknown` when no known family matches. When no later island is recovered, replace the middle clause with `file scanned with gaps`. Keep code `parse_failed`, severity `Error`, and location at the smallest problem.

- [ ] **Step 6: Prove file and repository isolation**

Add a private seam without changing the public function:

```rust
type ParseFileFn = fn(
    &mut tree_sitter::Parser,
    &Path,
) -> Result<ParsedKotlinFile, ParseKotlinFileError>;

fn scan_repository_with_parser(
    repo_root: &Path,
    config: &ComposeScanConfig,
    parse_file: ParseFileFn,
) -> Result<TreeSitterScanResult, TreeSitterScanError>;
```

`scan_repository` delegates with `parse_kotlin_file_permissive`. A test parser returns `ParseKotlinFileError::ParseFailed` only for `NoTree.kt`; the same runtime repository also contains one malformed partial file and one valid file. Assert the no-tree file is skipped with a diagnostic, the malformed file contributes recoverable facts, and the valid file still scans. A separate read-permission/I/O test keeps the existing rule that a real filesystem error aborts the scan.

If a parser seam is needed, add private `parse_source_with(parser, bytes) -> Option<Tree>` and unit-test it; do not expose parser injection publicly.

- [ ] **Step 7: Verify and commit**

```bash
cd engine
cargo fmt --all
cargo test -p wax-lang-compose
cargo clippy -p wax-lang-compose --all-targets -- -D warnings
cd ..
git add engine/crates/wax-lang-compose docs/plans/2026-07-22-compose-parse-recovery-plan.md
git commit -m "fix: continue compose scans after syntax gaps"
```

Expected: all commands exit `0`; every accepted pass advances and output ids are unique.

---

### Task 5: Report Truthfully, Validate with Kotlin, and Add the Corpus Release Gate

- [ ] **Task 5 complete**

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-cli/tests/scan_command.rs`
- Create: `scripts/verify-compose-kotlin-fixtures.sh`
- Create: `scripts/test-verify-compose-kotlin-fixtures.sh`
- Create: `scripts/replay-compose-corpus.sh`
- Create: `scripts/test-replay-compose-corpus.sh`
- Modify: `.github/workflows/build_engine.yml`
- Modify: `docs/plans/2026-07-22-compose-parse-recovery-design.md`
- Modify: `docs/plans/2026-07-22-compose-parse-recovery-plan.md`
- Modify: `docs/plans/README.md`

**Interfaces:**
- Consumes: full merged diagnostics, committed compiler matrix, a maintainer-supplied corpus repo, and baseline JSON.
- Produces: truthful terminal truncation, reproducible compiler checks, an attributable corpus delta report, and final roadmap/ADR handoff data.

- [ ] **Step 1: Write failing CLI summary tests for 0, 1, 5, and 7 failures**

Change the unit helper to pass the complete filtered diagnostic vector to `write_failure_diagnostics`. Assert exact headings:

```text
failure diagnostics: none
failure diagnostics (1 total; showing 1):
failure diagnostics (5 total; showing 5):
failure diagnostics (7 total; showing 5):
```

For seven, assert exactly five formatted rows plus:

```text
  2 more diagnostics omitted; see /repo/.wax/out/scan-merged.json for the complete list.
```

Update the process-level assertion in `scan_command.rs` from `failure diagnostics (up to 5):` to `failure diagnostics (3 total; showing 3):` for its current fixture.

Run:

```bash
cd engine
cargo test -p wax-cli commands::scan::tests::failure_diagnostics_report_total_and_omitted_count -- --exact
```

Expected: FAIL because `.take(5)` currently discards the total before rendering.

- [ ] **Step 2: Move truncation into the formatter**

Collect all matching diagnostics in `write_scan_summary` by removing `.take(MAX_FAILURE_DIAGNOSTICS)`. Change the function signature to accept `output_path`:

```rust
fn write_failure_diagnostics(
    writer: &mut impl Write,
    diagnostics: &[&Diagnostic],
    output_path: &Path,
) -> Result<(), ScanCommandError>
```

Print `diagnostics.len()` and `diagnostics.len().min(MAX_FAILURE_DIAGNOSTICS)`, iterate with `.take(5)`, then print the omitted count only when positive. Use the actual `output_path` already passed to `write_scan_summary`; do not hard-code `.wax/out`.

- [ ] **Step 3: Add the explicit Kotlin compiler validator**

`scripts/verify-compose-kotlin-fixtures.sh` accepts exactly:

```text
verify-compose-kotlin-fixtures.sh --version <2.x.y> --compiler </absolute/path/to/kotlinc>
```

It reads `compiler-matrix.tsv`, selects rows for the version, creates `mktemp -d`, traps cleanup, and invokes the supplied compiler once per selected fixture:

```bash
flags=()
if [[ "$flags_field" != "-" ]]; then
  read -r -a flags <<< "$flags_field"
fi
"$compiler" "$fixture" -d "$temp_dir/${fixture_name}.jar" "${flags[@]}"
```

Treat `-` as no flags, reject unknown arguments/relative compiler paths/missing executable/no matrix rows, and never use `eval` or download during normal invocation. The offline shell test supplies a fake executable that records arguments and proves version filtering, flags, space-safe paths, cleanup, and nonzero propagation.

- [ ] **Step 4: Add the pinned CI compiler matrix**

Add a `verify-compose-kotlin-fixtures` job with versions `2.1.0`, `2.2.0`, `2.3.0`, and `2.4.0`. For each matrix entry, download only the official release archive:

```text
https://github.com/JetBrains/kotlin/releases/download/v<VERSION>/kotlin-compiler-VERSION.zip
```

Unzip under `${RUNNER_TEMP}/kotlin-VERSION`, then call the validator with its absolute `bin/kotlinc` path. Also add `../scripts/test-verify-compose-kotlin-fixtures.sh` and `../scripts/test-replay-compose-corpus.sh` to the existing engine `verify` job. Add the four scripts and Kotlin fixture directory to workflow path filters.

- [ ] **Step 5: Add a deterministic maintainer corpus replay command**

`scripts/replay-compose-corpus.sh` accepts:

```text
replay-compose-corpus.sh --repo <absolute-path> --wax-bin <absolute-path> --baseline <json> [--max-slowdown-percent 10]
```

Validate that the repo contains `.wax/wax.config.json`, run the supplied binary with `scan --repo-root <repo> --no-auto-install`, and copy `.wax/out/scan-merged.json` to a temporary file. Emit a JSON report with `jq` containing:

```json
{
  "status": "complete|partial|failed",
  "parse_failure_count": 0,
  "files_scanned": 0,
  "usage_site_ids": [],
  "local_component_ids": [],
  "token_site_ids": [],
  "hardcoded_style_site_ids": [],
  "parse_extract_ms": 0,
  "baseline_parse_extract_ms": 0,
  "slowdown_percent": 0.0
}
```

Compare sorted ids and diagnostics with the baseline. Exit nonzero for pack `Failed`, lost pre-existing ids, any known-family `parse_failed`, unattributed new/removed ids, or slowdown greater than the configured percentage. Accept expected delta ids through baseline arrays `expected_added_ids` and `expected_removed_false_positive_ids`. Do not commit the proprietary 54-file corpus or its source paths.

The offline shell test uses a fake Wax binary and temporary repo to cover success, lost-id failure, known-syntax failure, and 10% slowdown failure.

- [ ] **Step 6: Run the complete release gate**

Run the normal checks:

```bash
scripts/test-verify-compose-kotlin-fixtures.sh
scripts/test-replay-compose-corpus.sh
cd engine
cargo fmt --all --check
cargo test -p wax-lang-compose
cargo test -p wax-cli
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Then, with the maintainer-provided compilers and corpus:

```bash
scripts/verify-compose-kotlin-fixtures.sh --version 2.1.0 --compiler "$KOTLINC_2_1_0"
scripts/verify-compose-kotlin-fixtures.sh --version 2.2.0 --compiler "$KOTLINC_2_2_0"
scripts/verify-compose-kotlin-fixtures.sh --version 2.3.0 --compiler "$KOTLINC_2_3_0"
scripts/verify-compose-kotlin-fixtures.sh --version 2.4.0 --compiler "$KOTLINC_2_4_0"
scripts/replay-compose-corpus.sh \
  --repo "$COMPOSE_CORPUS_REPO" \
  --wax-bin "$PWD/engine/target/release/wax" \
  --baseline "$COMPOSE_CORPUS_BASELINE" \
  --max-slowdown-percent 10
```

Expected: every command exits `0`; no known syntax family remains partial; no prior ids are lost; only recovered UI facts and named false-positive removals differ; slowdown is at most 10%.

- [ ] **Step 7: Close out documentation and commit**

Record the exact grammar/compiler versions and corpus before/after counts in the design's investigation section. Mark implementation complete in the roadmap only after Tasks 1–5 are merged. Archive the plan and create an ADR only after the entire plan ships; do not do either in an individual task PR.

```bash
git add engine/crates/wax-cli scripts .github/workflows/build_engine.yml docs/plans
git commit -m "test: gate compose parse recovery"
```

Expected: the Task 5 PR contains reporting, validation tooling, CI wiring, and checked plan steps only.

---

## Plan-Level Acceptance Checklist

- [ ] Every valid fixture compiles under every declared Kotlin version/flag combination.
- [ ] Every valid fixture parses without remaining `ERROR` or `MISSING` nodes after byte-preserving normalization.
- [ ] Facts before and after each known construct keep their original line and column.
- [ ] Guarded branches, annotated composable slot lambdas, and composable context-parameter bodies contribute UI facts.
- [ ] Suspend lambdas, explicit backing fields, and annotated type arguments do not contribute component or unresolved UI facts.
- [ ] An unknown syntax gap cannot suppress a later recoverable declaration or later file.
- [ ] Recovery attempts advance monotonically, stop at 64 or fewer, and never panic.
- [ ] Primary/recovery overlap cannot duplicate a fact id.
- [ ] Known complete recovery reports `Complete`; uncertain skipped regions report `Partial` with the smallest useful location.
- [ ] Terminal output prints total, shown, and omitted diagnostic counts while JSON retains every diagnostic.
- [ ] `ScanFacts`, schemas, parser identity, fact ids, sorting, and registry semantics remain compatible.
- [ ] The 54-file corpus has no known-family gaps, no unexplained metric changes, and no more than 10% runtime regression.
