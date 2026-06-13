# Interactive Init Design

## Context

Wax currently supports deterministic repository setup through:

```bash
wax init --non-interactive --language <language-id>
```

That path writes `.wax/wax.config.json`, `.wax/wax.lock.json`, per-language registry scaffold files, and `.gitignore` entries. It is suitable for scripts and CI, but it is terse for first-time local users now that Wax has language packs, per-language registries, and registry discovery.

The first interactive init should improve setup ergonomics without turning `wax init` into a scan or registry-discovery command.

## Goals

- Let a TTY user select one or more available language packs.
- Ask for scan roots per selected language and persist those roots in `.wax/wax.config.json`.
- Ask whether the design-system registry source lives in the current repository.
- If registry source roots are provided, use them only to print follow-up `wax registry discover` commands.
- Preserve the existing `--non-interactive` behavior for scripts and CI.
- Finish by telling the user they are ready to populate registries and scan.

## Non-Goals

- Do not run `wax scan` during init.
- Do not run `wax registry discover` during init.
- Do not add a new config field for registry source roots.
- Do not change lockfile, pack index, or scan output contracts.
- Do not prompt when `--non-interactive` is present.

## User Flow

When `wax init` runs without `--non-interactive` and stdin is a TTY:

1. Resolve the pack index using the same precedence as current init: `--registry`, `WAX_LANG_INDEX`, then the built-in default.
2. Show available language pack ids from that index and prompt for one or more selections.
3. For each selected language, ask for scan roots. These roots are written to the selected language entry as `roots`.
4. Ask whether the design-system registry source is part of this repository.
5. If the user answers no:
   - Write the standard empty per-language registry scaffold files when scaffolding is enabled.
   - Explain that meaningful scans require either populating `.wax/<language>.registry.json` or pointing the language `registry` setting at an external source.
6. If the user answers yes:
   - For each selected language, ask for registry source roots.
   - Do not run discovery.
   - After files are written, print exact follow-up commands:

```bash
wax registry discover --language <language-id> --root <registry-root>
```

7. Write `.wax/wax.config.json`, `.wax/wax.lock.json`, per-language registry scaffold files, and `.gitignore` entries through the same init write path used today.
8. End with a concise success message that points to the next setup action:
   - Run the printed `wax registry discover` command when registry roots were provided.
   - Populate or configure registries when they were not.
   - Then run `wax scan`.

## Non-Interactive Behavior

`wax init --non-interactive` remains the scriptable path. It continues to require at least one `--language <id>` and uses existing flags such as `--registry`, `--target`, `--no-install`, and `--no-scaffold-registries`.

If `wax init` is run without `--non-interactive` while stdin is not a TTY, it should fail with a friendly scriptable message, for example:

```text
wax init needs an interactive terminal. For CI or scripts, run:
wax init --non-interactive --language <language-id>
```

## Architecture

The implementation should separate prompt collection from file writing:

- A prompt layer gathers an `InitSelections` value:
  - selected language ids
  - scan roots by language id
  - whether registry source is in this repo
  - optional registry source roots by language id for follow-up output
- Existing init logic should be refactored just enough to accept selected scan roots instead of always using template defaults.
- The current config, lockfile, registry scaffold, install, and `.gitignore` write behavior should remain the source of truth for durable output.
- Registry source roots should not be persisted. They only drive the final command suggestions.

This keeps the interactive path and non-interactive path converging before durable writes, which makes the behavior easier to test and reduces drift between setup modes.

## Prompt Library

Use a Rust terminal prompt crate only inside `wax-cli`. Either `dialoguer` or `inquire` is acceptable; prefer the smaller integration that supports:

- multiselect language choices
- text input for roots
- yes/no confirmation
- testable prompt abstraction or an easily mockable wrapper

Prompt dependency choice should be documented in the implementation plan and kept out of `wax-core`.

## Error Handling

- If no language is selected, keep prompting or fail with the same missing-language semantics as non-interactive init.
- If a selected language cannot be resolved from the pack index, surface the existing registry/language error.
- If config already exists, fail before prompting for detailed roots when practical.
- If file writes fail, preserve current init error behavior and avoid adding partial-write semantics beyond what init already provides.
- If registry source roots are empty after the user says the registry source is in the repo, print registry-discovery guidance without generated root arguments.

## Testing

- Keep existing non-interactive init tests passing.
- Add focused tests for transforming interactive selections into `.wax/wax.config.json` language entries with user-provided scan roots.
- Add coverage that registry source roots are not persisted in config.
- Add coverage for final guidance:
  - external registry answer prints populate/configure guidance
  - internal registry answer prints `wax registry discover` commands
- Add a CLI-level non-TTY test that `wax init` without `--non-interactive` fails with the friendly scriptable message.

## Verification

Narrow verification for the implementation task:

```bash
cd engine
cargo test -p wax-cli init_interactive
cargo test -p wax-cli --test init_command
```

Broaden to full workspace verification if the implementation touches shared config parsing, lockfile behavior, language install behavior, or `wax-core`:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
