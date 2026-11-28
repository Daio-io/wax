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
`elevation`, `aspect_ratio`, and `unknown`. Registries also accept `ratio`
and `aspect-ratio` as input spellings for `aspect_ratio`.

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

### Hard-coded corner radii

Wax reports these literals with category and context `radius`. A matching
canonical radius token value produces an exact migration candidate.

- **React** detects literal values in JSX inline styles for `borderRadius`,
  `borderTopLeftRadius`, `borderTopRightRadius`, `borderBottomLeftRadius`,
  `borderBottomRightRadius`, `borderStartStartRadius`, `borderStartEndRadius`,
  `borderEndStartRadius`, and `borderEndEndRadius`. It also detects Tailwind
  arbitrary radius values such as `rounded-[6px]`.
- **Compose** detects radius literals in `clip` and `cornerRadius` calls.
  `RoundedCornerShape`, `CutCornerShape`, `AbsoluteRoundedCornerShape`,
  `AbsoluteCutCornerShape`, and `CornerSize` report one site per direct
  argument with a radius literal, located at the literal. Nested calls such
  as `Modifier.clip(RoundedCornerShape(4.dp))` report the shape literal once.
  Compose interprets `RoundedCornerShape(50)` as a percentage. Wax records
  the bare integer `50` and applies its usual numeric matching rules.
- **SwiftUI** detects literals in `cornerRadius` and `clipShape` calls and
  labelled radius arguments on `RoundedRectangle`, `UnevenRoundedRectangle`,
  and `.rect(...)`. Recognized labels include `cornerRadius`, `radius`,
  `topLeadingRadius`, `topTrailingRadius`, `bottomLeadingRadius`, and
  `bottomTrailingRadius`. `.shadow(radius:)` is an elevation observation.

### CSS custom properties

React scans count literal `var(--name)` references in `.css` files under the
scan roots (respecting `ignore`) when the registry has an unscoped token whose
key is exactly `var(--name)`. A logical token can link ordered concrete
variants (light, dark, Display P3) by ID with `css_variants`. The logical entry
needs an unscoped `var(--name)` key and no `value`; each linked token must be a
same-category entry with a nonblank value. A reference counts once for the
logical token and never selects a mode, gamut, or value. Lookup returns the
links, and each linked ID can be retrieved from the same snapshot.

```json
{
  "tokens": [
    {
      "id": "css.blue-9",
      "key": "var(--blue-9)",
      "category": "color",
      "css_variants": ["blue.blue9", "blueDark.blue9", "blueP3.blue9", "blueDarkP3.blue9"]
    },
    {"id": "blue.blue9", "key": "blue.blue9", "category": "color", "package": "@radix-ui/colors", "value": "#0090ff"},
    {"id": "blueDark.blue9", "key": "blueDark.blue9", "category": "color", "package": "@radix-ui/colors", "value": "#0090ff"},
    {"id": "blueP3.blue9", "key": "blueP3.blue9", "category": "color", "package": "@radix-ui/colors", "value": "color(display-p3 0.247 0.556 0.969)"},
    {"id": "blueDarkP3.blue9", "key": "blueDarkP3.blue9", "category": "color", "package": "@radix-ui/colors", "value": "color(display-p3 0.247 0.556 0.969)"}
  ]
}
```

Only literal references in declaration values are counted. Property
definitions, comments, strings, selectors, at-rule preludes, CSS imports, the
cascade, CSS-in-JS and hard-coded CSS values are out of scope. A malformed
stylesheet reports a `parse_failed` gap and keeps references from its clean
declarations. Discovery preserves authored `css_variants` groups and their
targets.

### Tailwind v4 theme tokens

React discovery reads top-level `@theme` blocks in CSS under a discovery root,
such as `--root src/styling`. For example, `--color-main: var(--main)` becomes
`css.color-main` with key `var(--color-main)`, category `color`, and source
evidence at the declaration. Discovery resolves whole-value references against
top-level `:root` properties and then the theme declarations.

| Theme namespace | Registry category |
| --- | --- |
| `--color-` | `color` |
| `--spacing-`, `--spacing`, `--container-` | `spacing` |
| `--radius-` | `radius` |
| `--shadow-`, `--inset-shadow-`, `--drop-shadow-`, `--text-shadow-` | `elevation` |
| `--text-`, `--font-`, `--font-weight-`, `--tracking-`, `--leading-` | `typography` |
| `--aspect-` | `aspect_ratio` |
| `--blur-`, `--perspective-`, `--ease-`, `--animate-`, `--breakpoint-` | `unknown` |

Out of scope are `.dark` variants, Tailwind v3 config, and deriving the
`--spacing` scale from the base spacing token.

#### Tailwind utility classes

React scans count token references in `className` and `class` attributes and in
calls to `cn`, `clsx`, `classnames`, `classNames`, `cx`, `twMerge`,
`twJoin`, `cva`, and `tv`. Scans read static class text in strings and
templates. Within helper calls, `cn`, `clsx`, `classnames`, `classNames`,
and `cx` read class names from object keys and ignore the condition values,
while `cva` and `tv` read class names from configuration values. Scans count
only classes that appear whole in source, as Tailwind does: string
concatenation such as `"bg-" + tone` or `"bg-main" + "-hover"` is not
reconstructed, and fragments that touch another operand or interpolation
without whitespace are ignored.

| Utility examples | Theme namespaces in lookup order |
| --- | --- |
| `bg-`, `border-`, `ring-`, `fill-`, `from-` | `color` |
| `text-` | `text`, `color` |
| `shadow-`, `inset-shadow-`, `drop-shadow-`, `text-shadow-` | Matching shadow namespace, then `color` |
| `font-` | `font-weight`, `font` |
| `tracking-`, `leading-` | Matching namespace |
| `rounded-`, including side and corner utilities | `radius` |
| Padding, margin, gap, height, inset, translation, and related utilities | `spacing` |
| `w-`, `min-w-`, `max-w-` | `container`, `spacing` |
| `blur-`, `perspective-`, `aspect-`, `ease-`, `animate-` | Matching namespace |

For example, `hover:bg-main/50` references `var(--color-main)` when that token
is registered. Direct references such as `bg-[var(--chart-1)]` and
`bg-(color:--chart-1)` can match any registered unscoped CSS custom-property
token. A class can reference more than one token: `/` modifiers such as
`text-sm/(--body-leading)` or `bg-main/[var(--alpha)]` also count the
referenced property, and a named font-size modifier such as `text-sm/tight`
counts `var(--leading-tight)`. Default palette colors count only when
registered. An explicitly registered numeric token such as `--spacing-4` counts
for `p-4`, but deriving `p-4` from the base `--spacing` scale, fractions such
as `w-1/2`, and Tailwind v3 configuration are outside this scan's scope.

#### Token inference for Tailwind arbitrary values

React scans record literal arbitrary values such as `text-[10px]`,
`bg-[#fff]`, `p-[13px]`, `rounded-[6px]`, and `aspect-[4/3]` as hard-coded style candidates
even when the registry has no tokens. Wax compares them with canonical token
values using the same inference rules as inline React styles. Type hints such
as `color:` disambiguate the category, and unescaped underscores become spaces.

Wax skips values containing `var(`, `theme(`, `env(`, `calc(`, `url(`, or `--`
and utility/value combinations without a mapped style context, such as
`border-[3px]` or `grid-cols-[1fr_auto]`. Registered CSS variable references
still count as token references.

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
