---
title: Registry
sidebar_position: 2
---

# Registry reference

Registry documents use `schema_version: 1`. The authoritative
[`registry.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry.schema.json)
permits unknown extension fields so publishers can preserve richer metadata.

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.button",
      "symbol": "Button",
      "package": "@acme/design-system"
    }
  ],
  "tokens": [
    {
      "id": "color.primary",
      "key": "theme.colors.primary",
      "category": "color",
      "aliases": ["tokens.color.primary"],
      "value": "#3366ff"
    }
  ]
}
```

Components carry stable ids and symbols, with optional package/import identity,
targets, metadata, source associations, signatures, best practices,
deprecation, attachments, and typed detail gaps. Tokens carry stable ids and
keys plus optional aliases, category, canonical value, metadata, and details.
Known token categories are `color`, `spacing`, `typography`, `radius`,
`elevation`, and `unknown`.

### Token package identity

Set a token's optional `package` to require import evidence. For React, use
the npm package root and write the key as the exported binding and member path.
For example, this entry matches `b.blue9` after
`import { blue as b } from '@radix-ui/colors'`. It also matches
`palette.blue.blue9` after a namespace import from the same package.

```json
{
  "id": "radix.blue.9",
  "package": "@radix-ui/colors",
  "key": "blue.blue9",
  "category": "color"
}
```

For Compose, use the Kotlin package. The scanner recognizes named imports,
import aliases, package-qualified paths, and a single wildcard import.
For Swift, use the Swift module. The scanner recognizes visible module
qualifiers, selective imports, and a single module import. Basic scans can
only match unscoped tokens.

Token IDs are unique across the registry. Different packages can register the
same key or alias under distinct IDs. Leave `package` out to match an exact
source expression. When a name is registered with a package, matching that name
requires compatible origin evidence.

### React palette tokens
`wax registry discover` for React also reads public palette declarations: a module-level `const`
whose initializer is an object literal of `#rgb`, `#rgba`, `#rrggbb`,
`#rrggbbaa`, or `color(display-p3 r g b)` (optionally `/ alpha`) string values,
exported through the package's public entry points. Each property becomes a
package-scoped `color` token such as `token:@radix-ui/colors#blue.blue9`, with
source evidence. Objects with spreads, computed keys, methods, shorthand, or
duplicate keys, and values outside those literal forms, are skipped. Rediscovery
refreshes a value only while it still equals the generated literal recorded in
`details.discovered_literal_value`; authored or cleared values are preserved.
A registry with only `tokens` (no `components`) is a valid scan input.

Publisher files normally live at `.wax/registries/<language-id>.json` and are
reviewed source. Consuming apps receive registry snapshots through `wax init`
and `wax sync`; global remembered sources and caches live under `~/.wax/`.

Search and exact detail have separate
[`registry-search.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry-search.schema.json),
[`registry-detail.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry-detail.schema.json),
and typed lookup-error schemas.

## Public catalogue discovery and installation

The built-in catalogue is the `main` branch of
[`wax-registries`](https://github.com/Daio-io/wax-registries), indexed by
`catalog/index.json`. No setup is needed:

```bash
wax registry find
wax registry find astryx --language react --format json
wax registry find --offline --format json
wax registry install astryx --version 1
wax registry install astryx --version 1 --offline
```

`find [query]` matches ids and names by case-insensitive substring. An empty
query lists all compatible systems. Results include public, configured
publisher/app, and remembered systems. Same-name rows retain their origin.
In an interactive terminal, text results show each name above its origin, ID,
revision when present, and languages. An available public revision includes a
copyable `wax registry install <id> --version <revision>` command. The
`--language` filter limits discovery. Each install includes the whole revision.
Offline find adds `--offline` to the suggested commands, which require cached
bundle objects as well as cached catalogue metadata.

Redirected streams, CI, and dumb terminals use tab-separated rows followed by
public status and any diagnostic. Use `--format json` for structured output.
JSON uses `schema_version: 1`, stable `(origin, id, revision)` ordering, and a
separate `public_status` object (`complete`, `cached`, `commit`, `diagnostic`).
The [find response schema](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry-catalog-find.schema.json)
defines this envelope. Local unversioned rows have a null revision and catalogue
commit. Cache freshness timestamps do not appear in result identity.

Every online find refreshes the index. Public Git, index, or compatibility
failure returns local results with `complete: false` and a structured
`public_unavailable` diagnostic, exiting successfully. Invalid local config or
global state is fatal. Warm offline find reads the selected cached index and
reports `cached: true`; cold offline find returns local rows with
`offline_cache_miss` and `complete: false`. Interactive text reports
`complete`, `complete (cached)`, or `incomplete`, with the full catalogue commit
and diagnostic when available.
Ordinary search and scan commands perform no public discovery traffic.

Installation requires an explicit positive bundle revision (`--version`),
installs **all** declared languages, and accepts no `--language` flag. The
selected index, definition, provenance, and registries are verified at one
immutable catalogue commit. If the selected index cannot supply the requested
revision, online installation refreshes it before selecting the commit.
Offline installation requires a cached index and Git bundle objects. Installs
write only global state/cache under `~/.wax/` (or `WAX_HOME`); they do not edit
app config, registries, packs, or lockfiles.

The remembered slug is `public-astryx-r1`. Repeating identical historical data
retains the original commit. A different source/artifact or an occupied local
slug fails without overwriting it. Installing revision 2 creates a separate
`public-astryx-r2` entry; app adoption remains explicit.

```bash
wax registry list
wax registry show public-astryx-r1
wax registry refresh public-astryx-r1
```

List/show expose bundle commit, revision, and languages. Refresh verifies or
refills the original revision and never selects a newer revision.

## App setup and explicit revision selection

`wax init --non-interactive --language <id> --design-system <remembered-id>`
uses the same remembered-system setup as the interactive menu. Requested
languages must be present before app files are written. Registry installation
never scaffolds an app or installs packs; init installs packs independently
unless `--no-install` is set.

`wax sync --design-system <remembered-id>` replaces only bindings to that
bundle's system in the same repository, requiring coverage for every bound
language. It verifies all selected digests and pins before writing, updates
config/lock together, restores original config bytes on lock-write failure,
and shows old/new revision directories and commits. It conflicts with
`--upgrade`. Other app systems keep their bindings.

Generated bundle config records Git repository, full commit `tag`, relative
`subdir` and `system`. Lock schema 3 adds definition and registry digest pins.
No global paths or remembered ids are required to replay the committed pins.
Init/sync warm the app-local cache; validation reads committed metadata only.
Ordinary sync/scan, refresh and global installation never select a newer
public revision. See the [offline/CI recipe](../guides/registries.md#offline-and-ci)
and [coverage/provenance policy](../guides/registries.md#public-coverage-and-provenance).
