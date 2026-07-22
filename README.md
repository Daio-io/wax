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
`key` and optional `aliases` exactly in source. Add an optional `value` with the
canonical source-facing representation to let Wax infer exact and near matches
against hard-coded styling:

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
      "aliases": ["tokens.color.primary"],
      "value": "#3366ff"
    },
    {
      "id": "space.medium",
      "key": "theme.space.medium",
      "category": "spacing",
      "value": "8px"
    }
  ]
}
```

Registries without `tokens` (or with an empty array) stay valid. Component-only
coverage still works; token facts are additive. A token without `value` remains
valid too. Same-category tokens with usable values are assessed independently;
a missing or unsupported sibling does not block matching. An observation is
`unassessed` when no same-category token has a usable value or its observed
format cannot be normalized. Inspect typed `evidence` before proposing a
registry change. See the reviewed [registry-maintenance workflow](skills/wax-registry-discover/SKILL.md).

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

The terminal summary includes token metrics for every scan:

```text
token metrics:
  Token references: 12
  Assessed observations: 6 of 10
  Confirmed migration candidates: 3
  Possible migration candidates: 1
  Unmatched observations: 2 (informational)
  Unassessed observations: 4 (comparison unavailable)
```

Every hard-coded styling observation from a parser-backed pack (`react`,
`compose`, `swift`) gets exactly one deterministic classification:

- **exact** — the normalized observed value matches a registry token's
  normalized canonical `value`; reported as a confirmed migration candidate.
- **near** — the observed value is numerically close to a canonical value,
  within `token_inference.numeric_tolerance`; reported as a possible migration
  candidate.
- **unmatched** — at least one same-category token has a usable canonical value,
  but none match closely enough; reported as informational evidence, not debt. A
  fixed dimension such as `width: 200px` stays visible here without counting
  against adoption.
- **unassessed** — Wax cannot complete the comparison, for example because no
  same-category token has a usable canonical value or the observed format cannot
  be normalized. Inspect the row's typed `evidence` before deciding whether
  registry maintenance is needed.

There is no combined debt, health, or compliance score. Exact, near, unmatched,
and unassessed counts stay separate on purpose. Raw hard-coded observations are
inventory, not debt; interpret them through the assessed subset. The retired
`token_reference_ratio` metric no longer appears in scan output.

Control near-match sensitivity with `token_inference.numeric_tolerance` in
`.wax/wax.config.json`:

```json
{
  "schema_version": 2,
  "token_inference": {
    "numeric_tolerance": 2
  }
}
```

The default tolerance is `2`; a value of `0` disables near matching while exact,
unmatched, and unassessed classifications remain. The `basic` pack matches token
references only and never emits hard-coded observations or inference rows.

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
`category`, optional `aliases`, and an optional `value` — the canonical
source-facing representation Wax compares against hard-coded styling:

```json
{
  "id": "space.medium",
  "key": "theme.space.medium",
  "category": "spacing",
  "value": "8px"
}
```

Supported categories: `color`, `spacing`, `typography`, `radius`, `elevation`,
and `unknown`.

Scan output adds token facts alongside component usage:

- `design_system_tokens[]` — configured registry tokens, each optionally
  carrying a canonical `value`
- `token_sites[]` — exact matches for token keys or aliases in source
- `hardcoded_style_sites[]` — every hard-coded styling observation from
  parser-backed packs, with typed usage context (padding, gap, width,
  height, radius, color, and so on)
- `token_inference` — one deterministic `exact` / `near` / `unmatched` /
  `unassessed` row per hard-coded observation, with confidence, suggested
  replacement tokens, and typed evidence

Findings map to a simple framing:

- **confirmed** (`exact`) and **possible** (`near`) rows are migration
  candidates.
- **informational** (`unmatched`) rows are visible facts, not debt — a fixed
  dimension without a matching canonical value is not treated as a token
  violation.
- **unassessed** rows mean Wax could not complete the comparison. Inspect their
  typed `evidence`: no usable same-category canonical `value` can be repaired
  through reviewed registry maintenance, while unsupported observed formats
  need different handling. Missing sibling values do not block assessment when
  another same-category token has a usable value.

Tune near-match sensitivity in `.wax/wax.config.json`:

```json
{
  "schema_version": 2,
  "token_inference": {
    "numeric_tolerance": 2
  }
}
```

`numeric_tolerance` defaults to `2` and accepts any finite non-negative number;
`0` disables near matching while preserving the other classifications. Near
matching applies only to compatible numeric scalar values in the same language
and category. Compose keeps `dp` and `sp` distinct, React uses CSS pixel
semantics only for numeric length properties, and Swift layout values remain in
their native scalar space. Wax never converts incompatible units or
environment-dependent units such as `rem` to `px`. Increasing the tolerance can
produce more possible migration candidates; colors, shadows, and composite
typography values always require an exact normalized match.

Token registry discovery and value maintenance are not automated end-to-end.
Author or sync `tokens[]` explicitly. When an unassessed row reports missing or
incomplete canonical-value evidence, use the
[`wax-registry-discover` skill](skills/wax-registry-discover/SKILL.md) to propose
reviewed canonical values with source evidence and an explicit approval step
before any registry write. After canonical values are approved, run `wax sync`
and `wax scan` again to reclassify the affected observations.

### CI

Commit `.wax/wax.lock.json`. In CI, use committed locks and scan without
auto-installing packs:

```bash
wax validate
wax language install react
wax scan --no-auto-install
```

### Local Builds

See [CONTRIBUTING.md](CONTRIBUTING.md#local-development) for the pinned Rust
toolchain prerequisite and how to confirm it is active.

```bash
cd engine
cargo build --release -p wax-cli
./target/release/wax --help
```

## Output

Scan output reports design-system coverage using deterministic JSON. The merged
scan file includes resolved design-system usages, local UI, candidate matches,
unresolved invocations, and additive token facts: references, raw hard-coded
observations with usage context, and a deterministic `token_inference` report
(exact, near, unmatched, and unassessed classifications with confidence and
suggested replacements).

For the full output contract, see
[Adoption Metrics v2](docs/specs/2026-06-20-adoption-metrics-v2-design.md).
For raw token facts, see
[Token scanning](docs/specs/2026-07-03-token-scanning-design.md). For
inference classifications, confidence, and reviewed registry maintenance, see
[Token inference and reporting](docs/specs/2026-07-19-token-inference-reporting-design.md).

## More Docs

- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md)
- [Component tracker design](docs/specs/2026-05-13-component-tracker-design.md)
- [Token scanning](docs/specs/2026-07-03-token-scanning-design.md)
- [Token inference and reporting](docs/specs/2026-07-19-token-inference-reporting-design.md)
- [Implementation plans](docs/plans/README.md)
- [Architecture decision records](docs/adr/README.md)
- [Contributing](CONTRIBUTING.md)
