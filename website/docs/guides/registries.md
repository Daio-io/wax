---
title: Registries
sidebar_position: 1
---

# Work with registries

Registries connect design-system publishers to consuming applications. A
publisher authors schema-version-1 registry files; an application pins reviewed
snapshots in repository-local `.wax/` config and lock files. Remembered sources
and downloaded snapshots live in global `~/.wax/` state.

## Publisher: discover, review, publish

Start with a dry run so discovery cannot write a file:

```bash
wax registry discover --language react --root src --dry-run
```

Review the JSON on stdout and the added/updated summary on stderr. Then write a
losslessly merged registry and remember the design system:

```bash
wax registry discover \
  --design-system acme \
  --name "Acme Design System" \
  --language react \
  --root src
```

Rediscovery preserves authored ids, metadata, tokens, and extension fields.
Review every change before committing or publishing it. The standalone
`wax discover` command is a compatibility alias for `wax registry discover`.

Existing JSON can be remembered from a local path, HTTP URL, or Git source:

```bash
wax registry add acme ./dist/react.registry.json --language react
wax registry add acme-git git+https://github.com/acme/design-system.git --tag v2.4.1
wax registry refresh acme
wax registry list
wax registry show acme
```

## Application: initialize and refresh

Run [`wax init`](../getting-started/initialize.md) to select a remembered system
and create app-local registry inputs. Later, refresh those inputs explicitly:

```bash
wax sync
wax validate
wax scan
```

Root Git sources remain pinned to the locked commit by default. Use
`wax sync --upgrade` to advance their tag and review the resulting lock change.

### Git bundles in subdirectories

An app can select a bundle published from a repository subdirectory:

```json
{
  "git": "https://github.com/acme/registries.git",
  "tag": "v1",
  "subdir": "systems/acme/1",
  "system": "acme"
}
```

Use this object as `languages.react.registry` in `.wax/wax.config.json`.
Wax reads `design_systems.acme` from
`systems/acme/1/.wax/wax.config.json` and verifies every declared language
registry at one immutable commit. Each registry must use its conventional
relative path, `.wax/registries/<language>.json`; published URL overrides are
ignored for bundles. `system` requires `subdir`. Omitting `system` reads only
the single conventional registry under that subdirectory.

Nested pins remain immutable during `sync --upgrade`; select a different
configured revision directory/ref or use the explicit revision-adoption command
below to change bundles. `wax validate` checks
committed config/lock metadata offline, without global state or Git cache.

Registry fields are defined by the
[registry schema](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry.schema.json),
and command behavior comes from
[`commands/registry.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/commands/registry.rs).

## Explicit public revision adoption

```bash
wax registry find shadcn --format json
wax registry install shadcn --version 1
wax init --non-interactive --language react --design-system public-shadcn-r1
wax search Button --language react --format json
wax component <id> --language react --snapshot <digest> --offline
wax scan --no-auto-install
wax validate
# When revision 2 is published and reviewed:
wax registry install shadcn --version 2
wax sync --design-system public-shadcn-r2
```

`--design-system` on sync selects a remembered Git bundle of the same system
and repository already bound by the app. It covers **all** languages bound to
that system, verifies definition, commit and registry digests before writes,
and persists config/lock together using rollback on failure. Other systems
are unchanged. Output shows old/new revision directories and commits.
`--upgrade` and `--design-system` cannot be combined.

Catalogue refreshes, installing another global revision, ordinary scans/sync,
and nested `sync --upgrade` never migrate the app to a recommendation. Full
commit pins are retained; missing or corrupt exact bytes fail without falling
back to a newer revision. Old installed revisions and snapshot-pinned detail
remain usable after adoption.

## Offline and CI

Init/sync populate `.wax/cache/registries/`. Warm app scans and
`wax search Button --offline` use those exact bytes without remembered registry
state. A clean checkout can fetch its locked Git commit directly, without
catalogue `main` or an installed remembered entry. `wax validate` only checks
committed metadata, so it works with empty `WAX_HOME` and no network/cache.

Preinstall the lockfile's language-pack versions in CI, then run
`wax validate` and `wax scan --no-auto-install`. `--no-install` on init skips
pack downloads; installing a registry alone never installs packs or creates
app files. Exclude `.wax/cache/` from git; a CI cache may retain it for warmed
runs. For configless scans use an installed id or exact public selector:

```bash
wax scan --design-system public-shadcn-r1 --language react --root src --no-auto-install
wax scan --design-system public:shadcn@1 --language react --root src --no-auto-install
```

These create no app config/lock. Existing `--offline` requires warmed exact
sources; cold caches report a miss rather than discovering a latest version.

## Public coverage and provenance

Use `wax registry find --format json` for the available revisions, supported
languages, declared coverage and gaps. A registry's provenance records what
was inspected; source-commit coverage does not prove those bytes shipped in a
package release. Public metadata is not upstream endorsement. Imported
same-name symbols from unrelated packages do not count as design-system use;
unknown provenance remains candidate. Unlisted tokens remain absent.

The Phase 4 pilot matrix is:

| Pilot | Documented matching boundary | Verification status |
| --- | --- | --- |
| Radix primitives / React | Canonical named imports; short aliases and unrelated `Root` collisions must not falsely resolve | Catalogue pilot pending |
| Radix Themes / React | Named imports from the declared Themes package | Catalogue pilot pending |
| shadcn / React | Exact `@/components/ui/<file>` alias; alternative local aliases are outside coverage | Pinned 353-component fixture verified |
| Astryx / React | Named imports from the registry's declared package | Catalogue pilot pending |
| Astryx / Swift | Declared module imports; experimental source-commit coverage | Catalogue pilot pending |
| Material M2 / Compose | M2 package, distinct from M3 | Catalogue pilot pending |
| Material M3 / Compose | M3 package, distinct from M2 | Catalogue pilot pending |

At catalogue commit `761243349f9a74a9a70dc20a8877b430241bcae7`, only shadcn is
published. This matrix records remaining verification gates, not availability
of the other pilots. Namespace and unsupported alias conventions must not be
assumed supported; parser behavior is unchanged by bundle setup.

Corrections should publish a new positive revision and retain old revision
paths, commits and index rows. Users explicitly select an older retained
revision to roll back. Report registry coverage/provenance issues and propose
reviewed changes in [wax-registries](https://github.com/Daio-io/wax-registries);
engine/parser changes follow [Wax contribution guidance](https://github.com/Daio-io/wax/blob/main/CONTRIBUTING.md).
