# wax

[![Nice](https://api.nice.sbs/badge/n_c1qWdL8brn1s.svg)](https://nice.sbs/button?id=n_c1qWdL8brn1s)
[![Release](https://img.shields.io/github/v/release/Daio-io/wax?include_prereleases&label=release)](https://github.com/Daio-io/wax/releases)
[![CI](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml/badge.svg?branch=main)](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml)

Design system coverage analytics optimised for agents.

Wax is a CLI that registers the components in your design system, scans app
code for usage, and writes deterministic coverage data that humans and agents
can both work with.

Use it to answer questions like:

- Which design-system components are actually used?
- Where are teams still using local or hard-coded UI?
- Is design-system adoption improving over time?
- Can an agent inspect coverage without guessing from source code alone?

## Get Started

### Install Wax

Homebrew:

```bash
brew tap Daio-io/wax
brew install wax
```

npm:

```bash
npm install -g @waxhq/wax@alpha
```

Curl:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

Check the CLI is available:

```bash
wax --help
```

### Register Your Design System

Initialize Wax in the app repository and enable one or more languages:

```bash
wax init --non-interactive --language react
```

For multiple stacks:

```bash
wax init --non-interactive --language react --language compose
```

This creates `.wax/` files for config, language-pack locks, and per-language
registries.

Add components manually:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "package": "@acme/design-system"
    }
  ]
}
```

Or discover components from your design-system source:

```bash
wax discover --language react --root ../design-system/src --dry-run
wax discover --language react --root ../design-system/src --force
```

Review discovered entries before committing them.

### Scan Your App

Validate the setup:

```bash
wax validate
```

Run a scan:

```bash
wax scan
```

Wax writes results under `.wax/out/`, including `.wax/out/scan-merged.json`.

## CLI Usage

### Languages

Wax uses language packs so the core CLI can stay small and each ecosystem can
have its own analyzer.

Current first-party packs:

| Pack | Use for |
| --- | --- |
| `react` | React and JSX/TSX projects |
| `compose` | Jetpack Compose and Kotlin UI code |
| `swift` | SwiftUI projects |
| `basic` | Text fallback scans and smoke tests |

Install or inspect packs:

```bash
wax language install react
wax language list
wax language doctor
```

Update installed packs:

```bash
wax language update react
wax language update --all
```

### Registries

By default, each language reads:

```text
.wax/<language-id>.registry.json
```

You can also point a language at a hosted registry:

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

After changing registry content or sources, refresh locks:

```bash
wax language update --all
wax validate
```

### CI

Commit `.wax/wax.lock.json`. In CI, use committed locks and scan without
auto-installing packs:

```bash
wax validate
wax language install react
wax scan --no-auto-install
```

### Local Builds

```bash
cd engine
cargo build --release -p wax-cli
./target/release/wax --help
```

## Output

Scan output reports design-system coverage using deterministic JSON. The merged
scan file includes resolved design-system usages, local UI, candidate matches,
and unresolved invocations.

For the full output contract, see
[Adoption Metrics v2](docs/specs/2026-06-20-adoption-metrics-v2-design.md).

## More Docs

- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md)
- [Component tracker design](docs/specs/2026-05-13-component-tracker-design.md)
- [Implementation plans](docs/plans/README.md)
- [Architecture decision records](docs/adr/README.md)
- [Contributing](CONTRIBUTING.md)
