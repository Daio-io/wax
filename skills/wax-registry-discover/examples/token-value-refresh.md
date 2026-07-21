# Example: Token Value Refresh

Golden end-to-end workflow for filling a missing canonical token value through reviewed registry maintenance. Companion to [../token-value-maintenance.md](../token-value-maintenance.md).

## Setup

- App scan produced schema-v3 `unassessed` observations because `space.medium` has no registry `value`.
- Publisher design-system repo is resolved via `registry.upstream` / remembered metadata.
- Language: `compose`.

## Before maintenance result

The fresh app scan and deterministic extractor show one metadata gap with typed missing-value evidence:

```text
token metrics:
  Confirmed migration candidates: 0
  Possible migration candidates: 0
  Unmatched observations: 0 (informational)
  Unassessed observations: 1 (registry values needed)

token_inference.unassessed_observations[0].evidence:
  - missing_canonical_values
```

## Before registry

Publisher registry before maintenance (token lacks `value`):

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "aliases": ["PrimaryBtn"],
      "package": "com.example.ds"
    },
    {
      "id": "ds.text-field",
      "symbol": "TextField",
      "package": "com.example.ds"
    }
  ],
  "tokens": [
    {
      "id": "color.primary",
      "key": "Theme.colors.primary",
      "category": "color",
      "aliases": ["AppColors.Primary"]
    },
    {
      "id": "space.medium",
      "key": "Spacing.Medium",
      "category": "spacing",
      "aliases": ["SpacingMd"]
    }
  ]
}
```

## Source evidence

Token declaration in the publisher package:

```kotlin
// packages/ds/src/main/kotlin/com/example/ds/Spacing.kt:12
object Spacing {
    val Medium = 16.dp
}
```

Evidence fields for the proposal:

| Field | Value |
|-------|-------|
| Language | `compose` |
| Token id / key / category | `space.medium` / `Spacing.Medium` / `spacing` |
| Current value | missing |
| Proposed source-facing value | `16.dp` |
| Source file / line | `packages/ds/src/main/kotlin/com/example/ds/Spacing.kt:12` |
| Resolution explanation | Direct constant `16.dp` assigned to `Spacing.Medium` |
| Confidence | high |

## Proposed diff

Structured diff groups (preview only; not yet written):

```text
Component changes: (none)
Token additions: (none)
Values filled:
  - space.medium: missing → "16.dp"
Values changed: (none)
Potential removals: (none)
```

No registry entries are proposed for deletion.

## Explicit approval

Stop and wait for the user to approve the values-filled group before writing.

- Approvals for additions/changes do not imply approval for removals.
- After approval, apply only the approved change with `apply_patch` on the publisher registry.
- Preserve ids, keys, aliases, categories, metadata, and all other values.

## After registry

Publisher registry after the approved value fill (no entries deleted):

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "aliases": ["PrimaryBtn"],
      "package": "com.example.ds"
    },
    {
      "id": "ds.text-field",
      "symbol": "TextField",
      "package": "com.example.ds"
    }
  ],
  "tokens": [
    {
      "id": "color.primary",
      "key": "Theme.colors.primary",
      "category": "color",
      "aliases": ["AppColors.Primary"]
    },
    {
      "id": "space.medium",
      "key": "Spacing.Medium",
      "category": "spacing",
      "aliases": ["SpacingMd"],
      "value": "16.dp"
    }
  ]
}
```

Confirmation: component count and token count are unchanged; only the approved `value` was added. No registry entries were deleted.

## Validation, sync, and rescan

In the publisher repo, validation succeeds after the approved patch:

```text
$ wax validate
validation passed
```

Return to the app (delegated flow with resolvable upstream):

```text
$ wax sync
updated compose registry from example-ds/compose -> .wax/registries/example-ds/compose.json
$ wax validate
validation passed
$ wax scan
scan output: .wax/out/scan-merged.json
```

## After maintenance result

The fresh post-sync scan uses the approved canonical value and deterministically reclassifies the same observation:

```text
token metrics:
  Confirmed migration candidates: 1
  Possible migration candidates: 0
  Unmatched observations: 0 (informational)
  Unassessed observations: 0 (registry values needed)

confirmed candidate:
  site: packages/app/src/main/kotlin/com/example/app/Checkout.kt:24
  observed value: 16.dp
  suggestion: space.medium / Spacing.Medium / 16.dp
  match: exact
```

Regenerate the report from this fresh result. The metadata-gap row is gone and the exact candidate appears in the confirmed migration table. A failed write or validation would have left the previous registry recoverable and stopped before sync.
