---
title: Troubleshooting
sidebar_position: 5
---

# Troubleshooting

Failure modes below map to the CLI command modules under
[`commands`](https://github.com/Daio-io/wax/tree/main/engine/crates/wax-cli/src/commands)
and the schemas under
[`wax-contract/schemas`](https://github.com/Daio-io/wax/tree/main/engine/crates/wax-contract/schemas).

## `wax init` needs a terminal

Interactive initialization requires a TTY. In scripts, provide one or more
languages explicitly:

```bash
wax init --non-interactive --language react
```

If scan reports no repository config in CI, commit `.wax/wax.config.json` and
`.wax/wax.lock.json`, then run `wax validate`.

## A language pack is missing

Local `wax scan` can install a missing locked pack automatically. CI should
install the exact `<id>@<version>` first, then scan with `--no-auto-install`:

```bash
wax language install react@<version>
wax language doctor
wax scan --no-auto-install
```

The pack index override is `--pack-index` or `WAX_PACK_INDEX`.

## Registry refresh or lookup fails

Run `wax registry show <id>` to inspect a remembered source and
`wax registry refresh <id>` to update its cache. `--offline` never fetches; an
`offline_cache_miss` means the required snapshot is not cached. Preserve the
search result's `snapshot.digest` to avoid `snapshot_mismatch` on exact detail.
Use `--language` or a more specific source when selection is ambiguous.

## A scan is partial

The merged artifact and diagnostics are still written when possible. Inspect
per-language status and typed diagnostics in `.wax/out/scan-merged.json`, repair
the reported parser, registry, or pack issue, then rerun. Add `--strict` when a
partial result must fail CI; omit it for exploratory best-effort scans.

Repository config and outputs live under `.wax/`. Downloaded packs, remembered
systems, and caches live under global `~/.wax/`; deleting one does not repair the
other. See the [CLI reference](../reference/cli.md) for recovery commands.
