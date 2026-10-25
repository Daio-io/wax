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

Publisher files normally live at `.wax/registries/<language-id>.json` and are
reviewed source. Consuming apps receive registry snapshots through `wax init`
and `wax sync`; global remembered sources and caches live under `~/.wax/`.

Search and exact detail have separate
[`registry-search.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry-search.schema.json),
[`registry-detail.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/registry-detail.schema.json),
and typed lookup-error schemas.
