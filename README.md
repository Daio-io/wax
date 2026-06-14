# wax

[![Nice](https://api.nice.sbs/badge/n_c1qWdL8brn1s.svg)](https://nice.sbs/button?id=n_c1qWdL8brn1s)
[![Release](https://img.shields.io/github/v/release/Daio-io/wax?include_prereleases&label=release)](https://github.com/Daio-io/wax/releases)
[![CI](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml/badge.svg?branch=main)](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml)

`wax` is an open-source CLI for analyzing design-system usage in codebases.

It helps teams define a canonical component registry, scan repositories with language-aware extractors, and produce deterministic outputs that work locally and in CI. Optional AI skills can help author and maintain registry files, but the core `wax` runtime stays deterministic.

## What wax does

- Tracks usage of canonical design-system components from a registry file.
- Scans source code with installable language packs such as `compose`, `react`, `swift`, and `basic`.
- Writes repo-local config, lock, and output files under `.wax/`.
- Supports deterministic validation and CI-safe scanning.
- Can bootstrap and refresh registries with `wax discover` (`wax registry discover` remains supported).
- Can be paired with optional agent skills for AI-assisted registry authoring and review.

## How it fits together

- `wax` binary: the engine and CLI you install and run.
- Language packs: stack-specific analyzers installed globally under `~/.wax/langs/`.
- Registry: your canonical design-system component list, usually `.wax/wax.registry.json`.
- Repo config: `.wax/wax.config.json` enables languages and points at roots and registry sources.
- Lockfile: `.wax/wax.lock.json` pins language-pack artifacts and registry digests for reproducible scans.
- AI skills: optional authoring workflows that call `wax` commands for tasks like registry discovery.

## Install

### Homebrew (macOS)

```bash
brew tap Daio-io/wax
brew install wax
```

### npm wrapper

```bash
npm install -g @waxhq/wax@alpha
wax --help
```

Or run it without a global install:

```bash
npx @waxhq/wax@alpha --help
```

### Curl installer

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

The installer downloads the matching GitHub Release archive for your OS and architecture, verifies `sha256`, and installs `wax` to `/usr/local/bin` or `~/.wax/bin` if the system location is not writable.

If it installs to `~/.wax/bin`, add that to your shell:

```bash
export PATH="$HOME/.wax/bin:$PATH"
```

Verify the install:

```bash
wax --help
```

Install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash -s -- --version 0.1.0-alpha.1
```

## Quick start

1. Install the `wax` binary.
2. Initialize a repository and choose one or more language packs:

```bash
wax init --non-interactive --language <language-id>
```

For example:

```bash
wax init --non-interactive --language compose
```

You can repeat `--language` for multiple stacks:

```bash
wax init --non-interactive --language compose --language react
```

For local setup, `wax init` can guide you through the same choices:

```bash
wax init
```

The wizard asks which language packs to enable, which source roots Wax should scan for each language, and whether your design-system registry source lives in this repository. If the registry source is in the repo, init prints the `wax registry discover` commands to run next. It does not run discovery or scan automatically.

`wax init` writes:

- `.wax/wax.config.json`
- `.wax/wax.lock.json`
- `.wax/<language-id>.registry.json`

When a language omits `registry` in config, `wax scan` falls back to `.wax/wax.registry.json`.

3. Validate the repo setup:

```bash
wax validate
```

4. Populate or generate the registry.

Minimal manual registry example:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton"
    }
  ]
}
```

Or discover components from source:

```bash
wax discover --language <language-id> --dry-run
wax discover --language <language-id> --force
wax language update --all
wax validate
```

5. Run a scan:

```bash
wax scan
```

6. Inspect outputs under `.wax/out/`, including `.wax/out/scan-merged.json`.

## Languages

Wax uses installable language packs instead of baking every analyzer into the core binary.

Current first-party packs in this repository:

| Language pack | Use for |
| --- | --- |
| `compose` | Jetpack Compose and Kotlin UI code |
| `react` | React and JSX/TSX projects |
| `swift` | SwiftUI projects |
| `basic` | Text-based fallback for unsupported ecosystems and smoke tests |

Install a pack explicitly:

```bash
wax language install compose
wax language install react
wax language install swift
wax language install basic
```

Or let `wax init` and `wax scan` install pinned packs automatically from your configured pack index.

List installed packs:

```bash
wax language list
```

Check repo and install state together:

```bash
wax language doctor
```

## Updating language packs

Use `wax language update` to pull the latest available version of an installed pack and replace older local versions.

Update one language pack:

```bash
wax language update compose
```

Update every installed language pack:

```bash
wax language update --all
```

If you changed repo registry content, registry sources, or lockfile-related config, refresh and then validate:

```bash
wax language update --all
wax validate
```

Use `wax language doctor` to check installed packs, repo lock state, and the effective pack index URL.

If you need to update from a non-default pack index, pass `--registry` or set `WAX_LANG_INDEX`.

## Registry workflow

The registry is the source of truth for the design-system components you want `wax` to track.

Per-language default:

```text
.wax/<language-id>.registry.json
```

For example, `wax init --language swift` scaffolds `.wax/swift.registry.json`. When a language entry omits `registry`, scan resolution falls back to `.wax/wax.registry.json`.

Typical workflow:

1. Initialize repo config with one or more languages.
2. Add or discover canonical components into each language's registry file (for example `.wax/swift.registry.json`).
3. Run `wax validate`.
4. Run `wax scan`.
5. After changing the registry, refresh lock state with `wax language update --all`.

You can also point a language at a hosted or alternate registry source:

```json
{
  "schema_version": 1,
  "languages": [
    {
      "id": "compose",
      "enabled": true,
      "registry": {
        "source": "https://example.com/acme-ds/registry/v2.4.1/compose.json"
      },
      "roots": ["app/src/main/kotlin"]
    },
    {
      "id": "swift",
      "enabled": true,
      "registry": ".wax/swift.registry.json",
      "roots": ["App/Sources"]
    }
  ]
}
```

### Separate design system repository

When the design system lives in its own codebase, you do not need `wax init` in that repository to generate a registry. Install the matching language pack once, point discover at the design-system source tree, and publish the JSON artifact for app repositories to consume.

**Generate in the design-system repo (configless):**

```bash
wax language install react
wax discover --language react --root packages/components/src
```

This writes `.wax/react.registry.json` under the design-system repository. From there you can:

- **Host it** — upload the file to a versioned URL in CI (for example `https://cdn.example.com/acme-ds/v2.4.1/react.json`)
- **Copy it** — commit or copy the generated JSON into an app repository's `.wax/` directory
- **Release it** — attach the registry file to a GitHub Release or internal artifact store

App repositories then point at the hosted file:

```json
"registry": {
  "source": "https://cdn.example.com/acme-ds/v2.4.1/react.json"
}
```

Run `wax language update` in the app repo after changing the registry source so the lockfile pins the new digest.

**Generate from a sibling checkout at your workspace root:**

If the design-system repository is cloned next to an app repository (or anywhere reachable from the repo root), you can pass `--root` to that folder without initializing Wax in the design-system repo:

```text
workspace/
  acme-app/          # wax init here; scans app code
  acme-design-system/ # design-system source only
```

From `acme-app`:

```bash
wax discover --language react --root ../acme-design-system/packages/components/src
```

Wax still writes the registry under `acme-app/.wax/react.registry.json`. Use this when you want to refresh the registry locally from a checked-out design-system tree before committing or publishing it.

Discovery requires a globally installed language pack when no repo lockfile exists. Deterministic discovery may include false positives — review the generated registry before publishing.

SwiftUI v1 detects `struct Name: View` components, `func Name(...) -> some View`
components, direct calls such as `PrimaryButton(...)`, and simple member-qualified
calls such as `DesignSystem.PrimaryButton(...)`.

## AI skills

Wax also ships optional agent skills under [skills](skills). These are guided workflows that call `wax` commands for registry authoring and adoption analytics — not a replacement for the deterministic CLI runtime.

Today the repo includes:

| Skill | Purpose |
| --- | --- |
| `wax-registry-discover` | Preview discovered registry entries, review changes, write per-language registry files (for example `.wax/react.registry.json`), validate, and refresh locks |
| `wax-scan` | Validate config, run a fresh scan, extract adoption metrics, and produce terminal or HTML design-system analytics reports |

The key distinction:

- `wax` CLI: deterministic runtime used in local development and CI
- AI skills: optional guided workflows that call `wax` commands to help you create or update registry files, or interpret scan results into actionable reports

### `wax-registry-discover`

In practice, `wax-registry-discover` fits around the registry workflow like this:

1. Run `wax discover --language <id> --dry-run`
2. Review additions and removals
3. Write `.wax/<language-id>.registry.json` with `wax discover --language <id> --force`
4. Run `wax validate`
5. Run `wax language update --all` when registry locks need refreshing

### `wax-scan`

Invoke the skill when you want adoption analytics after a scan — for example `/wax-skills:wax-scan` in Claude Code, or by attaching `skills/wax-scan/SKILL.md` in your agent.

The skill orchestrates:

1. `wax validate` — stop on failure
2. `wax scan` — always a fresh scan (pass `--no-auto-install` in CI)
3. `skills/wax-scan/scripts/extract-insights.sh` on `.wax/out/scan-merged.json`
4. A section-by-section terminal report (default), and optionally an HTML dashboard

| Parameter | Effect |
| --- | --- |
| *(none)* | Terminal report only |
| `--html` | Also write `.wax/out/report/index.html` |
| `--html-only` | Write HTML only; skip terminal output |
| `--baseline <path>` | Compare against a prior `scan-merged.json` for limited trend deltas |
| `--no-auto-install` | Pass through to `wax scan` for CI runs with committed lockfiles |

**Output paths:**

- Terminal report — default skill output (section-by-section analytics)
- `.wax/out/report/index.html` — self-contained HTML dashboard when `--html` or `--html-only` is requested
- Extractor JSON — stdout from `extract-insights.sh` (used internally by the skill for deterministic metrics)

### Install

With the [skills CLI](https://skills.sh/):

```bash
npx skills add daio-io/wax
```

Or install as a Claude Code plugin:

```text
/plugin marketplace add daio-io/wax
/plugin install wax-skills@wax-skills
/reload-plugins
```

Then invoke a skill directly, for example:

```text
/wax-skills:wax-registry-discover
/wax-skills:wax-scan
```

If you installed the earlier preview plugin as `wax@wax-skills`, reinstall it with the new plugin name:

```text
/plugin uninstall wax@wax-skills
/plugin install wax-skills@wax-skills
```

The skill command also changed from the earlier preview plugin command to `/wax-skills:wax-registry-discover`.

Advanced skills CLI options still work for scripted installs:

```bash
npx skills add daio-io/wax --list
npx skills add daio-io/wax --skill wax-registry-discover -a claude-code -y
npx skills add daio-io/wax --skill wax-scan -a claude-code -y
npx skills add daio-io/wax --skill wax-registry-discover -g -a claude-code -y
npx skills add daio-io/wax --skill wax-scan -g -a claude-code -y
```

### Manual skill install

Copy or symlink a skill directory from `skills/<skill-name>/` into your agent's skills folder, such as:

- `.agents/skills/`
- `.claude/skills/`
- `~/.cursor/skills/`

## CI usage

Commit `.wax/wax.lock.json`. In CI, install or restore the pinned language packs before running scans without auto-install.

Typical flow:

```bash
wax validate
wax language install compose
wax scan --no-auto-install
```

If you change registry content or registry sources, refresh locks locally before committing:

```bash
wax language update --all
wax validate
```

## Schema and config notes

For editor validation and autocomplete on `.wax/wax.config.json`:

```json
{
  "$schema": "https://raw.githubusercontent.com/Daio-io/wax/main/engine/crates/wax-contract/schemas/waxrc.schema.json"
}
```

Notes:

- Preferred config path is `.wax/wax.config.json`.
- Legacy `.waxrc` is still supported when the preferred file is absent.
- Preferred lockfile path is `.wax/wax.lock.json`.
- One repo should usually have one Wax config and one Wax lockfile.

## Uninstall

Remove the binary and Wax global state:

```bash
wax uninstall --full
```

Remove one language pack:

```bash
wax language uninstall compose
```

Remove a specific installed version:

```bash
wax language uninstall compose --version 0.1.0
```

## Build locally

If you want to run Wax from a local build instead of installing a release:

```bash
cd engine
cargo build --release -p wax-cli
./target/release/wax --help
```

## Contributing

Contributor workflow, verification commands, repo layout, and release/process notes live in [CONTRIBUTING.md](CONTRIBUTING.md).

## More docs

- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md)
- [Component tracker design](docs/specs/2026-05-13-component-tracker-design.md)
- [Implementation plans](docs/plans/README.md) — active plan order and status for agents
- [Architecture decision records](docs/adr/README.md) — what each completed phase shipped
- [Archived implementation plans](docs/plans/archive/README.md) — completed plan documents
- [Post-alpha UX plan](docs/plans/2026-05-24-post-alpha-ux-plan.md) — deferred follow-on work
