# wax

[![Nice](https://api.nice.sbs/badge/n_c1qWdL8brn1s.svg)](https://nice.sbs/button?id=n_c1qWdL8brn1s)
[![Release](https://img.shields.io/github/v/release/Daio-io/wax?include_prereleases&label=release)](https://github.com/Daio-io/wax/releases)
[![CI](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml/badge.svg?branch=main)](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml)

`wax` is an open-source CLI for analyzing design-system usage in codebases.

It helps teams define a canonical component registry, scan repositories with
language-aware analyzers, and produce deterministic outputs that work locally
and in CI. Optional AI skills can help author registries and interpret scan
results, but the core runtime stays deterministic.

## Summary

Wax is built around a few repo-local files and installable language packs:

- `.wax/wax.config.json` enables languages and source roots.
- `.wax/wax.lock.json` pins language packs and registry digests.
- `.wax/<language-id>.registry.json` lists the design-system components to track.
- `.wax/out/scan-merged.json` contains the merged scan output.
- Language packs such as `compose`, `react`, `swift`, and `basic` are installed
  under `~/.wax/langs/`.

Wax reports design-system adoption from source code usage. Scan output includes
UI invocation adoption, registry resolution, and raw invocation counts for
resolved, local, candidate, and unresolved calls.

In practice, Wax helps you:

- bootstrap repo-local scan config with `wax init`;
- discover or maintain a registry of canonical design-system components;
- scan app code with parser-backed language packs;
- validate committed config and lockfiles in CI;
- hand deterministic JSON outputs to dashboards, reports, or agent workflows.

## Install

### Homebrew

```bash
brew tap Daio-io/wax
brew install wax
```

### npm

```bash
npm install -g @waxhq/wax@alpha
wax --help
```

Or run without a global install:

```bash
npx @waxhq/wax@alpha --help
```

### Curl

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

If the installer uses `~/.wax/bin`, add it to your shell path:

```bash
export PATH="$HOME/.wax/bin:$PATH"
```

Install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash -s -- --version 0.1.0-alpha.1
```

Verify the install:

```bash
wax --help
```

## Getting started

Initialize a repository with one or more language packs:

```bash
wax init --non-interactive --language compose
wax init --non-interactive --language compose --language react
```

For local setup, the interactive wizard can guide the same choices:

```bash
wax init
```

`wax init` writes the repo-local config, lockfile, and per-language registry
stubs under `.wax/`. The wizard asks which languages to enable, which source
roots to scan, and whether your registry source lives in the current repo.

Then validate the repo setup:

```bash
wax validate
```

Populate the registry manually:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "package": "com.acme.designsystem"
    }
  ]
}
```

Or discover registry entries from your design-system source:

```bash
wax discover --language <language-id> --root <design-system-source> --dry-run
wax discover --language <language-id> --root <design-system-source> --force
wax language update --all
wax validate
```

Review discovered entries before committing them; deterministic discovery can
include false positives.

Run a scan:

```bash
wax scan
```

Inspect outputs under `.wax/out/`, especially `.wax/out/scan-merged.json`.
For the full scan output contract, see
[Adoption Metrics v2](docs/specs/2026-06-20-adoption-metrics-v2-design.md).

## Usage

### Language packs

Wax uses installable language packs instead of baking every analyzer into the
core binary.

| Pack | Use for |
| --- | --- |
| `compose` | Jetpack Compose and Kotlin UI code |
| `react` | React and JSX/TSX projects |
| `swift` | SwiftUI projects |
| `basic` | Text-based fallback scans and smoke tests |

Install packs explicitly:

```bash
wax language install compose
wax language install react
wax language install swift
wax language install basic
```

List installed packs and check repo state:

```bash
wax language list
wax language doctor
```

Update installed packs:

```bash
wax language update compose
wax language update --all
```

### Registry workflow

The registry is the source of truth for the design-system components Wax should
track. By default, each language uses:

```text
.wax/<language-id>.registry.json
```

Typical workflow:

1. Run `wax init` with the languages you want to scan.
2. Add or discover components in each registry file.
3. Run `wax validate`.
4. Run `wax scan`.
5. After registry changes, run `wax language update --all` and commit the
   refreshed lockfile.

When the design system lives in a separate repository, point discovery at that
source tree and publish or copy the generated registry JSON into app repos:

```bash
wax language install react
wax discover --language react --root packages/components/src --dry-run
wax discover --language react --root packages/components/src --force
```

You can also point a language at a hosted registry source:

```json
{
  "schema_version": 1,
  "languages": [
    {
      "id": "react",
      "enabled": true,
      "registry": {
        "source": "https://example.com/acme-ds/v2.4.1/react.json"
      },
      "roots": ["src"]
    }
  ]
}
```

After changing a hosted registry source, run `wax language update --all` so the
lockfile pins the new digest.

### CI

Commit `.wax/wax.lock.json`. In CI, install or restore pinned language packs
before scanning without auto-install:

```bash
wax validate
wax language install compose
wax scan --no-auto-install
```

### Config notes

Preferred paths:

- config: `.wax/wax.config.json`
- lockfile: `.wax/wax.lock.json`
- per-language registry: `.wax/<language-id>.registry.json`

Legacy `.waxrc` is still supported when the preferred config file is absent.
For editor validation, add the published schema to `.wax/wax.config.json`:

```json
{
  "$schema": "https://raw.githubusercontent.com/Daio-io/wax/main/engine/crates/wax-contract/schemas/waxrc.schema.json"
}
```

### AI skills

Wax includes optional agent skills under [skills](skills):

- `wax-registry-discover` helps preview, review, and write registry entries.
- `wax-scan` runs validation and scan analytics, with optional HTML reports.

These skills call the deterministic `wax` CLI; they do not replace it.

## Build locally

```bash
cd engine
cargo build --release -p wax-cli
./target/release/wax --help
```

## Uninstall

Remove the binary and Wax global state:

```bash
wax uninstall --full
```

Remove a language pack:

```bash
wax language uninstall compose
wax language uninstall compose --version 0.1.0
```

## Contributing

Contributor workflow, verification commands, repo layout, and release/process
notes live in [CONTRIBUTING.md](CONTRIBUTING.md).

## More docs

- [Adoption Metrics v2](docs/specs/2026-06-20-adoption-metrics-v2-design.md)
- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md)
- [Component tracker design](docs/specs/2026-05-13-component-tracker-design.md)
- [Implementation plans](docs/plans/README.md)
- [Architecture decision records](docs/adr/README.md)
