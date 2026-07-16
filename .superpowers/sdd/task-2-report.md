# THE-228 Task 2 Report

Date: 2026-07-16

## Summary

Implemented the wax-cli home-resolution fix so commands that require global state now return typed `PathsError::HomeUnavailable` instead of panicking when `HOME` and `WAX_HOME` are unavailable. Scan-time best-effort registry sync now preserves existing warning-only behavior when state-path resolution fails.

## Changed files

- `engine/crates/wax-cli/src/commands/state_path.rs`
- `engine/crates/wax-cli/src/lib.rs`
- `engine/crates/wax-cli/src/commands/scan.rs`
- `engine/crates/wax-cli/src/commands/sync.rs`
- `engine/crates/wax-cli/src/commands/init.rs`
- `engine/crates/wax-cli/src/commands/language.rs`
- `engine/crates/wax-cli/src/commands/registry.rs`
- `engine/crates/wax-cli/tests/sync_command.rs`
- `.superpowers/sdd/task-2-report.md`

## Design decisions

1. Added a shared `resolve_state_path(override_path: Option<&Path>) -> Result<PathBuf, PathsError>` helper in `commands/state_path.rs` so every command resolves overrides and fallback state paths the same way.
2. Added transparent `Paths` variants to `ScanCommandError` and `SyncCommandError` so required-state commands propagate typed home-resolution failures instead of panicking.
3. Replaced the previous private state-path helpers in `init.rs`, `language.rs`, and `registry.rs` with the shared resolver while keeping each command’s existing typed error conversion via `?`.
4. Kept scan-time registry sync best-effort: if state-path resolution fails before sync, `attempt_scan_time_registry_sync` emits the existing generic warning and returns `Ok(())`, matching the command’s non-fatal sync behavior.
5. Kept state-path overrides authoritative by returning override paths directly from the shared resolver without consulting `HOME`/`WAX_HOME`.

## TDD / regression coverage

Added tests first, then implemented the production fix:

- `engine/crates/wax-cli/tests/sync_command.rs`
  - `sync_without_home_returns_typed_error_instead_of_panicking`
- `engine/crates/wax-cli/src/commands/scan.rs`
  - `ephemeral_scan_without_home_returns_paths_error`
  - `scan_command_warns_and_continues_when_home_is_unavailable_for_best_effort_sync`

These cover the required binary regression, typed scan error propagation, and best-effort warning path.

## Verification commands and output

Ran from `engine/`:

```bash
cargo fmt --all
cargo test -p wax-cli sync_without_home_returns_typed_error_instead_of_panicking
cargo test -p wax-cli scan
cargo clippy -p wax-cli --all-targets -- -D warnings
```

Observed results:

- `cargo fmt --all` → exit 0
- `cargo test -p wax-cli sync_without_home_returns_typed_error_instead_of_panicking` → `1 passed, 102 filtered out`
- `cargo test -p wax-cli scan` → `15 passed, 1 ignored, 87 filtered out`
- `cargo clippy -p wax-cli --all-targets -- -D warnings` → `No issues found`

## Manual reproduction

Ran from `engine/`:

```bash
env -u HOME -u WAX_HOME ./target/debug/wax sync
```

Observed result:

- exit code `1`
- stderr:

```text
error: could not resolve wax home; set WAX_HOME or configure a user home directory
```

- confirmed stderr does not contain `panicked at`

## Self-review concerns

- No known functional concerns after the required verification.
- The new scan warning path intentionally reuses the existing generic warning text for state-path resolution failures, which keeps behavior stable but does not distinguish between sync failures and missing home configuration.
