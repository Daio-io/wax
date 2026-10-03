# Finding scan code

Read the [plan index](plans/README.md) for the current implementation gate and its active task. For shipped decisions, use the [ADR index](adr/README.md). Older specs describe the design at the time they were written; check current code and schemas before changing a contract.

The scan path runs from the CLI through core to each language pack, then back through core's merge and output code.

## CLI and scan orchestration

Start at the [`wax scan` command](../engine/crates/wax-cli/src/commands/scan.rs) for flags, setup, diagnostics, and output. Follow `Engine::scan_repo_with_options` in [core](../engine/crates/wax-core/src/lib.rs) for pack execution. Check the [CLI scan tests](../engine/crates/wax-cli/tests/scan_command.rs) and [core output tests](../engine/crates/wax-core/tests/scan_output.rs).

## Config, roots, and registries

The [repo file paths](../engine/crates/wax-core/src/config/repo_files.rs), [config parser](../engine/crates/wax-core/src/config/waxrc.rs), and [config schema](../engine/crates/wax-contract/schemas/waxrc.schema.json) own repository config. For root-group attribution and summaries, follow [root groups](../engine/crates/wax-core/src/root_group.rs) into [adoption merge](../engine/crates/wax-core/src/adoption_merge.rs). Check the [config tests](../engine/crates/wax-core/tests/config_v2.rs) and [scan resolution tests](../engine/crates/wax-core/tests/scan_resolve.rs).

For registry inputs, start at [source resolution](../engine/crates/wax-core/src/registry_source.rs), then the [Git fetch helper](../engine/crates/wax-core/src/registry_git.rs). Check the [registry source tests](../engine/crates/wax-core/tests/registry_source.rs) and focused tests in `registry_git.rs`.

## Contracts and language packs

Shared scan requests live in the [language API](../engine/crates/wax-lang-api/src/lib.rs). Fact types and validation live in the [contract](../engine/crates/wax-contract/src/lib.rs) and [scan facts schema](../engine/crates/wax-contract/schemas/scan-facts.schema.json). Check the [contract tests](../engine/crates/wax-contract/tests/schema_roundtrip.rs) and affected pack fixtures.

For parser extraction and usage classification, start at the [Compose scanner](../engine/crates/wax-lang-compose/src/tree_sitter_scan.rs), [React extraction](../engine/crates/wax-lang-react/src/extract.rs), or [Swift scanner](../engine/crates/wax-lang-swift/src/tree_sitter_scan.rs). Tests and fixtures sit under each `wax-lang-*` crate. Kotlin syntax recovery is in [recovery](../engine/crates/wax-lang-compose/src/kotlin_recovery.rs) and [AST handling](../engine/crates/wax-lang-compose/src/kotlin_ast.rs), with [recovery tests](../engine/crates/wax-lang-compose/tests/parse_recovery.rs).

For a change that affects usage classification or registry matching, follow the parity rules in [`AGENTS.md`](../AGENTS.md#language-pack-parity) and update every affected parser-backed pack.
