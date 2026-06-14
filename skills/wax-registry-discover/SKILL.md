---
name: wax-registry-discover
description: Use when updating Wax design-system registries from source packages; auto-detects languages and roots when needed, runs deterministic discovery, reviews candidates, asks about ambiguous exports, writes the language registry, validates config, and refreshes locks.
---

# Wax Registry Discover

Use this skill to help a project author update a Wax language registry, such as `.wax/react.registry.json` or a configured language-specific `registry` path, from source packages while keeping all runtime scan and validate behavior deterministic. AI review is an authoring aid only; do not make `wax scan` or `wax validate` depend on agent decisions.

## Command

Prefer the top-level discover command:

```bash
wax discover --language <id> [--root <path>...] [--dry-run] [--force]
```

`wax registry discover` remains valid for backward compatibility and accepts the same flags.

Discovery writes `.wax/<language-id>.registry.json` by default unless the language config points at another repo-relative registry path.

## Resolve language and roots first

Inspect the repository before running discover.

### When Wax config exists

- Prefer `.wax/wax.config.json`, then fall back to `.waxrc`.
- Use enabled language ids from config.
- Use configured `roots` for the selected language when `--root` is not passed.
- Ask which language to discover when more than one enabled language could apply and the user did not specify one.

### When Wax config is absent (no `wax init`)

Use configless discovery: **always pass `--root`** and do not assume configured roots exist.

1. Auto-detect candidate languages from source files and project markers:

   | Language id | Signals |
   |-------------|---------|
   | `compose` | `.kt` files with `@Composable`, Gradle Kotlin/Android modules, `build.gradle.kts` with Compose dependencies |
   | `react` | `.tsx`/`.jsx` component exports, `package.json` with `react` dependency |
   | `swift` | `.swift` files with `View` conformance or `-> some View` component functions, `.xcodeproj`/Package.swift |

2. Auto-detect design-system source roots when not obvious:

   - Prefer package or module source dirs such as `src/`, `src/main/kotlin`, `packages/*/src`, `Sources/`, or paths named like `components`, `design-system`, or `ui`.
   - Exclude tests, stories, demos, fixtures, and generated output (`__tests__`, `*.stories.*`, `*.test.*`, `fixtures`, `dist`, `build`, `.wax`).
   - When multiple candidates remain, pick the smallest set that clearly contains public component exports; ask the user only if still ambiguous.

3. Ensure the required language pack is installed globally before discover:

   ```bash
   wax language install <id>
   ```

   Configless discover uses the globally installed pack when no repo lockfile pins a version.

4. Ask which language to discover when auto-detection finds more than one plausible language and the user did not specify one.

## Workflow

1. Resolve `--language` and `--root` using the rules above.
2. Run discovery in preview mode first:

   ```bash
   wax discover --language <id> --root <path> --dry-run
   ```

   Omit `--root` only when Wax config exists and configured roots clearly target the design-system package.

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
   wax discover --language <id> --root <path>
   ```

   In configless mode, keep `--root` on the write command as well.

   If an existing target registry blocks the write, do not blindly overwrite. Show the diff or summary before `--force`, then run the forced write only after explicit user approval:

   ```bash
   wax discover --language <id> --root <path> --force
   ```

7. When Wax config exists, validate after write:

   ```bash
   wax validate
   ```

   Skip this step when the repository has no Wax config yet. Tell the user that `wax validate` requires `wax init` in consuming app repositories.

8. When Wax config and lockfile exist, refresh locks when registry locks are stale or validation indicates stale language/registry state:

   ```bash
   wax language update
   wax validate
   ```

   Skip lock refresh in configless repositories without `.wax/wax.lock.json`.

## Guardrails

- dry-run before write
- use `--root` in configless repositories
- do not blindly overwrite
- show diff or summary before --force
- validate after write when Wax config exists
- refresh locks when a lockfile exists
