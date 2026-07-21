---
name: wax-registry-discover
description: Use when updating Wax design-system registries from source packages; auto-detects languages and roots when needed, runs deterministic discovery, reviews candidates, proposes reviewed canonical token values, asks about ambiguous exports, remembers design systems, writes registries after explicit approval, validates config, and refreshes app inputs with wax sync.
---

# Wax Registry Discover

Use this skill to help design-system maintainers and app teams work with Wax registries while keeping runtime scan and validate behavior deterministic. AI review is an authoring aid only; do not make `wax scan` or `wax validate` depend on agent decisions.

This skill covers component discovery and reviewed canonical token-value maintenance. See [token-value-maintenance.md](token-value-maintenance.md) for evidence rules and the reviewed write workflow. See [examples/token-value-refresh.md](examples/token-value-refresh.md) for a golden end-to-end value fill.

## Entry points

```text
Direct: discover, update, refresh, or audit a Wax registry.
Delegated: wax-scan finds unassessed observations and offers registry enrichment.
```

| Entry | When | What to do |
|-------|------|------------|
| Direct | User asks to discover, update, refresh, or audit a registry | Run this skill end-to-end from the publisher repo |
| Delegated | `wax-scan` reports unassessed token observations and the user accepts maintenance | Resolve the publisher, propose reviewed values with source evidence, write only after approval, then sync and rescan the app |

## Two repository roles

| Role | Typical repo | Primary commands |
|------|--------------|------------------|
| Design-system publisher | DS package/monorepo | `wax registry discover` with `--design-system` |
| App consumer | Product codebase | `wax init`, `wax sync`, `wax scan` |

Repo config lives at `.wax/wax.config.json`. Lockfile lives at `.wax/wax.lock.json`.

**Authoritative writes always target the publisher registry.** Resolve the publisher through `design_systems` config or remembered upstream metadata (`wax registry show`, `registry.upstream`). If the publisher cannot be resolved, stop with instructions to point at the design-system repo or remember it; never edit an app-local synced copy under `.wax/registries/<design-system>/` as the authoritative source.

## Command

Prefer the registry subcommand:

```bash
wax registry discover \
  --design-system <id> \
  --name "<Display Name>" \
  --language <id> \
  [--root <path>...] \
  [--dry-run] \
  [--force]
```

`wax discover` remains a top-level alias with the same flags.

### Design-system discovery output

In a design-system repo, discovery:

1. Writes `.wax/registries/<language>.json`
2. Ensures `design_systems.<id>.registries.<language>.source` in `.wax/wax.config.json`
3. Remembers the design system in `~/.wax/state.json`

Tell app teams they can onboard with:

```bash
wax init
wax sync
wax scan
```

### App registry layout

After `wax init` selects a remembered design system, app repos copy or reference registries under:

```text
.wax/registries/<design-system>/<language>.json
```

App config stores `registry.source` and optional `registry.upstream` as `<design-system>/<language>`.

## Resolve language and roots first

Inspect the repository before running discover.

### When Wax config exists

- Read `.wax/wax.config.json` only.
- Use language keys from the `languages` object or registry keys from `design_systems`.
- Use configured `roots` for the selected language when `--root` is not passed.
- Ask which language to discover when more than one language could apply and the user did not specify one.

### When Wax config is absent

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

## Design-system workflow

1. Resolve `--language`, `--root`, `--design-system`, and `--name`.
2. Run discovery in preview mode first:

   ```bash
   wax registry discover \
     --design-system <id> \
     --name "<Display Name>" \
     --language <id> \
     --root <path> \
     --dry-run
   ```

3. Identify the target registry path:

   - Design-system repos: `.wax/registries/<language>.json` unless config overrides `design_systems.<id>.registries.<language>.source`.

4. Compare dry-run output with the existing registry. Show a concise **structured diff** or summary of added, removed, and changed component ids/symbols. Discovered registries should include a `package` field per component when the language pack can infer it:

   | Language | `package` inference |
   |----------|---------------------|
   | `compose` | Kotlin `package` declaration in the source file |
   | `react` | `package.json` `name` above the discovery root |
   | `swift` | Module name from `Sources/<Module>/` |

   When `package` is present, parser-backed scans count only imports from that package as resolved design-system usage.

5. When refreshing canonical token values (direct or delegated), follow [token-value-maintenance.md](token-value-maintenance.md): inspect token source, attach source evidence, and group the structured diff into component changes, token additions, values filled, values changed, and potential removals.

6. Review ambiguous candidates before writing:

   - Ask about exports that look like helpers, demos, previews, aliases, or duplicate public components.
   - Ask before excluding discovered symbols from the registry.
   - Ask before using `--force`.
   - Require **explicit approval** for additions and value changes; require a separate approval for any removals.

7. Write the registry only after review. Prefer `apply_patch` for registry edits so the approved diff is the only change applied. For full rediscovery writes:

   ```bash
   wax registry discover \
     --design-system <id> \
     --name "<Display Name>" \
     --language <id> \
     --root <path>
   ```

   If an existing registry blocks the write, show the structured diff or summary before `--force`, then run the forced write only after explicit user approval.

8. Validate after write when Wax config exists:

   ```bash
   wax validate
   ```

   A failed write or validation leaves the previous registry recoverable and stops before sync.

9. For a delegated app flow with resolvable upstream, return to the app and run:

   ```bash
   wax sync
   wax validate
   wax scan
   ```

   Then regenerate the report. Only the fresh post-sync scan may reclassify observations as exact, near, or unmatched.

## App workflow

For app repositories that consume a remembered design system:

1. Run interactive init when no committed config exists:

   ```bash
   wax init
   ```

   For CI/scripts, use `wax init --non-interactive --language <id>` with explicit registry inputs.

2. Refresh app registry inputs from the remembered design-system upstream before scanning or when DS registries change:

   ```bash
   wax sync
   wax validate
   ```

   `wax sync` copies local DS registry updates or switches `registry.source` to a declared `published_source`, then refreshes `.wax/wax.lock.json`.

3. Scan the app:

   ```bash
   wax scan
   ```

   When config contains `registry.upstream`, `wax scan` attempts the same best-effort sync first. Sync failures warn and the scan continues with current registry inputs.

Manage remembered design systems with:

```bash
wax registry list
wax registry show <id>
wax registry update <id> --repo-root <path>
wax registry delete <id>
```

## Guardrails

- dry-run before write
- use `--root` when Wax config is absent
- pass `--design-system` and `--name` when discovering in a design-system repo
- do not blindly overwrite
- show a structured diff or summary before `--force`
- require explicit approval before writing additions or value changes
- require separate approval before any removals
- **Never delete** components or tokens automatically
- preserve ids, keys, aliases, categories, metadata, and values outside the approved diff
- validate after write when Wax config exists
- never edit an app-local synced registry as authoritative source
- use `wax sync` for app repos with `registry.upstream`; use `wax language update` for language-pack lock refresh
- AI-proposed values become deterministic scan inputs only after approved registry writes and a fresh scan
