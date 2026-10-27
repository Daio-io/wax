---
title: CLI reference
sidebar_position: 4
---

# CLI reference

This page mirrors the public Clap surface in
[`cli.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/cli.rs).
Run `wax --help` or `wax <command> --help` for the installed version. Commands
write their primary result to stdout and diagnostics/progress to stderr so JSON
output remains scriptable. Invalid arguments and operational failures return a
nonzero exit status.

```text
wax
├── init
├── registry {discover,add,refresh,list,show,update,delete}
├── discover
├── search
├── component
├── token
├── scan
├── sync
├── validate
├── language {list,install,uninstall,update,doctor}
└── uninstall
```

## `wax init`

Create `.wax/wax.config.json`, `.wax/wax.lock.json`, registry scaffolds, and
generated-output ignore rules.

```text
wax init [--non-interactive] [--language ID]... [--no-install]
         [--pack-index URL] [--repo-root PATH] [--target TRIPLE]
         [--no-scaffold-registries]
```

`--repo-root` defaults to `.`. Interactive mode requires a TTY. Non-interactive
mode requires at least one repeatable `--language`. `--no-install` writes config
and lock data without downloading packs. Pack-index precedence is
`--pack-index`, then `WAX_PACK_INDEX`, then the built-in index. Initialization
does not discover registries or scan. See the [setup guide](../getting-started/initialize.md).

## `wax registry`

Manage publisher discovery and globally remembered registry sources.

### `wax registry discover`

```text
wax registry discover --language ID [--root PATH]... [--dry-run] [--force]
  [--design-system ID] [--name NAME] [--repo-root PATH]
```

`--language` is required; `--root` is repeatable; `--repo-root` defaults to `.`.
`--dry-run` prints merged registry JSON without writing. `--force` is retained
for compatibility; existing registries are refreshed by lossless merge.

### `wax registry add`

```text
wax registry add <design-system-id> <source>
  [--name NAME] [--language ID] [--tag TAG_OR_COMMIT]
```

`source` may be a local path, HTTP URL, or Git source. The display name defaults
to the id. Use `--language` when JSON/source context cannot identify it and
`--tag` for a Git selector.

### `wax registry refresh`

```text
wax registry refresh <design-system-id>
```

Refetch a remembered source and update its selected cached snapshots.

### `wax registry list`

```text
wax registry list
```

Print all remembered design systems.

### `wax registry show`

```text
wax registry show <design-system-id>
```

Print one remembered system and its source details.

### `wax registry update`

```text
wax registry update <design-system-id> --repo-root PATH
```

Change the remembered repository root for a design system.

### `wax registry delete`

```text
wax registry delete <design-system-id>
```

Delete one remembered design system from global state.

## `wax discover`

`wax discover` is the compatibility alias for `wax registry discover` and
accepts the same arguments. New scripts should prefer the namespaced command.

## `wax search`

Fuzzy-search registry components and tokens.

```text
wax search <query> [--design-system ID_OR_SOURCE] [--language ID]
  [--offline] [--format text|json] [--tag TAG_OR_COMMIT]
  [--repo-root PATH] [--kind component|token] [--limit N] [--compact]
```

The output `--format` is `text` (default) or `json`. `--kind` accepts exactly
`component` or `token`; the result limit has default: 20. `--repo-root`
defaults to `.`. `--design-system` selects a remembered id, publisher id, or
explicit path/URL. Add `--tag` for explicit Git selectors and `--offline` to
forbid fetching. JSON failures use the typed lookup-error envelope. See the
[search guide](../guides/search.md).

`--compact` emits a terse text shortlist retaining the snapshot-pinned handoff
fields. It conflicts with an explicit `--format`; default text and JSON output
remain unchanged.

## `wax component`

Fetch one exact component id, optionally pinned to the immutable digest returned
by search.

```text
wax component <id> [--snapshot DIGEST] [--design-system ID_OR_SOURCE]
  [--language ID] [--offline] [--format text|json]
  [--tag TAG_OR_COMMIT] [--repo-root PATH]
```

Text is the default stdout mode; JSON emits one schema-valid detail envelope.

## `wax token`

Fetch one exact token id. It accepts the same common selectors and
`--snapshot DIGEST` behavior as `wax component`.

```text
wax token <id> [--snapshot DIGEST] [--design-system ID_OR_SOURCE]
  [--language ID] [--offline] [--format text|json]
  [--tag TAG_OR_COMMIT] [--repo-root PATH]
```

## `wax scan`

Run all enabled language packs and write merged scan facts.

```text
wax scan [--repo-root PATH] [--strict] [--no-auto-install]
  [--concurrency N] [--root-group ID]
  [--format summary|quiet|json-summary|markdown]
  [--compact]
  [--output FORMAT=PATH]... [--allow-absolute-output]
  [--baseline PATH]
```

`--repo-root` defaults to `.`; concurrency must be at least one. Stdout
`--format` accepts exactly `summary` (default), `quiet`, `json-summary`, or
`markdown`. `--output FORMAT=PATH` is repeatable and unions with config
`outputs[]`; artifact formats are `json-summary`, `graph-data`, `markdown`, and
`html`. Absolute artifact paths require `--allow-absolute-output`.

`--compact` emits one agent-oriented text record with the merged path, ordered
language statuses, headline counts, failures, artifacts, and ephemeral marker.
It conflicts with an explicit `--format`; use the merged JSON artifact for full
analytics.

The default is best effort. `--strict` returns nonzero when a language is
partial or failed, after artifacts and the summary are written.
`--no-auto-install` fails rather than download a missing pack. `--baseline`
accepts a prior JSON summary or merged scan. See the [scan guide](../guides/scan.md).

## `wax sync`

```text
wax sync [--repo-root PATH] [--upgrade]
```

Refresh app registry inputs from remembered systems. `--repo-root` defaults to
`.`. Git sources reuse locked commits unless `--upgrade` advances selected tags.

## `wax validate`

```text
wax validate [--repo-root PATH]
```

Validate repository-local config, lock pins, registries, and output settings.
The root defaults to `.`. Validation is CI-friendly and does not require
pre-existing global install state.

## `wax language`

Manage global language-pack installations.

### `wax language list`

```text
wax language list [--pack-index URL]
```

List installed packs. `--pack-index`/`WAX_PACK_INDEX` is a deprecated
compatibility flag for this installed-state listing and is ignored.

### `wax language install`

```text
wax language install <id>@<version> [--pack-index URL] [--target TRIPLE]
```

The version suffix is optional; without it, Wax installs the latest indexed
version. Pack-index precedence is flag, `WAX_PACK_INDEX`, then built-in default.

### `wax language uninstall`

```text
wax language uninstall <language-id> [--version VERSION]
```

Remove one version, or every installed version of the language when
`--version` is omitted.

### `wax language update`

```text
wax language update <language-id> [--pack-index URL] [--target TRIPLE]
  [--repo-root PATH]
wax language update --all [--pack-index URL] [--target TRIPLE]
  [--repo-root PATH]
```

Provide one language id or `--all`; the selectors conflict. Update installs the
latest indexed version and removes older local versions. The repository root
defaults to `.` so lock/config references can be checked.

### `wax language doctor`

```text
wax language doctor [--repo-root PATH]
```

Check repository language configuration, lock pins, and installed binaries.
The root defaults to `.`.

## `wax uninstall`

```text
wax uninstall --full
```

`--full` is required. Without it, `wax uninstall` errors with
`` `wax uninstall` requires `--full` `` and makes no changes. With `--full`, the
command removes the complete `~/.wax` global directory and best-effort binary
install paths together. Repository-local `.wax/` directories are not global
uninstall state.

Implementation details and command-specific errors are in the
[`commands` modules](https://github.com/Daio-io/wax/tree/main/engine/crates/wax-cli/src/commands).
