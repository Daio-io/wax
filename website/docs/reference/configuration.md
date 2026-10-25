---
title: Configuration
sidebar_position: 1
---

# Configuration reference

The canonical repository config is `.wax/wax.config.json`. It uses
`schema_version: 2` and is validated by
[`waxrc.schema.json`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/waxrc.schema.json).
Unknown top-level fields are rejected.

```json
{
  "schema_version": 2,
  "engine": {"scan_concurrency": 4},
  "languages": {
    "react": {
      "roots": {"web": ["src"]},
      "registry": ".wax/registries/react.json"
    }
  },
  "outputs": [
    {"format": "json-summary", "path": ".wax/out/summary.json"}
  ]
}
```

- `engine.scan_concurrency` is an integer of at least one.
- `token_inference.numeric_tolerance` controls near numeric token matches; zero
  disables near matching.
- `adoption` controls facts-first invocation reporting, candidate policy, parent
  attribution, and symbol-summary limits.
- `languages` is keyed by language id. Each entry can use a roots array or
  named root groups, plus a local/HTTP source, an upstream remembered source,
  or a Git source with a tag.
- `design_systems` is keyed publication configuration with per-language
  registry sources and optional published sources.
- `outputs` accepts `json-summary`, `graph-data`, `markdown`, or `html` with a
  repository-relative path.

`.wax/wax.lock.json` pins pack artifacts and registry snapshots separately from
this configuration. Both are repository-local. Installed packs and registry
caches under `~/.wax/` are global and must not be committed.
