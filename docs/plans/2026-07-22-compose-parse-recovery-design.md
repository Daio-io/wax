# Compose Parse Recovery and UI Scope Design

## Summary

Wax should treat Kotlin parsing as a fault-tolerant source of UI facts, not as a requirement that every byte of a file match the bundled grammar. A recoverable syntax error must never crash the Compose pack, abort a repository scan, or prevent later valid declarations in the same file from being analyzed.

The scanner should also stop treating every PascalCase Kotlin call as potential UI. Component usages and unresolved UI calls should come only from UI-bearing scopes. Valid but non-UI Kotlin constructs such as suspend-lambda builders, explicit backing fields, and annotated generic arguments must be tolerated so scanning can continue, but their infrastructure calls must not affect design-system component metrics.

This design keeps the existing `ScanFacts` schema and the current `tree-sitter-kotlin-ng` dependency. It adds byte-preserving normalization for known grammar gaps, bounded recovery for unknown gaps, Compose-aware scope tracking during extraction, precise diagnostics, and truthful CLI truncation reporting.

## Investigation Findings

Wax currently bundles:

- `tree-sitter` 0.25.10;
- `tree-sitter-kotlin-ng` 1.1.0;
- a byte-preserving workaround for annotated parenthesized function-type parameters.

Direct parser probes and full Wax scans established:

- suspend lambda expressions produce a broad `ERROR` node;
- `when` guards produce a broad `ERROR` node;
- annotated property and receiver function types produce `ERROR` or `MISSING` nodes outside the existing parameter-only workaround;
- explicit backing fields produce a broad `ERROR` node;
- named `context(name: Type)` syntax is a context parameter, not a legacy context receiver, and produces a broad `ERROR` node;
- annotated generic type arguments produce an `ERROR` range that can consume later declarations;
- the supplied trailing comma before a `when` branch arrow already parses without errors in the pinned grammar.

For broad error nodes, facts before the unsupported construct survive while later composable declarations, component calls, and token references disappear. The diagnostic often points to the containing declaration because the current locator returns the first outer `ERROR` node before inspecting narrower descendants.

A probe against the published `tree-sitter-kotlin-sg` 0.4.1 grammar recovered suspend lambdas, `when` guards, and annotated generic arguments. It was not a drop-in replacement: 12 existing `wax-lang-compose` tests failed in import resolution, package-qualified local identity, hard-coded style extraction, and annotated function-type handling. It also caused infrastructure calls such as `MutableStateFlow(...)` to appear as unresolved UI calls. A grammar replacement therefore mixes parser recovery with a broad AST migration and does not solve UI scoping by itself.

The CLI issue is independent and confirmed: summary rendering calls `.take(5)` before formatting diagnostics, so it cannot print the true total or state how many diagnostics were omitted. JSON output retains the full list.

## Goals

- Never panic, abort the Compose pack, or abandon the remaining repository because one Kotlin file contains unsupported or malformed syntax.
- Continue extracting facts from valid syntax before and after the smallest unrecoverable region in a file.
- Handle the known valid Kotlin syntax families without losing later UI facts.
- Analyze UI-bearing portions of `when` guards, annotated composable function types, and context-parameter declarations.
- Tolerate suspend-lambda builders, explicit backing fields, and annotated generic arguments without classifying their infrastructure calls as UI.
- Restrict component usage and unresolved UI-call facts to Compose UI-bearing scopes.
- Preserve token references and syntactically UI-specific hard-coded style candidates from valid expression regions, including reusable UI helpers.
- Select the narrowest useful parser diagnostic location and explain whether metrics may be incomplete.
- Report the total number of failure diagnostics in terminal output, explicitly state truncation, and keep all diagnostics in JSON.
- Validate supported valid fixtures with pinned Kotlin compilers and flags as well as tree-sitter.
- Keep scan output deterministic and within the existing contract schema.

## Non-Goals

- Wax will not become a Kotlin compiler or type checker.
- Wax will not guarantee a perfect tree for every current or future Kotlin language feature.
- Wax will not execute Gradle, project build logic, annotation processors, or user code during a normal scan.
- Wax will not count arbitrary constructors or PascalCase calls as UI merely because parsing recovered them.
- Wax will not switch Kotlin tree-sitter grammars as part of this change.
- Wax will not add a Compose-specific output schema.
- Wax will not suppress diagnostics for malformed syntax when analysis completeness is uncertain.
- Wax will not infer that every token or style value used outside a composable is irrelevant; reusable UI constants and `Modifier` helpers remain in scope.

## Product Decisions

### Parsing and analysis are separate concerns

Wax must tolerate syntax in every scanned Kotlin file so later UI islands remain visible. Tolerating a construct does not make the construct a UI scope and does not authorize facts from every call inside it.

### Partial syntax is a recoverable file condition

Parser initialization failure remains fatal because no Kotlin file can be parsed. Reading an unreadable configured file remains an I/O error. Once a source file has been read and a parser initialized, syntax errors are recoverable:

- the file contributes every fact Wax can establish safely;
- later files are always scanned;
- later recoverable regions in the same file are scanned;
- uncertainty is represented through diagnostics and `ScanStatus::Partial`, not a process failure.

### Complete means no known metric gap

A known syntax normalization may still produce `Complete` when it is byte-preserving, the normalized construct is fully covered by tests, and every fact category defined as relevant for that construct is analyzed. Unknown or only partially recovered syntax produces `Partial` because Wax cannot prove that no relevant fact was skipped.

### UI-bearing scopes govern component facts

Component usages, local component invocations, and unresolved UI calls are emitted only from:

- a function declaration annotated `@Composable`;
- a lambda whose declared or expected source type is annotated `@Composable` and whose syntax can be associated statically;
- a lambda nested within an already established composable render scope, unless a nested construct explicitly leaves UI scope.

No additional inferred render scopes are introduced by this change. Expanding the list requires a separate design decision and corpus evidence.

A suspend-lambda builder explicitly leaves component UI scope. An explicit backing-field initializer is not a component UI scope. Annotation arguments and type syntax are never expression-extraction scopes.

### Tokens and hard-coded styles retain their existing UI-specific signals

Known registry token references remain reportable from valid expression nodes outside composables because UI constants and state holders can reference design-system tokens before a composable consumes them.

Hard-coded style candidates remain reportable outside composables only when the existing extractor recognizes UI-specific syntax or context, such as `Modifier` styling, `dp` dimensions, colors, shapes, or other supported styling APIs. Arbitrary literals remain excluded.

## Syntax Policy

| Syntax family | Parse/recovery behavior | Analysis behavior |
|---|---|---|
| Suspend lambda builder | Normalize or recover without losing the suffix; record its source range | Do not emit component or unresolved UI-call facts from the suspend lambda; valid token/style expressions follow their normal extractors |
| `when` guard | Mask only the guard clause for the pinned grammar while preserving the branch condition, arrow, and body | Ignore guard logic for UI-call classification; inherit the surrounding UI scope in the branch body |
| Annotated function type | Generalize the existing byte-preserving parenthesis normalization to properties, return types, receiver function types, and constructor parameters | Treat an initializer lambda with `@Composable` function type as UI-bearing |
| Explicit backing field | Normalize the field declaration or skip only its declaration region; preserve later members and declarations | Do not classify initializer constructors as component or unresolved UI calls; tokens/styles remain eligible when parsed safely |
| Context parameter | Mask the context prefix when necessary while preserving the annotated function/property body | Analyze an associated `@Composable` body or recognized `Modifier` helper normally |
| Legacy context receiver | Apply the same recovery boundary for projects compiled with the older opt-in syntax | Analyze the associated body using the same rules as context parameters |
| Trailing comma in `when` conditions | Keep as a positive regression fixture; no current normalization | Analyze the branch body normally |
| Annotated generic type argument | Mask the type-use annotation when necessary while preserving the generic type, trailing comma, and suffix | Do not traverse type annotations as expression scopes; resume normal extraction afterward |

## Architecture

The Compose scan pipeline becomes:

```text
Kotlin source
  -> lexical pass that understands comments, strings, delimiters, and declaration boundaries
  -> byte-preserving known-syntax normalization + recorded source regions
  -> primary tree-sitter parse
  -> syntax-problem collection and recovery assessment
  -> bounded island reparses when a broad error swallows later source
  -> merge primary and recovered trees by stable source identity
  -> scope-aware component extraction
  -> token and hard-coded style extraction from valid expression regions
  -> precise diagnostics and Complete/Partial status
```

### Parsed file model

`ParsedKotlinFile` should continue to retain the original source and primary tree. It should gain recovery metadata rather than exposing transformed source to callers:

- normalized regions with a stable syntax-family label and original byte range;
- regions excluded from component-call extraction;
- recovered parse islands with their original byte ranges;
- remaining syntax problems after recovery;
- whether any problem can affect a supported fact category.

Every transform must preserve byte length and newline positions. Tree-sitter locations can therefore continue to index the original source, and emitted `SourceLocation` values remain correct.

### Known-syntax normalization

The existing Kotlin lexical helpers already skip line comments, nested block comments, quoted literals, and triple-quoted strings. Extend this pass rather than adding regular-expression replacements.

Normalization is allowed only when:

- the construct is recognized unambiguously outside comments and strings;
- delimiters balance within the candidate range;
- replacement bytes preserve length and newlines;
- a focused fixture proves the Kotlin compiler accepts the original source;
- parser tests prove the normalized source has no `ERROR` or `MISSING` nodes in the covered construct;
- scanner tests prove facts before and after the construct retain their original locations.

If those conditions are not met, leave the source unchanged and use generic recovery.

### Generic bounded recovery

Tree-sitter already recovers many local errors. Generic recovery is needed when an `ERROR` node spans a broad containing declaration or reaches the end of the file.

For each broad problem:

1. Use the lexical pass to find the next safe declaration or statement boundary at the same or a shallower delimiter depth.
2. Create a byte-preserving recovery buffer that blanks only the failed region before that boundary.
3. Reparse the buffer with the same parser.
4. Accept the recovered island only when it advances beyond the previous error and yields named syntax nodes without a broader error.
5. Repeat with a fixed progress condition; stop at end-of-file or when no later boundary improves recovery.
6. Merge facts by their existing stable ids so reparsed overlap cannot duplicate output.

This is the equivalent of moving to the next safe token after failure, but it is bounded by lexical structure rather than advancing one byte at a time. It prevents infinite loops and avoids quadratic reparsing on every token.

Unknown skipped regions always retain a `Partial` result and a diagnostic. The recovery path must never panic even if delimiters are malformed or tree-sitter returns no tree.

### Scope-aware extraction

Component extraction should carry an explicit scope value while traversing nodes:

```text
NonUi
Composable
ComposableLambda
```

Entering an `@Composable` declaration or statically annotated composable lambda sets a UI scope. Normal nested lambdas inherit the enclosing scope because Compose APIs commonly use trailing content lambdas. Entering a known suspend-lambda builder or backing-field initializer sets `NonUi` for component and unresolved-call extraction.

Registry and local component calls are emitted only from a UI scope. Unknown PascalCase calls outside a UI scope are ignored rather than emitted as unresolved UI calls. Token and hard-coded style extraction remain separate traversals with their existing context predicates and recovery-region checks.

### Fact merging

Primary and recovered parses can overlap. Continue using the existing fact ids and sort order, then deduplicate by id before contract validation:

- local components by local definition id;
- usage sites by usage id;
- token sites by token-site id;
- hard-coded style sites by hard-coded-site id.

Conflicting facts with the same id are resolved in favor of the primary parse unless the primary fact came from an error-containing node and the recovered fact came from a clean island.

## Diagnostics

### Collection

Collect every `ERROR` and `MISSING` node before selecting a diagnostic location. Prefer:

1. the smallest `MISSING` node with a useful expected token;
2. the smallest nested `ERROR` range;
3. the outer recovery boundary only when no narrower problem exists.

Known normalized syntax can emit an informational `syntax_recovered` diagnostic during development fixtures, but production output should avoid noise when recovery is complete and metrics have no known gap.

Remaining gaps use `parse_failed` with language that states:

- the smallest skipped or uncertain region;
- that later source was still scanned when recovery succeeded;
- which fact categories may be incomplete when known;
- the likely syntax family when classification is unambiguous.

### Status

- `Complete`: every file was read, all roots resolved, and no remaining syntax problem can affect a supported fact category.
- `Partial`: a root is missing, a source region remains uncertain, or a file could not produce any tree.
- `Failed`: reserved for existing pack-level failure semantics; an individual source syntax problem never causes it.

### Terminal summary

The CLI should compute the full filtered diagnostic list before truncation. For example:

```text
failure diagnostics (54 total; showing 5):
  ...
  49 more diagnostics omitted; see .wax/out/scan-merged.json for the complete list.
```

When a language is partial because of parsing, the summary should explain that component, token, local-definition, or hard-coded-style metrics may be incomplete according to the diagnostic metadata available. No scan-output schema change is required; full structured diagnostics remain in JSON.

## Kotlin Compiler Validation

Valid parser fixtures must also be valid Kotlin. Compiler validation uses these exact releases:

- Kotlin 2.1.0 with `-Xwhen-guards` for preview guard syntax;
- Kotlin 2.2.0 with `-Xcontext-parameters` for named context parameters;
- Kotlin 2.2.0 with `-Xcontext-receivers` for the legacy context-receiver fixture;
- Kotlin 2.3.0 with `-Xexplicit-backing-fields` for preview explicit fields;
- Kotlin 2.4.0 without preview flags for stable guards, context parameters, and explicit backing fields;
- Kotlin 2.4.0 for established suspend-lambda, annotation, and trailing-comma fixtures.

Normal Rust tests must not require a globally installed Kotlin compiler. A dedicated script should accept explicit compiler paths or download pinned official compiler archives into a temporary cache for CI. Normal Wax scanning continues to have no Kotlin runtime or compiler dependency.

## Testing Strategy

### Parser and normalization tests

For each syntax family, add a positive original-source fixture and assert:

- Kotlin compiler validation succeeds under its declared version and flags;
- byte count and newline positions are unchanged by normalization;
- normalized parsing has no unexpected `ERROR` or `MISSING` nodes;
- declaration, expression, and body nodes retain correct ranges;
- syntax after the construct remains reachable.

Add malformed variants that assert bounded recovery advances or stops without looping, panicking, or hiding the remaining diagnostic.

### Scanner integration tests

Each fixture should contain known facts before, inside when relevant, and after the construct:

- a registry-backed component call;
- a local `@Composable` declaration;
- a registry token reference;
- a hard-coded style candidate.

Assert the syntax policy explicitly:

- calls inside `suspend {}` do not become component or unresolved UI facts;
- `MutableStateFlow(...)` in an explicit backing field does not become an unresolved UI call;
- a component call in a guarded `when` branch is counted;
- a component call in an annotated composable slot lambda is counted;
- an annotated context-parameter composable body is counted;
- facts after every construct are recovered with original line and column values;
- token and hard-coded style behavior matches the product decisions above.

### Recovery tests

Add unknown and malformed syntax fixtures that prove:

- a broad error cannot suppress a later top-level composable;
- a broad class-member error cannot suppress later recoverable members;
- recovery makes monotonic byte progress;
- overlapping primary and recovered parses do not duplicate facts;
- fully unparseable input yields one or more diagnostics and does not abort other files;
- an absent tree skips only that file and leaves the repository scan running.

### CLI tests

Verify:

- the actual total is printed before truncation;
- exactly five rows are shown when more than five matching diagnostics exist;
- the omitted count is correct;
- the JSON path is shown for complete diagnostics;
- partial parsing explains likely metric impact;
- JSON output still contains every diagnostic.

### Corpus regression

Provide a corpus replay command that accepts a maintainer-supplied repository path and compiler configuration. Against the original 54-file corpus, the release gate is:

- no crash, pack failure, or repository abort;
- no lost facts after any reported syntax construct;
- no component or unresolved UI facts from tolerance-only infrastructure scopes;
- no remaining parse gap for the known valid fixtures;
- every remaining partial diagnostic maps to a smaller unsupported or malformed region;
- metric deltas are attributable to recovered UI-bearing facts or removed false positives;
- scan time remains within 10% of the current corpus baseline.

The proprietary or external corpus does not need to be committed. Its reduced, non-sensitive reproductions should be committed as permanent fixtures.

## Error and Performance Bounds

- Recovery attempts must be bounded by the number of lexical recovery boundaries in the file.
- Every accepted attempt must advance the last recovered byte offset.
- The same source range must not be reparsed more than once for the same recovery purpose.
- Recovery attempts are capped at the smaller of the number of lexical recovery boundaries and 64 attempts per file; reaching the cap produces a normal `Partial` diagnostic, never a panic.
- Repository scanning continues after every file-level syntax result.
- The corpus performance gate is no more than 10% slower than the current baseline.

## Product Contracts and Compatibility

- Keep `ScanFacts` and its JSON schema unchanged.
- Keep existing fact ids and deterministic sorting.
- Keep full diagnostics in language and merged JSON output.
- Diagnostic message text may improve; `parse_failed` remains the stable code for a remaining syntax gap.
- Additive informational recovery diagnostics must not cause `Partial` by themselves.
- Compose-specific UI scope tracking does not change React or Swift syntax rules. It aligns outcomes: each parser-backed pack should count component usage only from syntax its ecosystem defines as UI-bearing.
- Registry resolution, token ids, and source locations remain backward-compatible.

## Alternatives Considered

### Replace the grammar now

`tree-sitter-kotlin-sg` 0.4.1 handles several dominant syntax gaps, but it changes AST shapes and failed 12 existing Compose tests in the probe. It also does not fully support context parameters or explicit backing fields. Migrating remains viable as a separate project after adding grammar-agnostic AST adapters and corpus parity tests, but it is too broad for this recovery fix.

### Fork and publish a patched grammar

Porting selected rules into a Wax-maintained grammar could produce accurate trees for the known syntax. It also creates a new release, security, and Kotlin-language maintenance surface. Wax still needs generic recovery and UI scope tracking for future grammar lag, so a fork does not remove the core work. Defer it unless byte-preserving recovery proves insufficient for an analysis-relevant construct.

### Use generic recovery only

Skipping to the next boundary without recognizing known syntax would protect later declarations, but it would lose guarded branch bodies and composable slot lambdas that matter directly to design-system analysis. Use generic recovery as defense in depth after focused known-syntax normalization, not as the only mechanism.

## Delivery and Roadmap Gate

Implementation should be split into focused reviewable tasks: recovery metadata and diagnostics, known syntax normalization, scope-aware extraction, generic bounded recovery, CLI reporting, compiler validation, and corpus replay. The detailed implementation plan should preserve the repository rule of one checked task per focused PR.

Token inference and reporting (order 14) is complete and archived, and the current roadmap has no promoted active plan. This Compose recovery work may be fully planned now, but implementation starts only when the maintainer promotes it or explicitly grants an exception to the active-plan gate.

## Acceptance Criteria

- No recoverable Kotlin syntax condition panics the pack, aborts repository scanning, or prevents later files from scanning.
- Known valid fixtures compile under their declared Kotlin versions and flags.
- Facts after all known syntax families are recovered with correct locations.
- Guarded branch bodies, annotated composable slot lambdas, and composable context-parameter bodies contribute expected UI facts.
- Suspend lambdas, backing-field initializers, annotations, and generic type syntax do not introduce component or unresolved UI-call false positives.
- Unknown syntax recovery advances to later safe islands, deduplicates facts, and remains `Partial` when completeness is uncertain.
- Diagnostics identify the narrowest useful syntax problem and explain likely metric impact.
- Terminal output reports the total failure count, explicitly reports truncation, and points to complete JSON diagnostics.
- Existing schemas, successfully parsed facts, and deterministic output do not regress.
- The original 54-file corpus meets the stated correctness and performance gates before release.
