# Wax Scan Reference

## Extractor

Script: `skills/wax-scan/scripts/extract-insights.sh`

Input: `.wax/out/scan-merged.json`

Optional second argument: `--baseline <path>` to a prior `scan-merged.json` with a compatible `schema_version`.

Output: versioned insights JSON consumed by the agent when rendering terminal and HTML reports.

## Insights JSON fields

| Field | Description |
|-------|-------------|
| `schema_version` | Insights contract version |
| `generated_at` | RFC3339 timestamp |
| `source_scan` | Path to merged scan input |
| `repo_summary` | Repository-level usage and adoption totals |
| `per_language` | Per-language status, adoption %, counts |
| `symbol_rollups.design_system` | DS symbol usage frequency |
| `symbol_rollups.local` | Local component symbol frequency |
| `symbol_rollups.unresolved` | Unresolved usage symbol frequency |
| `fragmentation_candidates` | Symbol families suggesting duplication |
| `limits[]` | Metrics unavailable from current facts |
| `baseline_deltas` | Trend deltas when `--baseline` supplied |

## Limits catalog (v1)

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

Compute when the baseline is a compatible `scan-merged.json`:

- Adoption coverage ratio change
- Resolved / candidate / unresolved count changes
- Per-language adoption change when language sets match

Otherwise emit a single limit entry explaining baseline incompatibility.

## HTML template placeholders

Template: `skills/wax-scan/templates/report.html`

The agent copies the template to `.wax/out/report/index.html` and substitutes placeholders. Use deterministic values from extractor JSON where available; synthesize narrative fields with confidence labels.

### Page metadata

| Placeholder | Source | Example |
|-------------|--------|---------|
| `{{repo_name}}` | Repository or project name | `my-app` |
| `{{generated_at}}` | Insights JSON `generated_at` (RFC3339) | `2026-06-14T12:00:00Z` |
| `{{source_scan}}` | Insights JSON `source_scan` | `.wax/out/scan-merged.json` |
| `{{schema_version}}` | Insights JSON `schema_version` | `1` |

### Executive summary (pinned at top)

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{health_score}}` | Agent-synthesized | e.g. `72/100`; explain weighting when data is sparse |
| `{{coverage_percent}}` | Deterministic | `repo_summary.adoption_coverage_ratio` as percent |
| `{{maturity_level}}` | Agent-synthesized | e.g. `Emerging`, `Established` |
| `{{debt_score_proxy}}` | Agent-synthesized or deterministic | Proxy from unresolved + candidate share |
| `{{debt_score_explanation}}` | Agent narrative | Short evidence line |
| `{{executive_severity_badge}}` | Agent judgment | HTML badge: `critical` / `high` / `medium` / `low` |
| `{{executive_summary_body}}` | Agent narrative | Top wins, top opportunities, major risks |

Badge HTML pattern:

```html
<span class="badge badge-high">high</span>
```

### Inline SVG charts

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{coverage_bar_width}}` | Deterministic | Pixel width 0–320 from `adoption_coverage_ratio` |
| `{{coverage_percent}}` | Deterministic | Formatted percent string |
| `{{resolved_count}}` | Deterministic | `repo_summary.resolved_count` |
| `{{total_usage_sites}}` | Deterministic | `repo_summary.total_usage_sites` |
| `{{debt_bar_width}}` | Deterministic or proxy | Pixel width 0–320 for debt proxy bar |
| `{{fragmentation_chart_svg}}` | Deterministic | SVG `<text>` + `<rect>` rows from `fragmentation_candidates` |

Omit or zero-width bars when data is missing. Keep charts inline; no external assets.

### Recommendations

| Placeholder | Source |
|-------------|--------|
| `{{recommendations_html}}` | Agent narrative |

Each item is a `<li>` with priority prefix:

```html
<li><span class="rec-priority">P0</span> Problem, impact, action, benefit.</li>
```

### Analytics section cards

Replace each `{{section_<id>}}` placeholder with a full section card. Sections follow analytics spec order (executive summary is separate, pinned above charts).

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

### Manual smoke checklist

After rendering `.wax/out/report/index.html`:

1. Open in a browser with network disabled (offline).
2. Verify executive summary card, section cards, and severity badges render.
3. Verify at least one inline SVG chart is visible.
4. Verify `data-gap` sections use muted dashed styling.
5. Verify footer shows `generated_at` and `source_scan`.

Fixture smoke script:

```bash
skills/wax-scan/scripts/render-fixture-smoke.sh
```

Writes `skills/wax-scan/fixtures/report.sample.html` for offline inspection.
