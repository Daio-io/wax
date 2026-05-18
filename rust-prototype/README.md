# wax API sketch (`wax-contract` + `wax-lang-api`)

Rough Rust types for the engine ↔ language pack boundary:

| Crate | Role |
|-------|------|
| `wax-contract` | `ScanFacts`, typed enums, `validate_schema_version` |
| `wax-lang-api` | `LanguageExtractor` (in-process), `protocol` (wire JSON) |

Docs: [language packs spec](../docs/specs/2026-05-16-language-packs-and-distribution.md) · [implementation plan](../docs/plans/2026-05-16-rust-engine-language-packs-plan.md)

```bash
cd rust-prototype && cargo build && cargo test -p wax-contract
```
