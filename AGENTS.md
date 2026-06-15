# Repository Guidance

## Scope

This repository is building `wax`, an open-source design-system analysis engine and CLI.

Production Rust code lives under `engine/`:

- `wax-cli` is the end-user `wax` binary.
- `wax-core` owns repository config, scan orchestration, language installation, validation, and output generation.
- `wax-contract` and `wax-lang-api` define stable contracts between the engine and language packs.
- `wax-lang-basic`, `wax-lang-compose`, and future `wax-lang-*` crates are language packs.

Release and install surfaces live outside `engine/` and are still part of the product:

- `.github/workflows/` for CI and release automation.
- `scripts/` for release artifact generation, pack indexes, and curl installation.
- `homebrew/` for the draft Homebrew formula.
- `packages/cli/` for the optional `@waxhq/wax` npm wrapper.

Keep generated scan output out of git, especially `.wax/` and global `~/.wax/` state.

## Verification

When changing Rust code under `engine/`, run the narrowest checks that cover the touched crates. For broad engine, CLI, contract, or cross-crate changes, run:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For focused crate changes, use the relevant package checks from the active plan, for example:

```bash
cd engine
cargo test -p wax-core
cargo clippy -p wax-core --all-targets -- -D warnings
```

Always run `cargo fmt` before committing Rust changes. If a plan specifies exact verification commands for a task, run those commands and broaden only when the touched code requires it.

For release/install changes, run the relevant script or workflow-adjacent checks in addition to Rust checks when applicable:

```bash
scripts/test-generate-pack-index.sh
scripts/install.sh --help
```

For documentation-only changes, inspect the rendered Markdown when practical and ensure links point to current plan/spec files.

## Planning Discipline

- Use `docs/plans/README.md` as the source of truth for plan order, active implementation phase, and gates. Completed plans live in `docs/plans/archive/` with ADR records in `docs/adr/`.
- Read the active plan before editing. Follow its task boundaries and verification commands.
- Treat one checked task as one focused PR unless explicitly directed otherwise.
- Tick off every completed plan checkbox in the same PR, including the task heading and each completed step under it.
- Before opening or updating a task PR, review the active plan section and make sure completed implementation, verification, and commit steps are reflected there.
- Do not start the next plan or phase until the roadmap and plan gates allow it.
- If a task updates release behavior, keep README, install docs, changelog, scripts, and plan checkboxes consistent in the same PR when the plan calls for it.

## Git And Commits

- Use focused branches and PRs; keep unrelated local changes out of commits.
- Use conventional commit messages such as `feat:`, `fix:`, `docs:`, `ci:`, `test:`, and `chore:`.
- Prefer small commits that explain the intent of the change.
- Do not include tool or assistant attribution in commits.

## Code Style

- Prefer existing crate patterns over new abstractions.
- Keep public Rust API docs current; crates may use `#![deny(missing_docs)]`.
- Use typed errors at contract boundaries instead of returning bare strings.
- Keep CLI behavior scriptable: preserve non-interactive flags, stable JSON output contracts, and deterministic paths where plans or specs require them.
- Keep language-pack protocol changes backward-aware. Update contract/API crates, pack implementations, fixtures, and schemas together.
- Prefer repo-relative paths in config and outputs unless a task explicitly introduces an absolute-path escape hatch.

## Language Pack Parity

Parser-backed language packs (`compose`, `react`, `swift`, and future packs) must stay aligned on scan semantics. Do not ship behavior in one pack that others cannot express without drift.

When changing registry resolution, usage classification, or scan config for one pack:

1. Update the shared contract (`wax-contract` schemas and types) and any shared helpers in `wax-lang-api` first when the behavior is cross-cutting.
2. Apply the same rules to every parser-backed pack that emits `usage_sites`, including `match_status` values such as `resolved` and `candidate`.
3. Keep optional registry fields backward-compatible across packs. Example: registry component `package` with per-language registry files at `.wax/<language-id>.registry.json`.
4. Add or update fixtures/tests in each affected pack crate, not just the pack you are editing.

If a pack needs ecosystem-specific import syntax, mirror the same outcomes: imports that match registry `package` resolve, non-matching imports are not counted as design-system usage, unclear imports become `candidate`, and legacy name-only behavior remains only when registry `package` is absent.

## Product Contracts

- `.waxrc`, `wax.lock.json`, pack index JSON, scan output JSON, and schema files are user-facing contracts. Update fixtures, schemas, docs, and tests when changing them.
- `wax validate` must remain repo-local and CI-friendly; it should not depend on global `~/.wax/` install state.
- `wax scan --no-auto-install` must remain suitable for CI with committed lockfiles and preinstalled language packs.
- Install channels distribute the engine binary; language packs are downloaded on demand into `~/.wax/langs/`.
- Alpha release work targets GitHub Releases, curl installer, Homebrew tap, and optional npm wrapper as described in the release plan.
