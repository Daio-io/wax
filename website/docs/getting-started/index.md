---
title: Get started
sidebar_position: 1
---

# Get started with Wax

Take a repository from setup to its first design-system scan:

1. [Install Wax](./installation.md).
2. Run `wax init` to create repository-local `.wax/` configuration. Language
   packs and caches live under global `~/.wax/` and are not part of that
   scaffold.
3. Run `wax validate` to check the configuration and language-pack lockfile.
4. Run `wax scan` to produce the first set of scan facts.

The quick-start commands are defined in
[`cli.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/cli.rs).

Next, learn how to [work with registries](../guides/registries.md), [search](../guides/search.md), [scan in CI](../guides/ci.md), use the [CLI reference](../reference/cli.md), or choose an [agent skill](../skills/index.md).
