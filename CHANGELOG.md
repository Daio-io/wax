# Changelog

## Unreleased

### Breaking

- **Source-boundary attribution / scan schema v4** — `SourceLocation` now
  carries optional `boundary_id`; merged scans expose deterministic
  `source_boundaries` metadata and per-boundary/language summaries with raw
  status, origin, and invocation-adoption counters. Configure explicit
  application/module/feature/source-root globs under
  `reporting.source_boundaries`; one-language and multi-language repositories
  may filter boundaries with `languages`. Existing configs without reporting
  boundaries retain their prior behavior.

For one language, omit the filter: `{ "id": "app/mobile", "include": ["app/**"] }`.
For multiple languages, set it explicitly, for example
`{ "id": "web/checkout", "languages": ["react"], "include": ["web/checkout/**"] }`.

- **Adoption Metrics v2 / scan facts v4** — `scan-merged.json` and per-language scan facts use `schema_version: 4`. Legacy `adoption_coverage_ratio` and flat v1 count fields are replaced by grouped `counts` (`raw_invocations`, `definitions`, `adoption`, `registry`, `parent_scopes`, `invocation_origins`), `metrics.invocation_adoption_ratio`, `metrics.registry_resolution_ratio`, and merged `symbol_usage_summary[]`. Every `usage_sites[]` row now carries `callee_origin` and `resolution_evidence`, including observed package/module evidence for mismatches. Parser-backed packs emit local and unresolved UI invocations with optional parent-scope attribution. The `wax-scan` insights extractor requires v4 scan input.

### Features

- Add `wax scan --strict` for CI: partial or failed language results return a
  nonzero exit after writing the merged scan artifact and human-readable
  summary. The default `wax scan` behavior remains best effort.
- Add config-v2 Git/tag registry sources using the conventional
  `.wax/registries/<language-id>.json` path.
- Materialize Git registries at locked commits with registry SHA-256 digests;
  add `wax sync --upgrade` for deliberate tag refreshes.
- Make Git-mode `wax validate` offline by checking committed config and lock
  metadata without remote or cache access.

### Release

- Promote `wax-lang-react` into alpha release artifacts and generated pack indexes alongside `compose` and `basic` (16 archives + checksums per tag). The default `gh-pages/index.json` updates when the next alpha tag publishes.

### wax-cli

- Show TTY progress spinners on stderr for `wax scan`, `wax validate`, and `wax language install`; suppressed when stderr is piped (CI and scripts).

### wax-lang-compose

- Stop treating `when` arm bodies of the form `-> if (cond) …` as when-guards. The false-positive mask was introducing `parse_failed` diagnostics on valid Kotlin.

### wax-core

- Raise the default language-pack scan timeout from 120s to 10 minutes and honor `WAX_SCAN_TIMEOUT_SECS`, matching the language-pack distribution spec.
- Add shared `registry_lock::verify_registry_lock` used by validate and scan.
- `validate_repo` reports `RegistrySourceDrift` when a locked registry source no longer matches config (aligned with scan).
- Remove unused pre-registry `ValidateError` variants (`MissingDesignSystemRegistry`, `InvalidDesignSystemRegistryPath`, `RegistryPathEscapesRepo`).

## 0.1.0-alpha.1

- Align publishable workspace crates on the `0.1.0-alpha.1` prerelease version.
- Ensure generated `wax.lock.json` files record the matching engine version.
- Publish the optional npm wrapper as `@waxhq/wax`, with alpha installs using the `alpha` dist-tag.
