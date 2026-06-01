# wax

Open-source, self-hostable design system component tracker. See [component tracker design](docs/specs/2026-05-13-component-tracker-design.md).

## Rust engine + language packs direction

- [Implementation plan roadmap](docs/plans/README.md) — plan order and status for agents
- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md) — `.waxrc`, global install, IPC, terminology
- [Rust engine implementation plan](docs/plans/2026-05-16-rust-engine-language-packs-plan.md) — engine foundation (order 1)
- [Release and rollout plan](docs/plans/2026-05-24-release-and-rollout-plan.md) — alpha release, install channels (order 2)
- [Post-alpha UX plan](docs/plans/2026-05-24-post-alpha-ux-plan.md) — guided init, scan exports, CI summaries, local reports (order 3)
- [`engine/`](engine/) — production Rust workspace (`wax` CLI, language packs, contract crates)

## Install (alpha: curl-ready, Homebrew pending)

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

### Homebrew (tap) — pending

Homebrew is part of the alpha rollout path, but the tap is not published yet.

- The formula currently lives in this repo as a draft at `homebrew/Formula/wax.rb`.
- A working tap install requires a dedicated tap repo (`Daio-io/homebrew-wax`) with `Formula/wax.rb`.
- The draft formula still needs real 64-character `sha256` values from published GitHub Release assets.
- Current formula targets macOS archives only.

Use the curl installer above for now.

### npm (optional alpha wrapper)

The npm wrapper is available as an optional alpha install path. It downloads the same host `wax` binary from GitHub Releases during `postinstall`, verifies the `sha256`, and exposes the `wax` executable through npm:

```bash
npm install -g @wax/cli
wax --help
```

You can also run it without a separate global install:

```bash
npx @wax/cli --help
```

The curl installer remains the primary alpha path while the npm package is validated across supported hosts.

## Uninstall

### Remove the `wax` binary

If you installed via the curl script, remove whichever install location exists:

```bash
rm -f /usr/local/bin/wax
rm -f "$HOME/.wax/bin/wax"
```

If you installed via npm:

```bash
npm uninstall -g @wax/cli
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

3. Populate `design-system/registry.json` with canonical components. Minimal valid example:

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

`wax init` scaffolds an empty registry; `wax scan` requires at least one component symbol in `components[]`.
4. Validate repository configuration:

```bash
wax validate
```

5. Run scan:

```bash
wax scan
```

6. Inspect outputs in `.wax/out/` (including `.wax/out/scan-merged.json`).

For editor validation/autocomplete on `.waxrc`, use:

```json
{
  "$schema": "https://raw.githubusercontent.com/Daio-io/wax/main/engine/crates/wax-contract/schemas/waxrc.schema.json"
}
```

If your environment cannot fetch remote schemas, copy that schema file into your repository and point `$schema` at the vendored path instead.

## Monorepo and multi-repo notes

- Use one `.waxrc` and one `wax.lock.json` per repository.
- The default pack index is shared (`WAX_LANG_INDEX` can still override per shell/CI job).
- Language packs install once globally under `~/.wax/langs/` and are reused across repos.

## CI recipe

Commit `wax.lock.json`. In CI, restore cached installs from `~/.wax/langs` (and `~/.wax/state.json`) or install pinned languages before scanning:

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
