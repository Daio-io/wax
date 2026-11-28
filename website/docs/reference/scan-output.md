---
title: Scan output
sidebar_position: 3
---

# Scan output reference

Wax always attempts to write merged scan facts at
`.wax/out/scan-merged.json`. Additional outputs come from config-v2 `outputs`
and repeatable `wax scan --output FORMAT=PATH` flags. Relative paths resolve
inside the repository; absolute paths require `--allow-absolute-output`.

## Merged facts

The [scan-facts v5 schema](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/scan-facts.schema.json)
defines language metadata, snapshot id, `complete|partial|failed` status,
design-system and local components, `usage_sites`, diagnostics, raw counts,
adoption metrics, token references, and hard-coded style observations.

React includes literal Tailwind arbitrary values in `hardcoded_style_sites`.
Each observation records the value, category, context, and position at the
start of the class, with a parent component when Wax can identify one. These
sites feed token inference alongside inline style observations. Wax skips values
containing `var(`, `theme(`, `env(`, `calc(`, `url(`, or `--` and utility/value
combinations without a mapped style context.

React detects literal ratios in inline `aspectRatio` styles and in Tailwind
utilities such as `aspect-[4/3]`. Compose detects ratios in
`Modifier.aspectRatio(16f / 9f)`, and SwiftUI support includes
`.aspectRatio(16/9, contentMode: .fit)`. Both the category and context are
`aspect_ratio`. Inference requires an exact match after rounding both values
to four decimal places.

In parser-backed packs, a design-system component's `symbol` is its declared
source name. When registry entries share that name, package-tagged entries use
`package#symbol` as their canonical `registry_symbol`. Unique names and the
unscoped entry keep the source name as their canonical key. Resolved and candidate
usage sites link to the component through this key, which also determines its
component fact ID and registry symbol summary ID.

## JSON summary

`--format json-summary` writes one versioned object to stdout. The
`json-summary` artifact uses the same
[scan-summary v1 schema](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/scan-summary.schema.json):
languages, adoption, diagnostics, artifact paths, limits, and optional baseline
deltas.

## Graph data

`graph-data` follows the
[scan-graph v1 schema](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-contract/schemas/scan-graph.schema.json).
It contains normalized language, design-system, local-component, and file nodes
with usage edges and metrics for charts or local report interfaces.

## Markdown and HTML

`markdown` is a review-friendly summary with adoption, changes, diagnostics,
artifact links, and limits. `html` is a self-contained static report. These are
artifacts, while stdout `--format markdown` emits Markdown directly. Stdout
also supports `summary` (default) and `quiet` (merged path only).

A baseline can be a prior JSON summary or merged scan. Artifact files are
generated repository-local state and should normally be uploaded by CI rather
than committed. They never belong in global `~/.wax/` state.
