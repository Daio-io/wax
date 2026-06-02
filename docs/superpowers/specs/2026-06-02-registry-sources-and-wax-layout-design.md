# Registry Sources and Centralized Wax Layout Design

## Summary

Wax should support design-system registry definitions that live outside the scanned repository while keeping local, committed registries simple. The new default repository layout centralizes wax-owned files under `.wax/` and makes the registry source optional in config:

```text
.wax/
  wax.config.json
  wax.lock.json
  wax.registry.json
  cache/
  out/
```

When a language does not declare a registry, wax uses `.wax/wax.registry.json`. Repositories can override that with a repo-relative path or a hosted/local source object. Remote and outside-repo sources are checked during validation and scan, and their resolved content is locked by digest for deterministic CI.

## Goals

- Keep the common path local and low-config.
- Let app repositories consume registry definitions published by a design-system repository.
- Let design-system teams encode versioning in their source URL or path instead of making wax infer package-manager semantics.
- Centralize wax-owned repository files while preserving searchable `wax.*.json` filenames.
- Keep scans deterministic with lockfile-pinned registry content.
- Preserve compatibility with existing `.waxrc`, `wax.lock.json`, and `design_system_registry` users during a migration window.

## Non-Goals

- Wax will not infer registry versions from Gradle, npm, Maven, or other dependency manifests in this design.
- Wax will not define a hosted registry service or package protocol.
- Wax will not support plain absolute filesystem paths in config; outside-repo local files must use `file://`.
- Language packs will not fetch remote registries directly.

## Repository Layout

New `wax init` writes the canonical layout:

```text
.wax/
  wax.config.json
  wax.lock.json
  wax.registry.json
  cache/
  out/
```

`wax init` also updates `.gitignore` with:

```gitignore
/.wax/cache/
/.wax/out/
```

The registry, config, and lockfile are intended to be committed when they are local to the repository. Materialized remote registry files and generated scan output remain ignored under `.wax/cache/` and `.wax/out/`.

## Configuration Shape

The canonical config path is `.wax/wax.config.json`. Its schema remains close to the current `.waxrc` shape:

```json
{
  "schema_version": 1,
  "engine": {
    "scan_concurrency": 2
  },
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "roots": ["app/src/main/kotlin"]
    }
  ]
}
```

Missing `registry` means the enabled language uses `.wax/wax.registry.json`.

A string `registry` is a repo-relative path:

```json
{
  "id": "compose",
  "enabled": true,
  "registry": ".wax/compose.registry.json",
  "roots": ["app/src/main/kotlin"]
}
```

An object `registry` declares a source:

```json
{
  "id": "compose",
  "enabled": true,
  "registry": {
    "source": "https://example.com/acme-ds/registry/v2.4.1/compose.json"
  },
  "roots": ["app/src/main/kotlin"]
}
```

`registry.source` supports:

- repo-relative paths
- `file://` URLs
- `http://` URLs
- `https://` URLs

Plain absolute paths are rejected. A user who wants to reference a sibling checkout or another absolute local path must use a `file://` URL so the repository escape is explicit.

The existing per-language `design_system_registry` key remains a deprecated alias for repo-relative local registry paths during a compatibility window.

## Registry Versioning

Wax treats registry sources as opaque addresses. If the design-system implementer wants a versioned registry, they should encode that version in the source URL or path, for example:

```json
{
  "registry": {
    "source": "https://example.com/acme-ds/registry/v2.4.1/compose.json"
  }
}
```

or:

```json
{
  "registry": {
    "source": "file:///Users/example/acme-ds/releases/v2.4.1/compose.json"
  }
}
```

Wax does not attempt to decide whether a branch, tag, path, or URL is stable. It validates the fetched registry content and relies on `.wax/wax.lock.json` to detect drift.

The registry file may later grow optional component availability fields such as `since` and `until`, but this design does not require those fields for source resolution.

## Resolution and Data Flow

Before `wax validate` or `wax scan` runs language-pack validation or scanning, wax resolves every enabled language's registry:

1. Read `.wax/wax.config.json`, or fall back to legacy `.waxrc`.
2. Normalize each enabled language's registry setting:
   - missing `registry` -> repo-relative `.wax/wax.registry.json`
   - string `registry` -> repo-relative path
   - object `registry.source` -> repo-relative path, `file://`, `http://`, or `https://`
   - legacy `design_system_registry` -> repo-relative path
3. Read or fetch the registry content.
4. Parse it as registry JSON and validate the supported schema version and required shape.
5. Materialize any `file://`, `http://`, or `https://` registry content under `.wax/cache/registries/<language-id>-<sha256>.json`.
6. Rewrite the language-pack config so `registry` resolves to a repo-relative local path before spawning the pack.
7. Run the language pack using only the resolved local registry path.

Language packs should continue to scan from local inputs. Remote fetching, digest checks, lockfile policy, and compatibility handling stay in `wax-core`.

## Lockfile Behavior

The canonical lockfile path is `.wax/wax.lock.json`. It continues to lock language-pack artifacts and gains registry source locks per enabled language.

Each registry lock entry records:

- language id
- normalized source string
- SHA-256 digest of the exact registry content used for the lock

`wax scan` rejects registry drift when the current resolved registry digest differs from the lockfile digest. This applies to hosted sources, `file://` sources, and repo-relative sources. Local repo-relative registries remain editable source files, but the lockfile gives CI a deterministic check that the committed registry and lock agree.

`wax validate` checks that enabled languages have registry lock entries and reports missing or mismatched locks with precise field paths. A future update command can refresh these locks intentionally.

## Compatibility and Precedence

Wax reads old and new file locations during a migration window:

- preferred config: `.wax/wax.config.json`
- legacy config: `.waxrc`
- preferred lockfile: `.wax/wax.lock.json`
- legacy lockfile: top-level `wax.lock.json`

If both old and new config files exist, `.wax/wax.config.json` wins and validation emits a warning about the ignored legacy config. If both old and new lockfiles exist, `.wax/wax.lock.json` wins and validation emits a warning about the ignored legacy lockfile.

New `wax init` writes only the centralized layout. Existing repositories can keep using legacy files until they migrate.

## Errors and Warnings

Validation and scan should fail early for:

- unsupported registry source schemes
- plain absolute paths in `registry` or `registry.source`
- missing registry files
- failed HTTP fetches
- malformed registry JSON
- unsupported registry schema versions
- lockfile registry digest drift
- missing lock entries for enabled languages

Validation should warn for:

- legacy config or lockfile ignored because a new file exists
- deprecated `design_system_registry` usage
- empty local registry components, matching current behavior

## Testing

Focused tests should cover:

- missing `registry` defaults to `.wax/wax.registry.json`
- string `registry` resolves as a repo-relative path
- object `registry.source` resolves repo-relative paths, `file://`, `http://`, and `https://`
- plain absolute paths are rejected
- malformed, missing, or unsupported registry files fail validation
- registry digest match allows scan
- registry digest drift rejects scan
- old and new config precedence emits warnings
- old and new lockfile precedence emits warnings
- `design_system_registry` remains accepted with a deprecation warning
- `wax init` scaffolds `.wax/wax.config.json`, `.wax/wax.lock.json`, `.wax/wax.registry.json`
- `wax init` adds `/.wax/cache/` and `/.wax/out/` to `.gitignore` without duplicating them
- language packs receive a resolved repo-relative registry path and do not fetch remote sources
