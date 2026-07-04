# Registry Sync and Config v2 Design

## Context

Wax can already discover design-system registries, scan codebases with language packs, and pin registry digests in `.wax/wax.lock.json`. The rough edge is the handoff between a design-system repo and app repos: after discovering a registry, users still copy files manually and keep app configs in sync by hand.

Because Wax is still alpha and not in broad public use, this design intentionally makes a clean cut instead of preserving old config shapes.

## Goals

- Make the first local scan easy without requiring a committed config.
- Let `wax registry discover` remember design-system registries automatically.
- Let `wax init` configure an app from a remembered design system and write committed, CI-friendly files.
- Add `wax sync` so an app can refresh its registry inputs from remembered design-system upstreams before scanning.
- Reduce config scatter: keep repo metadata in `.wax/wax.config.json`, reproducibility pins in `.wax/wax.lock.json`, and registry JSON under `.wax/registries/`.
- Remove alpha-era compatibility paths that make the product harder to explain.

## Non-Goals

- No migration path from `.waxrc`, top-level `wax.lock.json`, or `design_system_registry`.
- No hidden global registry package store. Global state remembers design systems and repo locations; app repos still scan from explicit config sources or ephemeral prompt selections.
- No scan failure just because upstream sync fails. `wax scan` may try to refresh upstream registries, but failures warn and the scan continues with the current configured registry inputs.
- No hosted registry service. Hosted registry URLs are opaque sources declared by design-system config.

## File Model

Wax supports only these committed repo files:

```text
.wax/
  wax.config.json
  wax.lock.json
  registries/
    <design-system>/
      <language>.json
```

Design-system repos that publish registries may use:

```text
.wax/
  wax.config.json
  registries/
    <language>.json
```

Global state remains outside repos:

```text
~/.wax/state.json
```

Global state is local convenience state. It is not required for CI scans.

## Config v2

`.wax/wax.config.json` uses `schema_version: 2`.

Top-level sections:

- `languages`: codebase scan configuration.
- `design_systems`: registry publication configuration; this is the defining section for design-system configs.
- `engine`: optional engine settings such as scan concurrency.
- `adoption`: optional adoption metrics settings.

`languages` is an object keyed by language id, not an array of objects with repeated `id` fields.

### App Config

```json
{
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": ".wax/registries/acme/react.json",
        "upstream": "acme/react"
      }
    }
  }
}
```

Rules:

- `languages.<language>.roots` lists repo-relative scan roots.
- `languages.<language>.registry.source` is the registry Wax scans with right now.
- `registry.source` may be a repo-relative path, `file://` URL, `http://` URL, or `https://` URL.
- `languages.<language>.registry.upstream` is optional and means `<design-system-id>/<language-id>`.
- `registry.upstream` is used by `wax sync`; it is not needed for scan.
- `enabled` is removed. A language key exists only when the language is enabled.

### Design-System Publishing Config

```json
{
  "schema_version": 2,
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "registries": {
        "react": {
          "source": ".wax/registries/react.json",
          "published_source": "https://cdn.example.com/acme/react.registry.json"
        }
      }
    }
  }
}
```

Rules:

- `design_systems.<id>.name` is display text for prompts and lists.
- `design_systems.<id>.registries.<language>.source` is the design-system-authored registry artifact.
- `published_source` is optional. When present, `wax sync` prefers it for app configs.
- A design-system config is a config with `design_systems`.
- A design-system config may contain `design_systems` without `languages`.
- A repo may contain both `design_systems` and `languages` when it publishes registries and also scans itself.

## Lockfile

`.wax/wax.lock.json` remains the reproducibility file. It pins:

- selected language pack artifacts
- resolved registry source strings
- registry content digests

The lockfile path is always `.wax/wax.lock.json`. Top-level `wax.lock.json` is removed.

## Global State

`~/.wax/state.json` gains remembered design systems:

```json
{
  "installed_languages": {},
  "design_systems": {
    "acme": {
      "name": "Acme Design System",
      "repo_root": "/Users/dai/work/acme-ds",
      "last_seen_config": ".wax/wax.config.json"
    }
  }
}
```

Global state does not store registry JSON snapshots. It records enough to find a known design-system config for prompts, init, and sync.

## CLI Flows

### Discover and Remember a Design System

```bash
wax registry discover --design-system acme --name "Acme Design System" --language react --root src
```

Behavior:

1. Discover registry components from the supplied roots.
2. Write `.wax/registries/react.json` in the design-system repo.
3. Ensure `.wax/wax.config.json` has `design_systems.acme.registries.react.source`.
4. Store or refresh `acme` in `~/.wax/state.json`.
5. Print the app setup command:

```text
Apps can use this registry with `wax init` or refresh existing setups with `wax sync`.
```

### Local Scan Without Config

```bash
wax scan
```

If `.wax/wax.config.json` does not exist and stdin is a TTY, Wax prompts for:

- language
- scan roots
- remembered design-system registry

The scan uses those answers for that invocation only. It does not write `.wax/wax.config.json`, `.wax/wax.lock.json`, or committed registry snapshots under `.wax/registries/`.

Ephemeral scans may still use normal operational paths such as `.wax/cache/` for materialized registry inputs and `.wax/out/` for scan output.

At the end of an ephemeral scan, Wax prints:

```text
To save this setup for CI or teammates, run `wax init`.
```

If no config exists and stdin is not a TTY, Wax fails with a scriptable message telling the user to run `wax init` first.

### Commit App Setup

```bash
wax init
```

In a TTY, Wax prompts for language, scan roots, and a remembered design-system registry.

For a local design-system source, Wax copies the registry into:

```text
.wax/registries/<design-system>/<language>.json
```

Then it writes `.wax/wax.config.json` with `registry.source` pointing at the committed app-local copy and `registry.upstream` set to `<design-system>/<language>`.

If the design system declares `published_source`, Wax writes `registry.source` to that hosted URL instead of copying a local file.

`wax init --non-interactive` remains the scriptable path and requires explicit language, roots, and registry source arguments.

### Sync an App Before Scanning

```bash
wax sync
wax scan
```

`wax sync` operates on the current repo.

For each `languages.<language>.registry.upstream`:

1. Resolve `<design-system>/<language>` through `~/.wax/state.json`.
2. Read the design-system repo's `.wax/wax.config.json`.
3. Find `design_systems.<design-system>.registries.<language>`.
4. If `published_source` exists, update app `registry.source` to that value.
5. Otherwise copy the design-system registry `source` into the app-local `.wax/registries/<design-system>/<language>.json`.
6. Refresh `.wax/wax.lock.json`.

`wax scan` should attempt the same app sync as a best-effort preflight when upstream metadata exists. If sync succeeds, the scan uses the refreshed registry inputs and lockfile. If sync fails because the design-system repo is unavailable, global memory is stale, a hosted source cannot be reached, or a registry cannot be copied, Wax prints a warning and continues scanning with the current configured `registry.source`.

`wax sync` remains useful as an explicit command before scan when users want to refresh inputs intentionally or see sync failures separately. When a scan falls back to current inputs after a failed best-effort sync, it should include a short hint:

```text
warning: registry sync failed for acme/react; scanning with current registry source. Run `wax sync` for details.
```

### Manage Remembered Design Systems

```bash
wax registry list
wax registry show acme
wax registry update acme --repo-root ../acme-ds
wax registry delete acme
```

`wax registry update acme --repo-root ...` updates Wax's local memory for that design system. It does not modify app repos.

`wax registry delete acme` removes the design system from local memory only. It does not edit any repo config.

## CLI Naming Cleanup

The word `registry` should refer to design-system registries. Language-pack install indexes should use `pack index` in user-facing flags and messages.

Rename user-facing flags such as:

```text
--registry -> --pack-index
WAX_LANG_INDEX -> WAX_PACK_INDEX
```

Internal Rust names can change where they improve clarity, but the user-facing cleanup is the requirement.

## Error Handling

- Missing app config in non-TTY scan fails and suggests `wax init`.
- Missing global memory for `registry.upstream` makes `wax sync` ask for a design-system repo in TTY mode or fail in non-TTY mode. During `wax scan`, the same condition warns and scan continues with the current configured registry source.
- Missing design-system registry entry fails with the exact upstream id, such as `acme/react`.
- Unsupported registry source schemes fail during init, sync, validate, and scan.
- Lockfile digest drift remains a validation and scan failure.

## Testing

Focused tests should cover:

- v2 config parsing for `languages` object and `design_systems`.
- rejection of old `.waxrc`, top-level lockfile, and `design_system_registry` assumptions by removal of those code paths.
- `wax registry discover --design-system` writes DS publication config and updates global memory.
- `wax init` copies a remembered local registry into an app and writes `registry.upstream`.
- `wax sync` copies local registry updates into the app.
- `wax sync` switches app `registry.source` to `published_source`.
- `wax scan` attempts best-effort sync for upstream registries and continues with a warning when sync fails.
- `wax scan` without config prompts in TTY mode and does not write files.
- non-TTY `wax scan` without config fails with a `wax init` message.
- `wax registry list/show/update/delete` manage global memory only.

## Verification

Because this is a cross-cutting alpha cutover, each implementation task should run focused tests for the touched crates. The final task should run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
