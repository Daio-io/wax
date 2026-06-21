# Changelog

## Unreleased

### Breaking

- **Adoption Metrics v2** — `scan-merged.json` and per-language scan facts use `schema_version: 2`. Legacy `adoption_coverage_ratio` and flat v1 count fields are replaced by grouped `counts` (`raw_invocations`, `definitions`, `adoption`, `registry`, `parent_scopes`), `metrics.invocation_adoption_ratio`, `metrics.registry_resolution_ratio`, and merged `symbol_usage_summary[]`. Parser-backed packs emit local and unresolved UI invocations with optional parent-scope attribution. The `wax-scan` insights extractor requires v2 scan input.

### Release

- Promote `wax-lang-react` into alpha release artifacts and generated pack indexes alongside `compose` and `basic` (16 archives + checksums per tag). The default `gh-pages/index.json` updates when the next alpha tag publishes.

### wax-cli

- Show TTY progress spinners on stderr for `wax scan`, `wax validate`, and `wax language install`; suppressed when stderr is piped (CI and scripts).

### wax-core

- Add shared `registry_lock::verify_registry_lock` used by validate and scan.
- `validate_repo` reports `RegistrySourceDrift` when a locked registry source no longer matches config (aligned with scan).
- Remove unused pre-registry `ValidateError` variants (`MissingDesignSystemRegistry`, `InvalidDesignSystemRegistryPath`, `RegistryPathEscapesRepo`).

## 0.1.0-alpha.1

- Align publishable workspace crates on the `0.1.0-alpha.1` prerelease version.
- Ensure generated `wax.lock.json` files record the matching engine version.
- Publish the optional npm wrapper as `@waxhq/wax`, with alpha installs using the `alpha` dist-tag.
