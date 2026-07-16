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

## Review fix addendum

Date: 2026-07-16

### Review findings addressed

1. Updated the committed-config unavailable-home regression so it no longer calls only the private `attempt_scan_time_registry_sync` helper. The test now goes through `run_scan_cli`, asserts the existing warning text, and verifies that execution continues into the real scan path before the engine returns `HomeUnavailable`.
2. Extracted the duplicated committed-config upstream scan fixture setup into a shared test helper and reused it in both scan warning regressions.

### Follow-up changed files

- `engine/crates/wax-cli/src/commands/scan.rs`
- `.superpowers/sdd/task-2-report.md`

### Follow-up verification commands and output

Ran from `engine/`:

```bash
cargo fmt --all
cargo test -p wax-cli scan_command_warns_and_continues_when_home_is_unavailable_for_best_effort_sync --lib
cargo test -p wax-cli sync_without_home_returns_typed_error_instead_of_panicking
cargo test -p wax-cli scan
cargo clippy -p wax-cli --all-targets -- -D warnings
```

Observed results:

- `cargo fmt --all` → exit 0
- `cargo test -p wax-cli scan_command_warns_and_continues_when_home_is_unavailable_for_best_effort_sync --lib` → `1 passed, 56 filtered out`
- `cargo test -p wax-cli sync_without_home_returns_typed_error_instead_of_panicking` → `1 passed, 102 filtered out`
- `cargo test -p wax-cli scan` → `15 passed, 1 ignored, 87 filtered out`
- `cargo clippy -p wax-cli --all-targets -- -D warnings` → `No issues found`

### Notes

- Production semantics are unchanged in this follow-up. The committed-config unavailable-home regression now proves that best-effort sync warning behavior does not short-circuit the real scan entrypoint, while the later engine-level home-resolution failure remains intact.

## Final review finding fix

Date: 2026-07-16

### Finding addressed

The new `scan.rs` unit tests had introduced a private `ENV_LOCK` and local `env_lock()` helper inside `engine/crates/wax-cli/src/commands/scan.rs`. That bypassed the crate-wide environment mutex in `engine/crates/wax-cli/src/testing.rs`, so env-mutating unit tests in `scan.rs` could run concurrently with other `wax-cli` unit tests that were using the shared lock.

### Fix details

1. Removed the private `ENV_LOCK` and local `env_lock()` helper from `engine/crates/wax-cli/src/commands/scan.rs`.
2. Switched the `scan.rs` test module to import `crate::testing::env_lock`.
3. Kept the local `EnvVarGuard` restoration helper unchanged so the tests still restore `HOME` and `WAX_HOME` exactly as before.
4. Did not change any production code paths or command behavior.

### Files changed for this review fix

- `engine/crates/wax-cli/src/commands/scan.rs`
- `.superpowers/sdd/task-2-report.md`

### Exact verification outputs

Ran from `engine/`:

```bash
cargo fmt --all
cargo test -p wax-cli --lib
cargo test -p wax-cli sync_without_home_returns_typed_error_instead_of_panicking
cargo test -p wax-cli scan
cargo clippy -p wax-cli --all-targets -- -D warnings
```

`cargo fmt --all`

```text
[no output]
```

`cargo test -p wax-cli --lib`

```text
cargo test: 57 passed (1 suite, 0.23s)
```

`cargo test -p wax-cli sync_without_home_returns_typed_error_instead_of_panicking`

```text
cargo test: 1 passed, 102 filtered out (13 suites, 0.62s)
```

`cargo test -p wax-cli scan`

```text
cargo test: 15 passed, 1 ignored, 87 filtered out (13 suites, 2.13s)
```

`cargo clippy -p wax-cli --all-targets -- -D warnings`

```text
cargo clippy: No issues found
```

### Concerns

- No known functional concerns from this review fix.
- Remaining separate env locks still exist in several integration-test files under `engine/crates/wax-cli/tests/`, but this change intentionally stayed scoped to the reported `scan.rs` unit-test finding.
