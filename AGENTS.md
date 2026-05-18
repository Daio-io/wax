# Repository Guidance

## Scope

This repository is building `wax`, a Rust analysis engine with downloadable language packs. Production Rust code lives under `engine/`. The historical `rust-prototype/` workspace is reference material only until the plan removes it.

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

- Follow `docs/plans/2026-05-16-rust-engine-language-packs-plan.md` task boundaries.
- Treat one checked task as one focused PR unless explicitly directed otherwise.
- Tick off the plan checkboxes for completed task steps in the same PR.
- Keep generated/local scan output out of git, especially `.wax/` and global `~/.wax/` state.

## Code Style

- Prefer existing crate patterns over new abstractions.
- Keep public Rust API docs current; crates may use `#![deny(missing_docs)]`.
- Use typed errors at contract boundaries instead of returning bare strings.
- Run `cargo fmt` before committing Rust changes.
