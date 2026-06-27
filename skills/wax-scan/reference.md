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
| `parent_scope_hotspots` | Parent scopes with the highest attributed invocation counts |
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
3. Never concatenate unescaped JSON strings into `{{section_*}}`, `{{limits_html}}`, `{{fragmentation_chart_svg}}`, or narrative placeholders.

Helper:

```bash
printf '%s' "$symbol_name" | skills/wax-scan/scripts/html-escape.sh
```

## HTML template placeholders

Template: `skills/wax-scan/templates/report.html`

Render helper: `scripts/render-wax-scan-fixture-report.sh [--insights PATH] [--repo-name NAME] [OUTPUT]`

The agent copies the template to `.wax/out/report/index.html` and substitutes placeholders. Use deterministic values from extractor JSON where available; synthesize narrative fields with confidence labels.

The template is the approved visual source of truth for the report UI. It uses a warm paper background, soft green adoption areas, beeswax yellow accents, a large adoption hero, a smooth 100% split-area trend chart, ranked project/package bars, ranked non-DS opportunity bars, and secondary diagnostics.

### Page metadata

| Placeholder | Source | Example |
|-------------|--------|---------|
| `{{repo_name}}` | Repository or project name | `my-app` |
| `{{generated_at}}` | Insights JSON `generated_at` (RFC3339) | `2026-06-14T12:00:00Z` |
| `{{source_scan}}` | Insights JSON `source_scan` | `.wax/out/scan-merged.json` |
| `{{schema_version}}` | Insights JSON `schema_version` | `2` |

### Opening adoption hero

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{coverage_percent}}` | Deterministic | `repo_summary.ds_vs_local_ratio` as formatted percent string |
| `{{non_ds_percent}}` | Deterministic | `100 - coverage_percent`, formatted as percent |
| `{{resolved_count}}` | Deterministic | `repo_summary.raw_invocations.resolved` |
| `{{total_usage_sites}}` | Deterministic | `repo_summary.adoption.eligible_invocation_count` |
| `{{adopted_components_count}}` | Deterministic | `repo_summary.registry.used_component_count` |
| `{{total_registry_components}}` | Deterministic | `repo_summary.registry.component_count` |
| `{{registry_resolution_percent}}` | Deterministic | `repo_summary.registry_resolution_ratio` as formatted percent string |
| `{{trend_delta}}` | Baseline or fallback | e.g. `+8 pts`; use `First scan` when no baseline exists |
| `{{trend_context}}` | Baseline or fallback | e.g. `Compared with previous baseline` |
| `{{trend_status}}` | Baseline or fallback | e.g. `Steady improvement` or `History starts here` |

### Visual chart placeholders

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{split_area_chart_svg}}` | Deterministic + template | Smooth 100% split-area SVG. Green lower area = DS share; beeswax line = adoption boundary |
| `{{trend_axis_html}}` | Deterministic + escaped labels | `<span>` labels for trend points |
| `{{project_package_rows_html}}` | Deterministic or inferred | Ranked horizontal rows. Prefer project/package over language |
| `{{migration_opportunity_rows_html}}` | Deterministic + agent | Ranked non-DS opportunities from local symbol usage, fragmentation, or AI-selected candidates |
| `{{visible_limits_html}}` | Insights JSON `limits[]` | Keep visually secondary |
| `{{diagnostics_summary_html}}` | Diagnostics summary | Keep visually secondary |

Omit or render first-scan fallback states when data is missing. Keep charts inline; no external assets or CDN scripts.

### Visual theme

- Background: warm paper gradient using `#f4efe9`, `#f8f2ec`, and white panels
- Accent: beeswax yellow `#d6a117`
- Adoption: soft green `#5f8d4e`, `#8fb17d`, `#dbe8cf`
- Neutral comparison fill: warm sand `#f5edd3`
- Red: reserved for true errors or severity states, not default chart language

### Recommendations

| Placeholder | Source |
|-------------|--------|
| `{{recommendations_html}}` | Agent narrative |

Each item is a `<li>` with priority prefix:

```html
<li><span class="rec-priority">P0</span> Problem, impact, action, benefit.</li>
```

### Analytics section cards

Replace each `{{section_<id>}}` placeholder with a full section card. Build card shells from the trusted patterns below; escape all scan-derived text inside `card-body` before insertion.

| Placeholder | Section title |
|-------------|---------------|
| `{{section_design_system_coverage}}` | Design System Coverage |
| `{{section_design_system_debt}}` | Design System Debt |
| `{{section_custom_component_analysis}}` | Custom Component Analysis |
| `{{section_component_health_analysis}}` | Component Health Analysis |
| `{{section_override_analysis}}` | Override Analysis |
| `{{section_deprecated_component_analysis}}` | Deprecated Component Analysis |
| `{{section_version_adoption}}` | Version Adoption |
| `{{section_fragmentation_analysis}}` | Fragmentation Analysis |
| `{{section_wrapper_proliferation_analysis}}` | Wrapper Proliferation Analysis |
| `{{section_feature_level_coverage}}` | Feature-Level Coverage |
| `{{section_design_system_maturity}}` | Design System Maturity |
| `{{section_missing_component_detection}}` | Missing Component Detection |
| `{{section_missing_variant_detection}}` | Missing Variant Detection |
| `{{section_component_api_pain_signals}}` | Component API Pain Signals |
| `{{section_reuse_analysis}}` | Reuse Analysis |
| `{{section_design_system_influence}}` | Design System Influence |
| `{{section_migration_roi_analysis}}` | Migration ROI Analysis |
| `{{section_migration_readiness}}` | Migration Readiness |
| `{{section_trend_analysis}}` | Trend Analysis |

Section card structure:

```html
<section class="card" id="design-system-coverage">
  <div class="card-header">
    <h2>Design System Coverage</h2>
    <span class="badge badge-medium">medium</span>
  </div>
  <div class="card-body">
    <p>Insight content with confidence labels.</p>
  </div>
</section>
```

For unsupported metrics, use `class="card data-gap"` and `badge-gap`:

```html
<section class="card data-gap" id="override-analysis">
  <div class="card-header">
    <h2>Override Analysis</h2>
    <span class="badge badge-gap">gap</span>
  </div>
  <div class="card-body">
    <p class="data-gap-notice">Data gap: Override rate requires override detection in language packs. Not computed in this scan.</p>
  </div>
</section>
```

Severity badges: `critical`, `high`, `medium`, `low`, or `gap` for data-gap sections.

### Data gaps aggregate

| Placeholder | Source |
|-------------|--------|
| `{{limits_html}}` | Insights JSON `limits[]` |

Each limit as a list item:

```html
<li class="data-gap-notice">Data gap: &lt;metric&gt; requires &lt;missing_capability&gt;. Not computed in this scan.</li>
```

### Manual HTML smoke checklist

After rendering `.wax/out/report/index.html`:

1. Open in a browser with network disabled (offline).
2. Verify warm paper theme, soft green adoption area, and beeswax yellow accent.
3. Verify the hero shows current adoption, usage counts, adopted components, and trend status.
4. Verify the trend chart is a smooth 100% split-area chart, not bars.
5. Verify project/package and non-DS opportunity rows render as ranked horizontal bars.
6. Verify visible limits and diagnostics stay secondary.
