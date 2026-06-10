# React Language Pack Design

## Summary

Wax should promote `wax-lang-react` from a scaffold to a production parser-backed language pack. The first production version focuses on the same normalized contract as Compose: load a design-system registry, scan configured source roots, discover local components, resolve design-system component usage sites, and emit deterministic `ScanFacts`.

React support must be more than uppercase JSX tag matching. The pack should parse JavaScript, TypeScript, JSX, and TSX with SWC, build enough module-resolution context to understand imports and aliases, and resolve JSX usage back to registry symbols before counting it as design-system adoption.

This design is **implemented**. See [React language pack ADR](../../adr/2026-06-07-react-language-pack.md) and the [archived implementation plan](./2026-06-07-react-language-pack-plan.md).

## Goals

- Make `wax-lang-react` a production parser-backed language pack.
- Preserve the existing engine and language-pack contract: React emits facts, the engine owns reports.
- Match Compose's core behavior for React projects: registry components, local components, usage sites, metrics, counts, and diagnostics.
- Use SWC for TS/JS/JSX/TSX parsing.
- Resolve design-system JSX usage through imports, exports, aliases, and configured package entrypoints.
- Keep scan output deterministic and CI-friendly.
- Make incomplete analysis visible through diagnostics and `Partial` status.
- Leave room for richer React analysis without changing the v1 `ScanFacts` contract.

## Non-Goals

- React v1 will not introduce a React-specific report schema.
- React v1 will not require a hosted service or external runtime process beyond the language pack binary.
- React v1 will not attempt full type checking with `tsc`.
- React v1 will not execute user code, run bundlers, or evaluate dynamic imports.
- React v1 will not model every wrapper, higher-order component, styled-component, or polymorphic component pattern.
- React v1 will not infer a design-system registry from consuming app usage.

## Product Decision

React v1 should target "usage plus locals" parity with Compose.

The pack should:

- load the configured Wax registry;
- scan configured roots for `.js`, `.jsx`, `.ts`, and `.tsx`;
- discover local React component declarations;
- find JSX usage sites;
- count a JSX usage as design-system usage only when it resolves to a registry symbol;
- emit design-system-relevant unresolved or unsupported cases as diagnostics instead of silently treating them as accurate facts.

Bare JSX names should not be enough to produce resolved design-system usage in v1. For example, `<Button />` should count as registry usage only when the local module graph shows that `Button` was imported or re-exported from a configured design-system source. This avoids false positives when local app components share design-system-like names.

## Configuration

React uses the existing language config shape:

```json
{
  "id": "react",
  "enabled": true,
  "registry": ".wax/wax.registry.json",
  "roots": ["apps/web/src"]
}
```

React v1 adds optional resolver configuration under the React language entry:

```json
{
  "id": "react",
  "enabled": true,
  "registry": ".wax/wax.registry.json",
  "roots": ["apps/web/src"],
  "ignore": [
    "**/node_modules/**",
    "**/*.d.ts",
    "**/*.stories.{js,jsx,ts,tsx}",
    "**/*.{spec,test}.{js,jsx,ts,tsx}"
  ],
  "tsconfig": "apps/web/tsconfig.json",
  "aliases": {
    "@/*": ["apps/web/src/*"]
  },
  "packages": {
    "@acme/design-system": {
      "exports": {
        ".": "packages/design-system/src/index.ts",
        "./*": "packages/design-system/src/*.ts"
      }
    }
  }
}
```

Field semantics:

- `roots`: repo-relative source roots or supported Wax root patterns.
- `registry`: existing design-system registry source.
- `ignore`: optional repo-relative glob patterns applied after root collection. React v1 also applies documented defaults for generated, declaration, story, and test files.
- `tsconfig`: optional repo-relative path used for `compilerOptions.paths` and `baseUrl`.
- `aliases`: explicit alias mappings when projects use bundler aliases that are not visible through `tsconfig`.
- `packages`: optional design-system package entrypoint hints. These map package imports to source modules so registry symbols can resolve through public package exports.

All paths must be repo-relative. Parent-directory escapes are invalid.

## Registry Matching

The registry remains the source of truth for canonical design-system symbols.

React v1 should accept the same simple registry component fields Compose already consumes:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.button",
      "symbol": "Button",
      "aliases": ["PrimaryButton"],
      "targets": ["react", "compose"]
    }
  ]
}
```

`targets` is an optional additive field for platform-specific registry availability. When omitted or null, the component is available to every language pack that can resolve the symbol. When present, it is an allow-list of language ids. A React-only component can therefore be modeled as:

```json
{
  "id": "ds.data-grid",
  "symbol": "DataGrid",
  "targets": ["react"]
}
```

Language packs must exclude registry components whose `targets` array is present and does not contain their language id. Excluded components do not appear in that language's `design_system_components` facts and do not contribute to that language's coverage denominator. This prevents a web-only component from being reported as unused by Compose, while keeping existing registries compatible because missing `targets` preserves the current all-languages behavior.

Matching rules:

- `symbol` is the canonical registry symbol.
- `aliases` are alternate imported or exported names that resolve to the canonical symbol.
- `targets` limits which language packs should consider the component available. Missing `targets` means all languages.
- A JSX usage resolved through an alias emits `symbol` as the observed source symbol and `registry_symbol` as the canonical registry symbol.
- Candidate and unresolved matches are diagnostics in v1 unless the current `ScanFacts` contract already has a clear usage status for that case.

## Architecture

`wax-lang-react` should mirror the Compose pack's public shape while using React-specific internals:

```text
ScanRequest
  -> parse React config
  -> load registry
  -> resolve roots
  -> collect JS/TS/JSX/TSX files
  -> parse modules with SWC
  -> build module/import/export index
  -> discover local components
  -> resolve JSX usage to registry symbols
  -> emit ScanFacts
```

Suggested internal modules:

- `config`: parse and validate React-specific scan config.
- `registry`: load registry symbols and aliases into a resolver-friendly index.
- `files`: collect supported source files under resolved roots.
- `swc_parse`: parse source files and return module ASTs with source locations.
- `module_graph`: index imports, exports, one-hop direct re-exports, aliases, and configured package entrypoints.
- `extract`: discover local components and JSX usage sites.
- `facts`: convert internal results into `ScanFacts`.

These boundaries keep the resolver and extractor testable without invoking the CLI or subprocess protocol.

## Local Component Discovery

React v1 should discover likely local React components conservatively:

- function declarations with PascalCase names that return JSX;
- const or let declarations with PascalCase names initialized to arrow/function expressions that return JSX;
- exported default functions or declarations when a stable component name can be derived from the declaration or filename;
- `React.forwardRef` and `memo` wrappers when the wrapped component name is direct and static.

React v1 should skip:

- lowercase declarations;
- declarations in `.d.ts` files;
- files matched by React's documented default or configured ignore patterns;
- components created only through dynamic factories that cannot be resolved statically.

Each local component emits a `LocalComponent` with a stable id, source symbol, and declaration location.

## Usage Extraction

React v1 should collect JSX opening elements:

- `<Button />`
- `<Button.Primary />` when the member expression can be resolved statically;
- fragments are ignored as component usages;
- lowercase intrinsic HTML elements are ignored for design-system usage.

The extractor should resolve the JSX tag through the module graph:

1. Find the binding in the current module.
2. Resolve imports and one-hop direct re-exports to a source module and exported symbol.
3. Apply configured aliases and package entrypoints.
4. Match the resolved symbol or alias against the registry index.
5. Emit a `UsageSite` only for resolved registry usage.

When resolution fails, the pack should emit a diagnostic only for design-system-relevant candidates: imports from configured design-system packages, package entrypoints listed in React config, or JSX names that match registry symbols or aliases but cannot be resolved. Ordinary local or third-party JSX components should not produce unresolved diagnostics. Unresolved JSX names must not inflate adoption counts.

React v1 supports direct one-hop re-exports such as `export { Button } from "./Button"` when resolving a configured design-system package entrypoint. Deeper barrel chains and workspace-wide re-export graphs belong to React v1.1.

## Error Semantics

React should separate fatal config errors from recoverable resolver gaps.

Fatal config errors return a wire error and no `ScanFacts`:

- malformed config value types;
- missing required `registry` or `roots` when scan config is present;
- absolute paths or parent-directory escapes;
- invalid `targets`, `aliases`, `packages`, or package export shapes;
- unreadable or malformed registry JSON.

Recoverable resolver gaps return `ScanFacts` with `Partial` status and diagnostics:

- configured roots missing or wildcard roots matching nothing;
- source file parse failures;
- configured package entrypoints that do not resolve to source files;
- design-system package imports that cannot be resolved with the available config;
- unsupported module syntax that skips an import/export edge while leaving the file otherwise usable.

## Diagnostics and Status

React scans should return `Complete` only when the configured roots and parsed files were processed without known gaps.

The scan should return `Partial` when any of these occur:

- configured root missing;
- wildcard root matched nothing;
- source file parse failure;
- missing resolver coverage that prevents configured design-system package imports from resolving;
- unsupported module syntax causes a file or import edge to be skipped.

Stable diagnostic codes should include:

- `root_not_found`
- `root_glob_not_found`
- `parse_failed`
- `ds_import_unresolved`
- `ds_export_unresolved`
- `package_entrypoint_unresolved`
- `unsupported_dynamic_import`
- `unsupported_jsx_member`

Diagnostics should be precise enough for users to fix configuration and source coverage.

## Data Flow

For a source file:

```tsx
import { Button as DsButton } from "@acme/design-system";

export function Screen() {
  return <DsButton variant="primary" />;
}
```

The pack should:

- discover `Screen` as a local component;
- resolve `DsButton` through the import to `@acme/design-system` export `Button`;
- match `Button` to the Wax registry;
- emit one resolved usage site at the JSX tag location;
- recompute counts and adoption coverage through the existing contract helpers.

## Testing

React v1 should have focused tests at each boundary:

- config parsing accepts valid repo-relative paths and rejects absolute or parent-escaping paths;
- registry loading supports canonical symbols and aliases;
- file collection finds `.js`, `.jsx`, `.ts`, and `.tsx` and skips unsupported files;
- SWC parsing handles TypeScript and JSX;
- local component discovery covers function declarations, arrow components, exports, default exports, and simple wrappers;
- usage extraction resolves named imports, default imports, aliased imports, relative imports, one-hop direct re-exports, and configured package entrypoints;
- unresolved design-system-relevant imports produce diagnostics and do not count as resolved usage;
- golden fixture scan emits stable `ScanFacts`.

## Documentation

Docs should explain:

- React support is parser-backed and registry-centered.
- Meaningful adoption metrics require a committed Wax registry.
- Import and alias configuration affects accuracy.
- React v1 counts resolved design-system JSX usage and discovers local components.
- Richer wrapper, prop, and dependency analysis is planned but not part of the first production React pack.
