---
name: wax-registry-sync
description: Use when updating Wax design-system registries from source packages; runs deterministic registry discovery, reviews candidates, asks about ambiguous exports, writes the language registry, validates config, and refreshes locks.
---

# Wax Registry Sync

Use this skill to help a project author update a Wax language registry, such as `.wax/react.registry.json` or a configured language-specific `registry` path, from source packages while keeping all runtime scan and validate behavior deterministic. AI review is an authoring aid only; do not make `wax scan` or `wax validate` depend on agent decisions.

## Workflow

1. Inspect the repository configuration:
   - Prefer `.wax/wax.config.json`.
   - Fall back to `.waxrc` when the newer config file is absent.
   - Identify enabled language ids and configured roots. Ask which language to sync if more than one enabled language could apply.
2. Run discovery in preview mode first:

   ```bash
   wax registry discover --language <id> --dry-run
   ```

   Add `--root <path>` only when the user explicitly wants to override configured roots.
3. Identify the target registry path for the selected language:
   - Use the language entry's configured `registry` path when present.
   - Otherwise expect the default `.wax/<language-id>.registry.json`.
4. Compare the dry-run output with the existing target registry when it exists. Show the user a concise diff or summary of added, removed, and changed component ids/symbols.
5. Review ambiguous candidates before writing:
   - Ask about exports that look like helpers, demos, previews, aliases, or duplicate public components.
   - Ask before excluding discovered symbols from the registry.
   - Ask before using `--force`.
6. Write the registry only after review:

   ```bash
   wax registry discover --language <id>
   ```

   If an existing target registry blocks the write, do not blindly overwrite. Show the diff or summary before --force, then run the forced write only after explicit user approval:

   ```bash
   wax registry discover --language <id> --force
   ```
7. Validate after write:

   ```bash
   wax validate
   ```

8. Refresh locks when registry locks are stale or validation indicates stale language/registry state:

   ```bash
   wax language update
   wax validate
   ```

## Guardrails

- dry-run before write
- do not blindly overwrite
- show diff or summary before --force
- validate after write
- refresh locks
