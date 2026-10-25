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
`.wax/wax.lock.json`, registry scaffolds under `.wax/registries/`, and the
appropriate generated-output entries in `.gitignore`. Commit the config,
lockfile, and reviewed registries. Generated `.wax/out/` files and downloaded
global `~/.wax/` state do not belong in the repository.

`wax init` configures the repository. It does not run registry discovery or a
scan. Check the result, then scan:

```bash
wax validate
wax scan
```

For every option, see [`wax init` in the CLI reference](../reference/cli.md#wax-init).
The implementation is in
[`commands/init.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/commands/init.rs).
