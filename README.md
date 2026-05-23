# wax

Open-source, self-hostable design system component tracker. See [component tracker design](docs/specs/2026-05-13-component-tracker-design.md).

## Rust engine + language packs direction

- [Language packs and distribution](docs/specs/2026-05-16-language-packs-and-distribution.md) — `.waxrc`, global install, IPC, terminology
- [Rust engine implementation plan](docs/plans/2026-05-16-rust-engine-language-packs-plan.md) — phased tasks
- [`engine/`](engine/) — production Rust workspace (`wax` CLI, language packs, contract crates)

```bash
cd engine
cargo test -p wax-cli
cargo build --release -p wax-cli   # optimized binary at target/release/wax
cargo install --path crates/wax-cli --locked   # install wax into $PATH
```
