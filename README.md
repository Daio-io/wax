<p align="center">
  <img src="./docs/assets/wax-full-logo.png" alt="wax logo" width="320" />
</p>

# wax

[![Nice](https://api.nice.sbs/badge/n_c1qWdL8brn1s.svg?theme=rich)](https://nice.sbs/button?id=n_c1qWdL8brn1s)
[![Release](https://img.shields.io/github/v/release/Daio-io/wax?include_prereleases&label=release)](https://github.com/Daio-io/wax/releases)
[![CI](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml/badge.svg?branch=main)](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml)

Design system coverage analytics optimised for agents.

Wax is a CLI that registers the components in your design system, scans app
code for usage, and writes deterministic coverage data that humans and agents
can both work with.

Use it to answer questions like:

- Which design-system components are actually used?
- Where are teams still using local or hard-coded UI?
- Which design tokens show up in source, and where is styling still hard-coded?
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

Discover and remember a design-system registry:

```bash
wax registry discover --design-system acme --name "Acme Design System" --language react --root src
```

Initialize an app from the remembered design system:

```bash
wax init
```

For scripts and CI, use the non-interactive path:

```bash
wax init --non-interactive --language react
```

For multiple stacks:

```bash
wax init --non-interactive --language react --language compose
```

This creates `.wax/wax.config.json`, `.wax/wax.lock.json`, and app-local registry
files under `.wax/registries/`.

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
wax registry discover --language react --root ../design-system/src --dry-run
wax registry discover --language react --root ../design-system/src --force
```

Review discovered entries before committing them.

Add design tokens to the same per-language registry files. Wax matches each token's
`key` and optional `aliases` exactly in source:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "package": "@acme/design-system"
    }
  ],
  "tokens": [
    {
      "id": "color.primary",
      "key": "theme.colors.primary",
      "category": "color",
      "aliases": ["tokens.color.primary"]
    },
    {
      "id": "space.medium",
      "key": "theme.space.medium",
      "category": "spacing"
    }
  ]
}
```

Registries without `tokens` (or with an empty array) stay valid. Component-only
coverage still works; token facts are additive.

### Scan Your App

Validate the setup:

```bash
wax validate
```

Refresh app registry inputs from remembered design systems:

```bash
wax sync
```

Run a scan:

```bash
wax scan
```

Wax writes results under `.wax/out/`, including `.wax/out/scan-merged.json`.

The terminal summary includes token metrics when registry tokens are configured:

```text
token metrics:
  Token reference ratio: 75.0%
  Token references: 12
  Hard-coded style candidates: 4
```

The token reference ratio is factual, not a compliance score: it compares known
token references to hard-coded styling candidates. Parser-backed packs (`react`,
`compose`, `swift`) can emit hard-coded styling candidates in styling contexts.
The `basic` pack matches token references only.

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
  "schema_version": 2,
  "languages": {
    "react": {
      "roots": ["src"],
      "registry": {
        "source": "https://example.com/acme-ds/v2.4.1/react.json"
      }
    }
  }
}
```

After changing registry content or sources, refresh locks:

```bash
wax sync
wax validate
```

### Tokens

Token definitions live in each language registry (for example
`.wax/react.registry.json`). Each entry needs an `id`, exact source `key`,
`category`, and optional `aliases`.

Supported categories: `color`, `spacing`, `typography`, `radius`, `elevation`,
and `unknown`.

Scan output adds token facts alongside component usage:

- `design_system_tokens[]` — configured registry tokens
- `token_sites[]` — exact matches for token keys or aliases in source
- `hardcoded_style_sites[]` — conservative hard-coded styling candidates from
  parser-backed packs

Token registry discovery is not automated yet. Author or sync `tokens[]`
explicitly, then run `wax sync` and `wax scan` as usual.

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
unresolved invocations, and additive token facts (references and hard-coded
styling candidates).

For the full output contract, see
[Adoption Metrics v2](docs/specs/2026-06-20-adoption-metrics-v2-design.md).
For token facts and CLI metrics, see
[Token scanning](docs/specs/2026-07-03-token-scanning-design.md).

## More Docs

- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md)
- [Component tracker design](docs/specs/2026-05-13-component-tracker-design.md)
- [Token scanning](docs/specs/2026-07-03-token-scanning-design.md)
- [Implementation plans](docs/plans/README.md)
- [Architecture decision records](docs/adr/README.md)
- [Contributing](CONTRIBUTING.md)
