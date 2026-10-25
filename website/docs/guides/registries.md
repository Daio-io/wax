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

Git tags remain pinned to the locked commit by default. Use `wax sync --upgrade`
only when you intend to advance a tag and review the resulting lock change.

Registry fields are defined by the
[registry schema](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry.schema.json),
and command behavior comes from
[`commands/registry.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/commands/registry.rs).
