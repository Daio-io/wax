# wax

Open-source, self-hostable design system component tracker. See [component tracker design](docs/specs/2026-05-13-component-tracker-design.md).

## Rust engine + language packs direction

- [Implementation plan roadmap](docs/plans/README.md) — plan order and status for agents
- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md) — `.waxrc`, global install, IPC, terminology
- [Rust engine implementation plan](docs/plans/2026-05-16-rust-engine-language-packs-plan.md) — engine foundation (order 1)
- [Release and rollout plan](docs/plans/2026-05-24-release-and-rollout-plan.md) — alpha release, install channels (order 2)
- [Post-alpha UX plan](docs/plans/2026-05-24-post-alpha-ux-plan.md) — guided init, scan exports, CI summaries, local reports (order 3)
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

Language packs are not bundled with the CLI binary. After installing `wax`, run:

```bash
wax init --non-interactive --language compose
wax language install compose
```

To install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash -s -- --version 0.1.0-alpha.1
```

Note: `--dry-run` without `--version` still queries the GitHub API to resolve the latest release tag.

### Homebrew (tap) — pending

Homebrew is not a usable install path yet.

- The formula currently lives in this repo as a draft at `homebrew/Formula/wax.rb`.
- A working tap install requires a dedicated tap repo (`Daio-io/homebrew-wax`) with `Formula/wax.rb`.
- The draft formula still needs real 64-character `sha256` values from published GitHub Release assets.
- Current formula targets macOS archives only.

Use the curl installer above for now.

Contributor/local install path:

```bash
cd engine
cargo test -p wax-cli
cargo build --release -p wax-cli   # optimized binary at target/release/wax
cargo install --path crates/wax-cli --locked   # install wax into $PATH
```
