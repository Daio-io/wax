# Token Value Maintenance

Reviewed workflow for filling and refreshing optional canonical token `value` fields in publisher registries. Companion to [SKILL.md](SKILL.md). Golden walkthrough: [examples/token-value-refresh.md](examples/token-value-refresh.md).

AI-derived values are authoring suggestions only. They become deterministic scan inputs only after the user reviews them, they are written to the publisher registry, and a fresh scan runs.

## When to use

- Direct: discover, update, refresh, or audit token values in a Wax registry.
- Delegated: `wax-scan` reports schema-v3 `unassessed` observations (registry metadata gaps) and the user accepts enrichment.

## Resolve the publisher first

1. From an app repo, read `registry.upstream` / `design_systems` and remembered state (`wax registry show <id>`).
2. Open the publisher design-system repository as the authoritative edit target.
3. If the publisher cannot be resolved, stop and instruct the user to remember or point at the design-system repo.
4. Never treat an app-local synced copy under `.wax/registries/<design-system>/` as authoritative source.

## Canonical-value evidence rules

Every proposal must show:

| Field | Requirement |
|-------|-------------|
| Language | Registry language id (`compose`, `react`, `swift`, …) |
| Token id / key / category | Existing registry identity; do not invent ids without approval |
| Current value | Present value, or explicitly `missing` |
| Proposed source-facing value | One canonical string for that language registry |
| Source file / line | Declaration or assignment used as **source evidence** |
| Resolution explanation | How the proposed value was derived from that evidence |
| Confidence | High for direct constants; lower for aliases; refuse ambiguous cases |

Accept:

- Direct constants assigned to the token declaration.
- Traceable simple aliases that resolve to a single constant without computation.

Reject / leave unassessed:

- Computed expressions, runtime lookups, or theme/mode-dependent values.
- Do not flatten light/dark or other modes into one canonical value.

Attach source evidence to every AI-inferred token change. Do not propose a value without a concrete file/line citation.

## Reviewed write workflow

1. Run deterministic component discovery in preview mode:

   ```bash
   wax registry discover \
     --design-system <id> \
     --name "<Display Name>" \
     --language <id> \
     --root <path> \
     --dry-run
   ```

2. Compare dry-run output with the current publisher registry.
3. Inspect token declarations and source assignments for missing or stale canonical values.
4. Show a **structured diff** with separate groups:

   - Component changes
   - Token additions
   - Values filled (missing → proposed)
   - Values changed (existing → proposed)
   - Potential removals

5. Require **explicit approval** for additions and value changes.
6. Require a separate approval for removals. **Never delete** components or tokens automatically.
7. Preserve ids, keys, aliases, categories, metadata, and values outside the approved diff.
8. Apply only the approved registry edits with `apply_patch`.
9. Run validation, then sync/rescan when this was a delegated app flow (see below).

## Validation, sync, and rerun

After an approved publisher-registry edit:

```bash
wax validate
```

A failed write or validation leaves the previous registry recoverable and stops before sync.

For a delegated app flow with resolvable upstream, return to the app and run:

```bash
wax sync
wax validate
wax scan
```

Then regenerate the report. Only the fresh post-sync scan may reclassify observations as exact, near, or unmatched. Do not insert inferred values directly into scan metrics or report KPIs.
