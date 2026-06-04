# wax

[![Nice](https://api.nice.sbs/badge/n_c1qWdL8brn1s.svg)](https://nice.sbs/button?id=n_c1qWdL8brn1s)

Open-source, self-hostable design system component tracker. See [component tracker design](docs/specs/2026-05-13-component-tracker-design.md).

## Rust engine + language packs direction

- [Implementation plan roadmap](docs/plans/README.md) — plan order and status for agents
- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md) — `.wax/wax.config.json`, global install, IPC, terminology
- [Rust engine implementation plan](docs/plans/2026-05-16-rust-engine-language-packs-plan.md) — engine foundation (order 1)
- [Release and rollout plan](docs/plans/2026-05-24-release-and-rollout-plan.md) — alpha release, install channels (order 2)
- [Registry sources and layout plan](docs/plans/2026-06-02-registry-sources-and-wax-layout.md) — `.wax/` layout and registry source locking (order 3)
- [Registry discovery design](docs/plans/2026-06-04-registry-discovery-design.md) and [implementation plan](docs/plans/2026-06-04-registry-discovery-plan.md) — next phase for `wax registry discover` and skill-assisted registry sync (order 4)
- [Post-alpha UX plan](docs/plans/2026-05-24-post-alpha-ux-plan.md) — guided init, scan exports, CI summaries, local reports (order 5)
- [`engine/`](engine/) — production Rust workspace (`wax` CLI, language packs, contract crates)

## Install (alpha)

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

The installer detects your OS/arch, downloads the matching release archive from GitHub Releases, verifies the `sha256`, and installs `wax` to `/usr/local/bin` (or `~/.wax/bin` when `/usr/local/bin` is not writable).
If the installer falls back to `~/.wax/bin`, add it to your shell PATH:

```bash
export PATH="$HOME/.wax/bin:$PATH"
```

Verify the installed binary directly with:

```bash
$HOME/.wax/bin/wax --help
```

Language packs are not bundled with the CLI binary. Continue with the compose walkthrough in [Getting started](#getting-started-compose-alpha-path).

To install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash -s -- --version 0.1.0-alpha.1
```

Note: `--dry-run` without `--version` still queries the GitHub API to resolve the latest release tag.

### Homebrew (tap)

```bash
brew tap Daio-io/wax
brew install wax
```

The Homebrew formula currently targets macOS archives only. Language packs are not bundled with the CLI binary; continue with the compose walkthrough in [Getting started](#getting-started-compose-alpha-path).

### npm (optional alpha wrapper)

The npm wrapper is available as an optional alpha install path. It downloads the same host `wax` binary from GitHub Releases during `postinstall`, verifies the `sha256`, and exposes the `wax` executable through npm:

```bash
npm install -g @waxhq/wax@alpha
wax --help
```

You can also run it without a separate global install:

```bash
npx @waxhq/wax@alpha --help
```

The curl installer remains the primary alpha path while the npm package is validated across supported hosts.

Before the first CI publish, configure npm trusted publishing for `@waxhq/wax`:

1. Publish the package once manually so the npm package page exists.
2. In npm package settings, add trusted publishing for GitHub Actions repo `Daio-io/wax` and workflow file `release.yml`.
3. The release workflow stamps `packages/cli/package.json` from the Git tag before `npm publish`, so keep the checked-in file on its snapshot placeholder and let CI derive the published version.
4. Remove or avoid legacy `NPM_TOKEN` publish secrets so OIDC remains the active auth path.

For local smoke tests from the package folder, set `WAX_CLI_VERSION` to the release you want to download; otherwise the wrapper uses the checked-in snapshot placeholder.

## Uninstall

To remove the installed binary and global wax state in one command:

```bash
wax uninstall --full
```

This removes `~/.wax` and attempts to remove common binary locations such as `/usr/local/bin/wax` and `~/.wax/bin/wax`.

### Remove the `wax` binary

If you prefer manual cleanup, remove whichever binary install location exists:

```bash
rm -f /usr/local/bin/wax
rm -f "$HOME/.wax/bin/wax"
```

If you installed via npm:

```bash
npm uninstall -g @waxhq/wax
```

If you installed via Homebrew (once the tap is published):

```bash
brew uninstall wax
```

### Remove installed language packs

Uninstall a specific language (all installed versions):

```bash
wax language uninstall compose
```

Uninstall one version only:

```bash
wax language uninstall compose --version 0.1.0
```

### Remove all global `wax` state (optional)

This removes cached language packs, install state, and fallback binaries under `~/.wax`:

```bash
rm -rf "$HOME/.wax"
```

## Getting started (compose alpha path)

1. Install `wax` (curl path above).
2. Initialize repo config (one-time per repository):

```bash
wax init --non-interactive --language compose
```

`wax init` writes `.wax/wax.config.json`, `.wax/wax.lock.json`, and `.wax/wax.registry.json`. Generated scan output lands in `.wax/out/`, which init adds to `.gitignore`.

3. Populate `.wax/wax.registry.json` with canonical components. Minimal valid example:

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

`wax scan` requires at least one component symbol in `components[]`.

To discover components from Compose sources instead of hand-editing the registry:

```bash
wax registry discover --language compose --dry-run
wax registry discover --language compose
wax language update
wax validate
```

4. Validate repository configuration:

```bash
wax validate
```

5. Run scan:

```bash
wax scan
```

6. Inspect outputs in `.wax/out/` (including `.wax/out/scan-merged.json`).

### AI-assisted registry sync

The distributable `wax-registry-sync` skill package lives at `.agents/skills/wax-registry-sync/`. It helps an AI agent run deterministic registry discovery, review the dry-run output with a human, write `.wax/wax.registry.json`, validate, and refresh stale language locks without making AI part of `wax scan` or `wax validate` runtime.

Until a marketplace listing exists, install it from the repository path in agents or skill managers that support repo-path skills:

```bash
<your-skill-manager> install github:Daio-io/wax/.agents/skills/wax-registry-sync
```

You can also copy `.agents/skills/wax-registry-sync/` into your agent's local skills directory. The same self-contained package is intended for publication through skills.h/skills.sh or a Claude plugin marketplace, but do not assume a marketplace listing exists until it is published.

For editor validation/autocomplete on `.wax/wax.config.json` (or legacy `.waxrc`), use:

```json
{
  "$schema": "https://raw.githubusercontent.com/Daio-io/wax/main/engine/crates/wax-contract/schemas/waxrc.schema.json"
}
```

If your environment cannot fetch remote schemas, copy that schema file into your repository and point `$schema` at the vendored path instead.

Optional hosted registry source on a language entry in `.wax/wax.config.json`:

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

## Monorepo and multi-repo notes

- Use one `.wax/wax.config.json` and one `.wax/wax.lock.json` per repository (legacy `.waxrc` and top-level `wax.lock.json` are still read when preferred files are absent).
- The default pack index is shared (`WAX_LANG_INDEX` can still override per shell/CI job).
- Language packs install once globally under `~/.wax/langs/` and are reused across repos.

## Migrating layout and registry locks

- New repos: `wax init` writes `.wax/wax.config.json`, `.wax/wax.lock.json`, and `.wax/wax.registry.json` only.
- Existing repos can keep legacy `.waxrc` and top-level `wax.lock.json` until you copy or move them under `.wax/`. `wax validate` warns when both old and new files exist.
- If you migrate config to `.wax/wax.config.json` but still use a top-level `wax.lock.json`, `wax validate` warns to move the lockfile to `.wax/wax.lock.json`.
- Lockfiles upgraded from schema v1 may lack per-language `registries` entries. After adopting centralized config or changing a registry source/path, run `wax language update` to refresh registry locks before CI scan.
- After editing a repo-local registry (for example `.wax/wax.registry.json`), run `wax language update` so `registries` digests and sources stay aligned with config.

## CI recipe

Commit `.wax/wax.lock.json`. In CI, restore cached installs from `~/.wax/langs` (and `~/.wax/state.json`) or install pinned languages before scanning. If the lockfile predates registry locks or you change registry sources, refresh locks locally with `wax language update` before committing.

```bash
wax validate
wax language install compose
wax scan --no-auto-install
```

`--no-auto-install` expects required language packs to already be present on disk.

## Contributor/local install path

```bash
cd engine
cargo test -p wax-cli
cargo build --release -p wax-cli   # optimized binary at target/release/wax
cargo install --path crates/wax-cli --locked   # install wax into $PATH
```
