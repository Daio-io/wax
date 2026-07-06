# Wax Scan Reference

## Extractor

Script: `skills/wax-scan/scripts/extract-insights.sh`

Input: `.wax/out/scan-merged.json`

Optional second argument: `--baseline <path>` to a prior `scan-merged.json` with a compatible `schema_version`.

Output: versioned insights JSON consumed by the agent when rendering terminal and HTML reports.

## Insights JSON fields

| Field | Description |
|-------|-------------|
| `schema_version` | Insights contract version (`2` for Adoption Metrics v2) |
| `generated_at` | RFC3339 timestamp |
| `source_scan` | Path to merged scan input |
| `repo_summary` | Repository-level DS-vs-local coverage, invocation adoption, registry resolution, raw invocation counters, local definitions, and parent-scope totals |
| `per_language` | Per-language status, DS-vs-local coverage, invocation adoption, registry resolution, and v2 count groups |
| `symbol_rollups.design_system` | DS symbol usage frequency |
| `symbol_rollups.candidate` | Candidate design-system symbol frequency, reported separately from confirmed design-system usage |
| `symbol_rollups.local` | Local invocation symbol frequency |
| `symbol_rollups.unresolved` | Unresolved invocation symbol frequency |
| `top_local_symbols` | Top local rows from `symbol_usage_summary[]` |
| `top_unresolved_symbols` | Top unresolved rows from `symbol_usage_summary[]` |
| `unused_registry_components` | Registry components with no resolved usage in the current scan |
| `parent_scope_hotspots` | Parent scopes with raw, resolved, local, candidate, and unresolved invocation counts |
| `fragmentation_candidates` | Symbol families suggesting duplication |
| `limits[]` | Metrics unavailable from current facts |
| `baseline_deltas` | Trend deltas when `--baseline` supplied |

## Limits catalog (v2)

Emit a `limits[]` entry when the metric is not supported by current `ScanFacts`:

| Metric | Missing capability |
|--------|-------------------|
| Coverage by feature/screen/route/module/team | Reporting boundary metadata in usage sites |
| Override rate / override patterns | Override detection in language packs |
| Deprecated usage | Deprecation metadata in registry or facts |
| Version adoption / upgrade lag | DS package version facts |
| Wrapper proliferation | Composition/wrapper edges in facts |
| Feature-level coverage | Feature/module attribution |
| LOC reduction estimates | Source line metrics beyond usage sites |

## Confidence labeling

| Label | When to use |
|-------|-------------|
| **Deterministic** | Value comes directly from extractor JSON or `scan-merged.json` |
| **Inferred (medium confidence)** | Pattern heuristic with multiple supporting occurrences |
| **Inferred (low confidence)** | Weak naming or sparse evidence; always include evidence count |

## Data-gap block

```text
Data gap: <metric> requires <missing capability>. Not computed in this scan.
```

## Baseline deltas (when `--baseline` provided)

Compute when the baseline is a compatible v2 `scan-merged.json`:

- UI invocation adoption ratio change
- DS-vs-local ratio change
- Registry resolution ratio change
- Raw invocation count changes (`resolved`, `local`, `candidate`, `unresolved`)
- Symbol summary changes by `symbol_id` (`raw_invocation_count`, `file_count`, `parent_scope_count`)
- Parent-scope total change
- Per-language deltas when language sets match

If the baseline is schema v1, emit a single limit entry explaining the compatibility data gap instead of mixing v1 and v2 denominators.

## HTML escaping

Scan-derived text is untrusted HTML input. Repository-controlled symbol names, limit messages, and paths can contain markup.

Before substituting any value from `scan-merged.json` or insights JSON into HTML or SVG text nodes:

1. Run the value through `skills/wax-scan/scripts/html-escape.sh`.
2. Treat only intentional template snippets (card shells, badge markup) as trusted raw HTML.
3. Never concatenate unescaped JSON strings into chart/table placeholders or narrative placeholders.

Helper:

```bash
printf '%s' "$symbol_name" | skills/wax-scan/scripts/html-escape.sh
```

## HTML template placeholders

Template: `skills/wax-scan/templates/report.html`

Render helper: `scripts/render-wax-scan-fixture-report.sh [--insights PATH] [--repo-name NAME] [OUTPUT]`

The renderer copies the template to `.wax/out/report/index.html`, inlines the Wax logo SVG into `{{logo_svg}}`, and substitutes the report placeholders directly. Use deterministic values from extractor JSON where available and keep diagnostics secondary to the migration story.

The template is the approved visual source of truth for the report UI. It uses the dark dashboard shell, wax-yellow accents, inline SVG charts, inventory tables, and a compact findings section.

### Page metadata

| Placeholder | Source | Example |
|-------------|--------|---------|
| `{{repo_name}}` | Repository or project name | `my-app` |
| `{{generated_at}}` | Insights JSON `generated_at` (RFC3339) | `2026-06-14T12:00:00Z` |
| `{{source_scan}}` | Insights JSON `source_scan` | `.wax/out/scan-merged.json` |
| `{{schema_version}}` | Insights JSON `schema_version` | `2` |

### Opening KPI band

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{resolved_count}}` | Deterministic | `repo_summary.raw_invocations.resolved` |
| `{{adopted_components_count}}` | Deterministic | `repo_summary.registry.used_component_count` |
| `{{total_registry_components}}` | Deterministic | `repo_summary.registry.component_count` |
| `{{invocation_adoption_percent}}` | Deterministic | Still used as a template placeholder name, but populate it with `repo_summary.ds_vs_local_ratio` |
| `{{raw_invocation_total}}` | Deterministic | `repo_summary.raw_invocations.total` |
| `{{registry_resolution_percent}}` | Deterministic | `repo_summary.registry_resolution_ratio` |
| `{{local_definition_count}}` | Deterministic | `repo_summary.definitions.local_definition_count` |
| `{{unresolved_count}}` | Deterministic | `repo_summary.raw_invocations.unresolved` |
| `{{caveat_html}}` | Renderer-built HTML | Explain DS-vs-local headline semantics and keep unresolved counts contextual |

Do not render UI invocation adoption as a primary KPI in the HTML report. Keep unresolved counts and registry resolution as supporting context.

### Visual theme

- Background: black shell using `#000000` and `#0d0d0d`
- Accent: bumblebee yellow `#FCC457`
- Adoption: wax yellow and warm contrast fills
- Neutral comparison fill: dark panel borders and muted gray text
- Red: reserved for true errors or severity states, not default chart language

### Section placeholders

| Placeholder | Notes |
|-------------|-------|
| `{{logo_svg}}` | Trusted inline Wax logo SVG loaded from `skills/wax-scan/assets/wax-logo-icon.svg` |
| `{{ds_usage_chart_svg}}` | Top DS usage chart |
| `{{ds_symbols_table_html}}` | DS usage inventory table |
| `{{unused_components_table_html}}` | Named unused registry components |
| `{{parent_scope_chart_svg}}` | Parent-scope adoption chart |
| `{{parent_scope_table_html}}` | Parent-scope counts table |
| `{{adoption_gaps_chart_svg}}` | Multi-language DS-vs-local chart |
| `{{adoption_gaps_table_html}}` | Multi-language DS-vs-local table |
| `{{duplicate_components_table_html}}` | Exact-name duplicate table |
| `{{migration_candidates_table_html}}` | Local-only migration queue |
| `{{key_findings_html}}` | Deterministic findings list |

### Manual HTML smoke checklist

After rendering `.wax/out/report/index.html`:

1. Open in a browser with network disabled (offline).
2. Verify the KPI band uses the dark shell and still labels DS vs local UI coverage correctly.
3. Verify unused registry symbols are named when present.
4. Verify migration candidates exclude unresolved symbols from the main table.
5. Verify empty sections are hidden rather than replaced with placeholder copy.
6. Verify unresolved template placeholders do not remain in the output.
