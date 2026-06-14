# Wax Scan Reference

## Extractor

Script: `skills/wax-scan/scripts/extract-insights.sh`

Input: `.wax/out/scan-merged.json`

Optional second argument: `--baseline <path>` to a prior compatible scan artifact.

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

Compute when baseline schema is compatible:

- Adoption coverage ratio change
- Resolved / candidate / unresolved count changes
- Per-language adoption change when language sets match

Otherwise emit a single limit entry explaining baseline incompatibility.

## HTML template placeholders

Template: `skills/wax-scan/templates/report.html`

| Placeholder | Source |
|-------------|--------|
| `{{generated_at}}` | Insights JSON |
| `{{source_scan}}` | Insights JSON |
| `{{health_score}}` | Agent-synthesized executive summary |
| `{{coverage_percent}}` | Deterministic repo summary |
| `{{maturity_level}}` | Agent-synthesized maturity assessment |
| `{{sections}}` | All analytics sections |
| `{{recommendations}}` | P0–P3 priority recommendations |
| `{{limits}}` | Data-gap entries |
| `{{charts}}` | Inline SVG from deterministic metrics |
