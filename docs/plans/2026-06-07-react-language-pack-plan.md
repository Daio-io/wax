# React Language Pack Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or `superpowers:executing-plans` to implement this plan task-by-task after this plan is approved and scheduled in `docs/plans/README.md`.

**Goal:** Promote `wax-lang-react` from a scaffold to a production SWC-backed language pack that emits registry components, local components, and resolved design-system JSX usage sites through the existing `ScanFacts` contract.

**Architecture:** `wax-lang-react` parses React scan config, loads the Wax registry, resolves source roots, parses JS/TS/JSX/TSX files with SWC, builds an import/export module graph, discovers local React components, resolves JSX usage to registry symbols, and emits deterministic facts. `wax-core` and `wax-cli` should not need React-specific report logic.

**Tech Stack:** Rust 2024, SWC crates, serde JSON, existing `wax-contract` and `wax-lang-api`, existing root resolution helpers, parser-backed language-pack subprocess protocol.

---

## Reference Spec

- Design spec: `docs/plans/2026-06-07-react-language-pack-design.md`
- Capability roadmap: `docs/plans/2026-06-07-react-language-pack-roadmap.md`
- Language-pack contract: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Roadmap source: `docs/plans/README.md`

## Scheduling Gate

This plan is the current active implementation plan. Task 3 is complete in this PR; after it lands, Task 4 is the next task to execute.

## File Structure

- Modify `engine/crates/wax-lang-react/Cargo.toml`
  - Add SWC and helper dependencies.
- Modify `engine/crates/wax-lang-react/src/lib.rs`
  - Replace scaffold scan behavior with configured production scan behavior. Preserve the current empty-config scaffold response only for contributor smoke compatibility until React is promoted to the public pack index.
- Create `engine/crates/wax-lang-react/src/config.rs`
  - Parse and validate React scan config.
- Create `engine/crates/wax-lang-react/src/registry.rs`
  - Load registry symbols and aliases into a React resolver index.
- Create `engine/crates/wax-lang-react/src/files.rs`
  - Resolve roots and collect supported source files.
- Create `engine/crates/wax-lang-react/src/swc_parse.rs`
  - Parse JS/TS/JSX/TSX files and map SWC spans to repo-relative locations.
- Create `engine/crates/wax-lang-react/src/module_graph.rs`
  - Resolve imports, exports, one-hop direct re-exports, aliases, and configured package entrypoints.
- Create `engine/crates/wax-lang-react/src/extract.rs`
  - Discover local components and collect JSX usage sites.
- Create `engine/crates/wax-lang-react/src/facts.rs`
  - Convert scan results into validated `ScanFacts`.
- Modify `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs`
  - Map production errors to stable wire error codes.
- Create `engine/crates/wax-lang-react/tests/fixtures/small/...`
  - React fixture with registry, imports, aliases, locals, and unresolved cases.
- Create `engine/crates/wax-lang-react/tests/golden_small.rs`
  - End-to-end fact assertions.
- Modify release/index docs only when React is promoted to a public pack index.

## Phase 1 - Config, Registry, and File Collection

### - [x] Task 1: Add React scan config parsing

**Files:**
- Create: `engine/crates/wax-lang-react/src/config.rs`
- Modify: `engine/crates/wax-lang-react/src/lib.rs`

- [x] **Step 1: Define `ReactScanConfig`**

Include:

- `design_system_registry: PathBuf`
- `roots: Vec<PathBuf>`
- `ignore: Vec<String>`
- `tsconfig: Option<PathBuf>`
- `aliases: BTreeMap<String, Vec<String>>`
- `packages: BTreeMap<String, PackageConfig>`

- [x] **Step 2: Define config modes**

Use the Compose pattern:

- `Scaffold` for empty config to preserve the current contributor-only stdio smoke path.
- `Configured(ReactScanConfig)` when registry and roots are present.

- [x] **Step 3: Validate paths**

Reject absolute paths and parent-directory segments for registry, roots, `tsconfig`, aliases, package export targets, and ignore patterns that are path-like escapes. These are fatal config errors and should map to a wire error, not partial facts.

- [x] **Step 4: Add focused config tests**

Run:

```bash
cd engine
cargo test -p wax-lang-react config
```

### - [x] Task 2: Load React registry symbols

**Files:**
- Create: `engine/crates/wax-lang-react/src/registry.rs`
- Modify: `engine/crates/wax-lang-react/src/lib.rs`

- [x] **Step 1: Read schema v1 registry JSON**

Load `components[].symbol`, optional `components[].aliases`, and optional `components[].targets`.

- [x] **Step 2: Build canonical and alias maps**

Map every observed registry name to its canonical registry symbol for components available to React. If `targets` is missing or null, include the component. If `targets` is present, include the component only when it contains `"react"`.

- [x] **Step 3: Exclude non-React targets from React facts**

Do not emit non-React-targeted registry entries in `design_system_components`, and do not let them contribute to React coverage counts.

- [x] **Step 4: Add invalid registry diagnostics/errors**

Malformed JSON, missing components array, non-string symbols, and empty registries must fail with typed errors.

- [x] **Step 5: Add registry unit tests**

Cover omitted `targets`, null `targets`, `targets: ["react"]`, and `targets` arrays that exclude React.

Run:

```bash
cd engine
cargo test -p wax-lang-react registry
```

### - [x] Task 3: Collect React source files

**Files:**
- Create: `engine/crates/wax-lang-react/src/files.rs`
- Modify: `engine/crates/wax-lang-react/src/lib.rs`

- [x] **Step 1: Resolve source roots**

Use existing `wax-lang-api` root helpers so wildcard behavior matches Compose.

- [x] **Step 2: Collect supported files**

Include `.js`, `.jsx`, `.ts`, and `.tsx`. Exclude `.d.ts`.

- [x] **Step 3: Add default and configured skip patterns**

Skip common generated, declaration, story, and test files through documented defaults. Apply configured `ignore` patterns after defaults.

- [x] **Step 4: Add file collection tests**

Run:

```bash
cd engine
cargo test -p wax-lang-react files
```

## Phase 2 - SWC Parse and Module Graph

### - [ ] Task 4: Add SWC parser wrapper

**Files:**
- Modify: `engine/crates/wax-lang-react/Cargo.toml`
- Create: `engine/crates/wax-lang-react/src/swc_parse.rs`

- [ ] **Step 1: Add SWC dependencies**

Use crate versions compatible with the workspace and Rust edition.

- [ ] **Step 2: Parse TypeScript with JSX enabled**

Support `.js`, `.jsx`, `.ts`, and `.tsx` through one parser path.

- [ ] **Step 3: Convert parser errors into diagnostics**

Parse failures should mark the scan `Partial` and skip only the failed file.

- [ ] **Step 4: Add parser tests**

Run:

```bash
cd engine
cargo test -p wax-lang-react swc_parse
```

### - [ ] Task 5: Build import/export module graph

**Files:**
- Create: `engine/crates/wax-lang-react/src/module_graph.rs`

- [ ] **Step 1: Index imports**

Support named imports, default imports, namespace imports, and local aliases.

- [ ] **Step 2: Index exports**

Support named exports, default exports, and one-hop direct re-exports such as `export { Button } from "./Button"`. Deeper barrel chains and multi-hop re-export graphs are deferred to React v1.1.

- [ ] **Step 3: Resolve relative imports**

Resolve extensionless paths and `index` modules for supported source extensions.

- [ ] **Step 4: Resolve configured aliases and package entrypoints**

Use `tsconfig`, explicit `aliases`, and `packages` config to map import specifiers to repo-relative source modules.

- [ ] **Step 5: Emit resolver diagnostics**

Unresolved design-system-relevant imports and exports should not fail the whole scan unless config is invalid. Emit diagnostics only when a configured design-system package import, configured package entrypoint, or registry-name candidate cannot resolve.

- [ ] **Step 6: Add graph tests**

Run:

```bash
cd engine
cargo test -p wax-lang-react module_graph
```

## Phase 3 - Extraction and Facts

### - [ ] Task 6: Discover local React components

**Files:**
- Create: `engine/crates/wax-lang-react/src/extract.rs`

- [ ] **Step 1: Detect JSX-returning declarations**

Support PascalCase function declarations and PascalCase arrow/function expressions.

- [ ] **Step 2: Detect simple exported components**

Support named exports and default exports when a stable component name can be derived.

- [ ] **Step 3: Detect simple wrapper calls**

Support direct `memo(Component)` and `forwardRef(function Component(...))` cases.

- [ ] **Step 4: Add local component tests**

Run:

```bash
cd engine
cargo test -p wax-lang-react extract local_component
```

### - [ ] Task 7: Resolve JSX usage to registry symbols

**Files:**
- Modify: `engine/crates/wax-lang-react/src/extract.rs`
- Modify: `engine/crates/wax-lang-react/src/module_graph.rs`

- [ ] **Step 1: Collect JSX opening elements**

Ignore fragments and lowercase intrinsic HTML elements.

- [ ] **Step 2: Resolve JSX bindings through the module graph**

Resolve local aliases to imported/exported symbols.

- [ ] **Step 3: Match against registry index**

Emit `UsageSite` only when the resolved symbol or alias maps to a registry component.

- [ ] **Step 4: Add scoped unresolved usage diagnostics**

Unresolved JSX names should produce diagnostics only when they are design-system-relevant candidates: imported from configured design-system packages, matched by configured package entrypoints, or matching registry symbols or aliases. Ordinary local and third-party JSX names should not produce diagnostics. Unresolved candidates must not affect resolved counts.

- [ ] **Step 5: Add usage extraction tests**

Run:

```bash
cd engine
cargo test -p wax-lang-react extract usage
```

### - [ ] Task 8: Emit validated `ScanFacts`

**Files:**
- Create: `engine/crates/wax-lang-react/src/facts.rs`
- Modify: `engine/crates/wax-lang-react/src/lib.rs`
- Modify: `engine/crates/wax-lang-react/src/bin/wax-lang-react.rs`

- [ ] **Step 1: Assemble facts**

Populate metadata with `language.id = "react"`, `ecosystem = "react"`, `parser_name = "swc"`, and a maintained parser-version constant that is updated with SWC dependency bumps.

- [ ] **Step 2: Recompute and validate counts**

Use existing contract helpers before returning facts.

- [ ] **Step 3: Map errors to wire responses**

Fatal config errors, parser initialization errors, registry errors, and scan failures should map to stable wire error codes. Recoverable resolver gaps should return partial facts with diagnostics.

- [ ] **Step 4: Add golden fixture test**

Run:

```bash
cd engine
cargo test -p wax-lang-react --test golden_small
```

## Phase 4 - Integration and Docs

### - [ ] Task 9: Preserve engine integration contracts

**Files:**
- Existing engine integration tests as needed.

- [ ] **Step 1: Verify subprocess protocol**

Run:

```bash
cd engine
cargo test -p wax-lang-react --test stdio_cli
```

- [ ] **Step 2: Verify workspace tests affected by React facts**

Run focused `wax-core` or `wax-cli` tests if fixture behavior changes.

### - [ ] Task 10: Document React v1 behavior

**Files:**
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`
- Modify: `README.md` only when React is ready for public install docs.
- Modify release/index docs only when React is added to a public pack index.

- [ ] **Step 1: Document config fields**

Explain `tsconfig`, `aliases`, and `packages`.

- [ ] **Step 2: Document accuracy model**

State that resolved design-system usage is import-aware and registry-backed.

- [ ] **Step 3: Keep release docs honest**

Do not add React to getting-started or public pack indexes until the production pack is releasable.

## Verification

Focused development commands:

```bash
cd engine
cargo test -p wax-lang-react
cargo clippy -p wax-lang-react --all-targets -- -D warnings
```

Before promoting React beyond draft status:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
