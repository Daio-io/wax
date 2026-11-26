---
title: Configuration
sidebar_position: 1
---

# Configuration reference

The canonical repository config is `.wax/wax.config.json`. It uses
`schema_version: 2` and is validated by
[`waxrc.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/waxrc.schema.json).
Unknown top-level fields are rejected.

```json
{
  "schema_version": 2,
  "engine": {"scan_concurrency": 4},
  "languages": {
    "react": {
      "roots": {"web": ["src"]},
      "registry": ".wax/react.registry.json"
    }
  },
  "outputs": [
    {"format": "json-summary", "path": ".wax/out/summary.json"}
  ]
}
```

- `engine.scan_concurrency` is an integer of at least one.
- `token_inference.numeric_tolerance` controls near numeric token matches; zero
  disables near matching.
- `adoption` controls facts-first invocation reporting, candidate policy, parent
  attribution, and symbol-summary limits.
- `languages` is keyed by language id. Each entry can use a roots array or
  named root groups, plus a local/HTTP source, an upstream remembered source,
  or a Git source with a tag. Git sources accept optional `subdir` and
  `system` fields for [publisher bundles](../guides/registries.md#git-bundles-in-subdirectories).
- `design_systems` is keyed publication configuration with per-language
  registry sources and optional published sources.
- `outputs` accepts `json-summary`, `graph-data`, `markdown`, or `html` with a
  repository-relative path.

`.wax/wax.lock.json` pins pack artifacts and registry snapshots separately from
this configuration. Writers use lock schema 3 with optional `subdir` and
`definition` pins; readers still accept lock schema 1 and 2. Config remains
schema 2. Both files are repository-local. Installed packs and registry
caches under `~/.wax/` are global and must not be committed.

## React resolution hints

Set these optional keys under `languages.react`:

- `tsconfig` is a repository-relative path to the TypeScript config. Wax
  reads `compilerOptions.baseUrl` and `compilerOptions.paths` to resolve imports.
- `aliases` maps import specifiers or single-`*` patterns to ordered arrays of
  repository-relative targets. Wax tries these aliases before TypeScript paths.
- `packages` maps package names to `exports` maps. Each export maps an
  entry point such as `.` or `./*` to a repository-relative file or pattern.
  Set the package key to the `package` value in the discovered registry, which
  comes from the library's `package.json` `name`.

For copied-in components under `src/components/ui`, configure the registry's
package name and point its exports at the copied source:

```json
{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": ".wax/react.registry.json",
      "tsconfig": "tsconfig.json",
      "aliases": {"ui/*": ["src/components/ui/*"]},
      "packages": {
        "neobrutalism-components": {
          "exports": {"./*": "src/components/ui/*"}
        }
      }
    }
  }
}
```

Imports that resolve into the exported files use the configured package name
for registry matching. Relative imports, TypeScript paths, and aliases all
work. Keep the copied source under the scan roots so Wax can resolve imports
and report parse errors there. Owned files emit no usage sites, local component
definitions, token sites, or hard-coded style sites.

An exact target owns one resolved file. Wildcard targets also match paths with
the file extension or trailing `/index.<ext>` removed. If packages overlap, the
longest target prefix wins. Wax warns on ties and uses the first package name in
lexicographic order.

## Public bundle handoff

`wax init --non-interactive --language react --design-system public-shadcn-r1`
writes a Git registry object shaped as follows (replace the placeholder with
the installed full commit):

```json
{
  "git": "https://github.com/Daio-io/wax-registries.git",
  "tag": "<full installed commit>",
  "subdir": "systems/shadcn/releases/4.23.0",
  "system": "shadcn"
}
```

Each enabled language's lock records the same bundle commit/definition and its
own registry SHA-256. There are no global absolute registry paths. Init and
sync materialize verified inputs under ignored `.wax/cache/registries/`;
committed metadata alone is enough for offline `wax validate`. A clean
checkout fetches only the exact commit when registry bytes are needed.

`wax sync --design-system <remembered-id>` deliberately adopts another bundle
of the same system/repository across all bound languages. Ordinary operations
and nested `sync --upgrade` retain the current selection.
