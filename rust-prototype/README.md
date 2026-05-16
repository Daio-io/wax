# wax API sketch (`wax-contract` + `wax-lang-api`)

Rough Rust types for the engine ↔ language pack boundary. Full architecture and implementation steps live in the docs:

- [Language packs and distribution](../docs/specs/2026-05-16-language-packs-and-distribution.md)
- [Rust engine implementation plan](../docs/plans/2026-05-16-rust-engine-language-packs-plan.md)

## Crates

| Crate | Role |
|-------|------|
| `wax-contract` | `ScanFacts`, `LanguageMetadata`, `MergedScan` (JSON shape) |
| `wax-lang-api` | `LanguageExtractor` trait, `ScanRequest`, `LanguageError` |

Language implementations (`wax-lang-compose`, `wax-lang-react`, …) and the `wax` CLI are not in this PR—only the shared contract.

```bash
cd rust-prototype && cargo build
```
