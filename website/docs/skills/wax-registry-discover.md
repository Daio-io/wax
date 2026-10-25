---
title: wax-registry-discover
sidebar_position: 4
---

# wax-registry-discover

Use `wax-registry-discover` when a design-system publisher needs to discover,
refresh, enrich, or audit registry entries. It separates deterministic
discovery from reviewed authoring decisions.

Safety boundary: Keep existing-registry work preview-only until explicit approval, preserve unknown fields losslessly, and never author into an app-synced copy.

## Publisher and application roles

A publisher owns authoritative `.wax/registries/<language>.json` files and
`design_systems` config. An application consumes a remembered publisher through
`wax init`/`wax sync`. App-local `.wax/registries/<design-system>/` copies are
never the authoritative edit target.

## Prerequisites

- The publisher repository or an unambiguous remembered upstream.
- A resolved language and source root. Without Wax config, always pass
  `--root`; ask when more than one language remains plausible.
- The required language pack installed globally for configless discovery.

## Happy path

Preview every discovery first:

```bash
wax registry discover \
  --design-system acme \
  --name "Acme Design System" \
  --language react \
  --root src \
  --dry-run
```

Review a structured diff of component changes, proposed aliases/tags, token
additions, canonical values, and potential removals. Require explicit approval
for additions or changes and separate approval for removals. For a brand-new
registry only, rerun without `--dry-run` after approval. For an existing
registry, transfer only approved changes with a lossless patch so authored ids,
metadata, values, source associations, and extension fields survive.

After a publisher write:

```bash
wax validate
```

For a delegated app flow, return to the app and regenerate evidence:

```bash
wax sync
wax validate
wax scan
```

## Output

Discovery writes schema-version-1 publisher registries, publication config, and
remembered global source metadata. Component aliases/tags aid search only;
token aliases are exact source identifiers and require evidence. Approved
canonical values become deterministic scan inputs only after sync and a fresh
scan.

## Failures and guardrails

- Keep existing registries in `--dry-run` mode; `--force` is compatibility-only
  and does not authorize destructive replacement.
- Never delete components or tokens automatically, and never remove authored
  aliases, metadata, values, or unknown fields outside the approved diff.
- Ask about helper/demo/preview exports and ambiguous public components.
- Resolve the publisher before editing; stop rather than treat an app snapshot
  as source of truth.
- Registry authoring does not select an implementation. Use
  [`wax-search`](./wax-search.md) against the published snapshot for that task.

Read the canonical
[`skills/wax-registry-discover/SKILL.md`](https://github.com/Daio-io/wax/blob/main/skills/wax-registry-discover/SKILL.md),
the [token-value maintenance rules](https://github.com/Daio-io/wax/blob/main/skills/wax-registry-discover/token-value-maintenance.md),
and the [component enrichment example](https://github.com/Daio-io/wax/blob/main/skills/wax-registry-discover/examples/component-alias-enrichment.md)
for complete operational detail.
