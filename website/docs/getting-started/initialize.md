---
title: Initialize a repository
sidebar_position: 3
---

# Initialize a repository

From the repository root, use the guided setup in an interactive terminal:

```bash
wax init
```

For CI or scripts, select at least one language explicitly. Repeat
`--language` for a mixed stack:

```bash
wax init --non-interactive --language react
wax init --non-interactive --language react --language compose
```

Initialization writes repository-local `.wax/wax.config.json`,
`.wax/wax.lock.json`, default registry scaffolds at
`.wax/<language-id>.registry.json`, and the appropriate generated-output
entries in `.gitignore`. Commit the config, lockfile, and reviewed registries.
Generated `.wax/out/` files and downloaded global `~/.wax/` state do not belong
in the repository. Remembered design-system copies and publisher layouts use
`.wax/registries/` separately from this default scaffold path.

`wax init` configures the repository. It does not run registry discovery or a
scan. Check the result, then scan:

```bash
wax validate
wax scan
```

For every option, see [`wax init` in the CLI reference](../reference/cli.md#wax-init).
The implementation is in
[`commands/init.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/commands/init.rs).

## Initialize from an exact public revision

Check available systems with `wax registry find --format json`, then install a
positive, exact bundle revision. For example, for a published shadcn revision:

```bash
wax registry install shadcn --version 1
wax init --non-interactive --language react --design-system public-shadcn-r1
wax search Button --language react --format json
wax scan --no-auto-install
wax validate
```

Interactive setup offers the same remembered-system choice. The explicit flag
requires `--non-interactive`; select every desired language with `--language`.
A language missing from the bundle fails before app config/lock writes.
Registry installation only remembers verified artifacts. Init separately
installs language packs unless `--no-install` is set.

Bundle configs use repository Git pins with full commits and relative revision
paths, with definition/registry digests in the lockfile. Init warms the app
cache for later scans and offline search. Commit config and lock, exclude
`.wax/cache/` and `.wax/out/`. Clean checkouts validate without global state or
network; first scans can fetch the exact locked commit. In CI preinstall the
locked packs, then use `wax scan --no-auto-install`.

To adopt another installed revision of the same system, use
`wax sync --design-system <remembered-id>`. Every bound language switches
together. See [registry adoption](../guides/registries.md#explicit-public-revision-adoption).
