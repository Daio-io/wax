# THE-230 Task 1B Report

Date: 2026-07-17

## Scope

Implemented the mechanical Clippy fixes required by `task-1b-brief.md` and incorporated only the interrupted worker's uncommitted edits that satisfied Task 1B. Left unrelated edits untouched.

## Files Changed

- `engine/crates/wax-core/src/install.rs`
- `engine/crates/wax-core/src/registry_discovery.rs`
- `engine/crates/wax-core/src/registry_memory.rs`
- `engine/crates/wax-core/src/sync.rs`
- `engine/crates/wax-cli/src/commands/init.rs`
- `engine/crates/wax-cli/src/commands/language.rs`
- `engine/crates/wax-cli/src/commands/scan.rs`
- `engine/crates/wax-cli/src/progress.rs`
- `engine/crates/wax-cli/tests/registry_memory_commands.rs`
- `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`

## Implemented Changes

1. Removed the redundant digest clone in the install path.
2. Removed redundant clones in registry discovery and registry memory tests by borrowing until ownership was needed.
3. Replaced collection-length assertions with iterator `count()` in:
   - `engine/crates/wax-cli/tests/registry_memory_commands.rs`
   - `engine/crates/wax-lang-compose/src/tree_sitter_scan.rs`
4. Changed `resolve_registry_url` to return `String` directly and updated CLI callers/tests.
5. Changed `update_config_registry_source` to return `bool` directly and updated its caller.
6. Kept the interrupted worker's matching core/CLI edits, and added the missing Compose test cleanup required by the brief.

## Verification Commands And Output

```bash
cd /Users/dai/personal/wax/.worktrees/the-230/engine
rtk cargo fmt --all
```

Output: exited 0

```bash
cd /Users/dai/personal/wax/.worktrees/the-230/engine
rtk cargo test -p wax-lang-compose
```

Output: `cargo test: 87 passed (7 suites, 0.44s)`

```bash
cd /Users/dai/personal/wax/.worktrees/the-230/engine
rtk cargo test -p wax-cli
```

Output: `cargo test: 99 passed, 1 ignored (14 suites, 7.59s)`

```bash
cd /Users/dai/personal/wax/.worktrees/the-230/engine
rtk cargo test -p wax-core
```

Output: `cargo test: 211 passed, 2 ignored (20 suites, 21.12s)`

```bash
cd /Users/dai/personal/wax/.worktrees/the-230/engine
rtk cargo clippy -p wax-core -p wax-cli -p wax-lang-compose --all-targets -- -W clippy::redundant_clone -W clippy::needless_collect -W clippy::unnecessary_wraps
```

Output: `cargo clippy: No issues found`

## Self-Review

- Confirmed the final diff stays within the Task 1B brief's mechanical fixes.
- Confirmed the interrupted worker's uncommitted changes were only incorporated where they matched the Task 1B lint targets.
- Confirmed the one missing baseline item (`wax-lang-compose` `needless_collect`) was added before verification.
- Confirmed targeted tests passed for all touched crates and the targeted Clippy pass is clean.

## Concerns

- None for this task slice.
