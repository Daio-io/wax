# Contributing

This repository builds `wax`, an open-source design-system analysis engine and CLI.

## Repo layout

Production Rust code lives under [engine](engine):

- `wax-cli`: end-user `wax` binary
- `wax-core`: repo config, scan orchestration, installs, validation, output generation
- `wax-contract` and `wax-lang-api`: stable contracts between the engine and language packs
- `wax-lang-basic`, `wax-lang-compose`, `wax-lang-react`, `wax-lang-swift`: language packs

Other important surfaces:

- [.github/workflows](.github/workflows): CI and release automation
- [scripts](scripts): release helpers, pack index generation, installer
- [homebrew](homebrew): Homebrew formula
- [packages/cli](packages/cli): npm wrapper
- [skills](skills) and [.claude-plugin](.claude-plugin): optional AI skills and plugin metadata

Keep generated scan output out of git, especially `.wax/` outputs and global `~/.wax/` state.

## Local development

Rustup with Rust 1.95.0 is required for engine development. The repository's
`rust-toolchain.toml` installs the minimal toolchain with Clippy and rustfmt;
confirm it is active with:

```bash
rustup show active-toolchain
```

Build and run the CLI locally:

```bash
cd engine
cargo build --release -p wax-cli
./target/release/wax --help
```

Install from the local workspace into your shell path:

```bash
cd engine
cargo install --path crates/wax-cli --locked
```

## Verification

For broad Rust changes under `engine/`, run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For focused crate changes, run the narrowest checks that cover the touched code, for example:

```bash
cd engine
cargo test -p wax-core
cargo clippy -p wax-core --all-targets -- -D warnings
```

Always run `cargo fmt` before committing Rust changes.

For release and install changes, also run the relevant script checks when applicable:

```bash
scripts/test-generate-pack-index.sh
scripts/install.sh --help
```

For documentation-only changes, inspect the rendered Markdown when practical and verify links and examples stay current.

## Code and product conventions

- Prefer existing crate patterns over new abstractions.
- Keep public Rust API docs current.
- Use typed errors at contract boundaries instead of bare strings.
- Keep CLI behavior scriptable, stable, and CI-friendly.
- Prefer repo-relative paths in config and outputs unless a task explicitly requires otherwise.
- When changing user-facing contracts such as `.wax/wax.config.json`, `.wax/wax.lock.json`, per-language `.wax/<language-id>.registry.json`, or schemas, update fixtures, tests, and docs together.

## AI skills

- Skills live at `skills/<skill-name>/SKILL.md`.
- Keep the `name:` frontmatter in `SKILL.md` aligned with the skill directory name.
- Keep [.claude-plugin/marketplace.json](.claude-plugin/marketplace.json) and [.claude-plugin/plugin.json](.claude-plugin/plugin.json) aligned with the public install commands.

## Commits and PRs

- Use focused branches and PRs.
- Keep unrelated changes out of commits.
- Prefer conventional commit prefixes such as `feat:`, `fix:`, `docs:`, `ci:`, `test:`, and `chore:`.
- Keep commits small and intent-revealing.
- Do not include tool or assistant attribution in commits.

## Release notes

- The engine binary is distributed through release channels.
- Language packs install on demand into `~/.wax/langs/`.
- If a change affects release behavior, keep README, install docs, scripts, and relevant plans aligned in the same PR when required.
