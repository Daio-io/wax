---
title: wax-refine-docs
sidebar_position: 5
---

# wax-refine-docs

Use `wax-refine-docs` when a design-system maintainer wants Wax to register
richer component details. It improves KDoc, Swift documentation comments, and
JSDoc/TSDoc in the exact shapes the Compose, Swift, and React language packs
read. Those shapes produce summaries, parameter descriptions, enum values,
defaults, usage guidance, examples, and deprecations.

Safety boundary: Edit only source documentation grounded in the code, with structural changes approved separately and no registry writes.

## Prerequisites

- A design-system repository with Compose, SwiftUI, or React component source.
- A `wax` binary on `PATH` or the published npm wrapper, the matching language
  pack, and `jq`.
- A maintainer available to approve edits and answer questions about intent.

## Happy path

Measure what Wax extracts today:

```bash
wax registry discover --language react --root src --dry-run >"$TMPDIR/before.json"
skills/wax-refine-docs/scripts/docs-coverage.sh "$TMPDIR/before.json"
```

The coverage report lists components with the worst coverage first, with issues
such as `missing_documentation`, `undocumented_parameters`,
`missing_guidance`, `missing_example`, and partial extraction caused by field
gaps. The agent then drafts docs from the implementation, shows a grouped
diff, applies the doc-only edits you approve, and reruns the dry run to report
the coverage gain.

## What good docs look like

| Fact | Compose | Swift | React |
| --- | --- | --- | --- |
| Summary | KDoc body | `///` body | Component JSDoc body |
| Parameter docs | `@param` | `- Parameters:` on the `public init` | JSDoc on each props member |
| Guidance | `@guidance` (one line) | `- Guidance:` | `@guidance` (one line) |
| Example | Fenced block in the KDoc | `Example:` and a fenced block | `@example` without fences |

Each language has a golden example and the exact registry details Wax extracts
from it. Every pack's test suite runs those examples, so the documented shapes
stay accurate.

## Failures and guardrails

- Docs must describe actual behavior. The agent asks rather than inventing
  intent, platform support, or accessibility claims.
- Structural fixes, such as adding an explicit Swift `public init` or moving an
  enum into the component's file, need separate approval and must keep the
  public API compatible.
- Doc-only edits never change runtime code. The package's normal build or
  tests run after editing.
- The skill never writes registry files. Publish the improved details through
  [`wax-registry-discover`](./wax-registry-discover.md), and app teams pick
  them up with `wax sync`.

Read the canonical
[`skills/wax-refine-docs/SKILL.md`](https://github.com/Daio-io/wax/blob/main/skills/wax-refine-docs/SKILL.md)
and the [language references](https://github.com/Daio-io/wax/tree/main/skills/wax-refine-docs/reference)
for complete operational detail.
