# Repository Guidance

## Scope

This repository is building `wax`, a design-system analysis engine written in Rust. Production Rust code lives under `engine/`.

## Rust Verification

Before opening or updating a PR that changes `engine/`, run the same checks as CI:

```bash
cd engine
cargo fmt --all --check
cargo test -p wax-contract
cargo clippy -p wax-contract --all-targets -- -D warnings
```

As more crates land, broaden `cargo test` and `cargo clippy` from `-p wax-contract` to the relevant crate or workspace scope in the same PR that introduces them.

## Planning Discipline

- Follow the active plan's task boundaries.
- Treat one checked task as one focused PR unless explicitly directed otherwise.
- Tick off every completed plan checkbox in the same PR, including the task heading and each completed step under it.
- Before opening or updating a task PR, review the active plan section and make sure completed implementation, verification, and commit steps are reflected there.
- Keep generated/local scan output out of git, especially `.wax/` and global `~/.wax/` state.

## Git And Commits

- Use focused branches and PRs; keep unrelated local changes out of commits.
- Use conventional commit messages such as `feat:`, `fix:`, `docs:`, `ci:`, `test:`, and `chore:`.
- Prefer small commits that explain the intent of the change.
- Do not include tool or assistant attribution in commits.

## Code Style

- Prefer existing crate patterns over new abstractions.
- Keep public Rust API docs current; crates may use `#![deny(missing_docs)]`.
- Use typed errors at contract boundaries instead of returning bare strings.
- Run `cargo fmt` before committing Rust changes.
