# wax

[![Nice](https://api.nice.sbs/badge/n_c1qWdL8brn1s.svg)](https://nice.sbs/button?id=n_c1qWdL8brn1s)
[![Release](https://img.shields.io/github/v/release/Daio-io/wax?include_prereleases&label=release)](https://github.com/Daio-io/wax/releases)
[![CI](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml/badge.svg?branch=main)](https://github.com/Daio-io/wax/actions/workflows/build_engine.yml)

`wax` is an open-source CLI for analyzing design-system usage in codebases.

It helps teams define a canonical component registry, scan repositories with language-aware extractors, and produce deterministic outputs that work locally and in CI. Optional AI skills can help author and maintain registry files, but the core `wax` runtime stays deterministic.

## What wax does

- Tracks usage of canonical design-system components from a registry file.
- Scans source code with installable language packs such as `compose`, `react`, and `basic`.
- Writes repo-local config, lock, and output files under `.wax/`.
- Supports deterministic validation and CI-safe scanning.
- Can bootstrap and refresh registries with `wax registry discover`.
- Can be paired with optional agent skills for AI-assisted registry authoring and review.

## How it fits together

- `wax` binary: the engine and CLI you install and run.
- Language packs: stack-specific analyzers installed globally under `~/.wax/langs/`.
- Registry: your canonical design-system component list, usually `.wax/wax.registry.json`.
- Repo config: `.wax/wax.config.json` enables languages and points at roots and registry sources.
- Lockfile: `.wax/wax.lock.json` pins language-pack artifacts and registry digests for reproducible scans.
- AI skills: optional authoring workflows that call `wax` commands for tasks like registry sync.

## Install

### Homebrew

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

`wax init` writes:

- `.wax/wax.config.json`
- `.wax/wax.lock.json`
- `.wax/wax.registry.json`

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
wax registry discover --language <language-id> --dry-run
wax registry discover --language <language-id>
wax language update
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
| `basic` | Text-based fallback for unsupported ecosystems and smoke tests |

Install a pack explicitly:

```bash
wax language install compose
wax language install react
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
wax language update
wax validate
```

Use `wax language doctor` to check installed packs, repo lock state, and the effective pack index URL.

If you need to update from a non-default pack index, pass `--registry` or set `WAX_LANG_INDEX`.

## Registry workflow

The registry is the source of truth for the design-system components you want `wax` to track.

Default location:

```text
.wax/wax.registry.json
```

Typical workflow:

1. Initialize repo config with one or more languages.
2. Add or discover canonical components into `.wax/wax.registry.json`.
3. Run `wax validate`.
4. Run `wax scan`.
5. After changing the registry, refresh lock state with `wax language update`.

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
    }
  ]
}
```

## AI skills

Wax also ships optional agent skills under [plugins/wax/skills](plugins/wax/skills). These are for authoring help, not runtime analysis.

Today the repo includes:

| Skill | Purpose |
| --- | --- |
| `wax-registry-sync` | Preview discovered registry entries, review changes, write `.wax/wax.registry.json`, validate, and refresh locks |

The key distinction:

- `wax` CLI: deterministic runtime used in local development and CI
- AI skills: optional guided workflows that call `wax` commands to help you create or update registry files

In practice, a skill like `wax-registry-sync` fits around the registry workflow like this:

1. Run `wax registry discover --language <id> --dry-run`
2. Review additions and removals
3. Write `.wax/wax.registry.json`
4. Run `wax validate`
5. Run `wax language update` when registry locks need refreshing

### Install skills with `skills.sh`

Requires Node.js.

List available skills from this repo:

```bash
npx skills add Daio-io/wax --list
```

Install `wax-registry-sync` into a project-local skills directory:

```bash
npx skills add Daio-io/wax --skill wax-registry-sync -a cursor -y
```

Install it globally instead:

```bash
npx skills add Daio-io/wax --skill wax-registry-sync -g -a cursor -y
```

Swap `cursor` for your agent, such as `claude-code`, `codex`, or `opencode`.

### Install skills in Claude Code

```text
/plugin marketplace add Daio-io/wax
/plugin install wax@wax-skills
/reload-plugins
```

Then invoke the skill directly, for example:

```text
/wax:wax-registry-sync
```

### Manual skill install

Copy or symlink a skill directory from `plugins/wax/skills/<skill-name>/` into your agent's skills folder, such as:

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
wax language update
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
- [Implementation plans](docs/plans/README.md)
