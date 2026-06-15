#!/usr/bin/env bash
# Fixture-driven smoke render for wax-scan HTML template (repository maintainer verification).
# Substitutes deterministic placeholders from expected-insights.sample.json
# and writes a self-contained HTML file to the real report output path.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../skills/wax-scan/scripts/html-escape.sh
source "$ROOT/skills/wax-scan/scripts/html-escape.sh"

FIXTURE="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"
TEMPLATE="$ROOT/skills/wax-scan/templates/report.html"
REPO_ROOT="$(git -C "$ROOT" rev-parse --show-toplevel 2>/dev/null || echo "$ROOT")"
OUTPUT="${1:-$REPO_ROOT/.wax/out/report/index.html}"

CHART_WIDTH=400

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

if [[ ! -f "$FIXTURE" || ! -f "$TEMPLATE" ]]; then
  echo "FAIL: missing fixture or template" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"

generated_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
source_scan="$(jq -r '.source_scan' "$FIXTURE" | html_escape_stdin)"
schema_version="$(jq -r '.schema_version' "$FIXTURE")"
total="$(jq -r '.repo_summary.total_usage_sites' "$FIXTURE")"
resolved="$(jq -r '.repo_summary.resolved_count' "$FIXTURE")"
candidate="$(jq -r '.repo_summary.candidate_count' "$FIXTURE")"
unresolved="$(jq -r '.repo_summary.unresolved_count' "$FIXTURE")"
coverage_ratio="$(jq -r '.repo_summary.adoption_coverage_ratio' "$FIXTURE")"
coverage_pct="$(awk "BEGIN { printf \"%.1f%%\", $coverage_ratio * 100 }")"
coverage_bar_width="$(awk "BEGIN { printf \"%.0f\", $coverage_ratio * $CHART_WIDTH }")"
local_defs="$(jq -r '.repo_summary.local_definition_count' "$FIXTURE")"
ds_vs_local_ratio="$(jq -r '.repo_summary.ds_vs_local_ratio' "$FIXTURE")"
ds_vs_local_pct="$(awk "BEGIN { printf \"%.1f%%\", $ds_vs_local_ratio * 100 }")"

# Debt proxy: share of usage sites not fully resolved to DS (candidate + unresolved).
debt_ratio="$(awk "BEGIN { n=$candidate+$unresolved; t=$total; if (t>0) print n/t; else print 0 }")"
debt_pct="$(awk "BEGIN { printf \"%.1f%%\", $debt_ratio * 100 }")"
debt_bar_width="$(awk "BEGIN { printf \"%.0f\", $debt_ratio * $CHART_WIDTH }")"
debt_score_explanation="${candidate} candidate + ${unresolved} unresolved of ${total} usage sites"

kpi_grid_html="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

summary = data["repo_summary"]
ds_count = len(data.get("symbol_rollups", {}).get("design_system", []))
local_count = summary.get("local_definition_count", len(data.get("symbol_rollups", {}).get("local", [])))
coverage = summary.get("adoption_coverage_ratio")
coverage_pct = f"{coverage * 100:.1f}%" if coverage is not None else "n/a"
ds_vs_local = summary.get("ds_vs_local_ratio")
ds_vs_local_pct = f"{ds_vs_local * 100:.1f}%" if ds_vs_local is not None else "n/a"

kpis = [
    (ds_vs_local_pct, "DS vs local"),
    (esc(summary["resolved_count"]), "DS resolved sites"),
    (esc(local_count), "Local definitions"),
    (f"{ds_count}", "DS symbols in use"),
    (esc(summary["total_usage_sites"]), "Total usage sites"),
    (esc(summary["unresolved_count"]), "Unresolved sites"),
]

for num, label in kpis:
    print(
        f'<div class="panel kpi"><div class="num">{num}</div>'
        f'<div class="label">{label}</div></div>'
    )
PY
)"

caveat_html='<div class="caveat"><strong>How to read this report.</strong> Wax scans configured language packs for component usage sites and matches them against the design system registry. <strong>DS vs local</strong> compares resolved DS usage sites to local component definitions — the primary directional adoption signal in this report, not strict UI coverage. Inferred insights are labeled with confidence; data-gap sections indicate metrics unavailable from current scan facts.</div>'

ds_vs_local_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
summary = data["repo_summary"]
resolved = summary.get("resolved_count", 0)
local_defs = summary.get("local_definition_count", 0)
max_val = max(resolved, local_defs, 1)
bar_max = chart_width - 120
scale = bar_max / max_val
ds_w = int(resolved * scale)
local_w = int(local_defs * scale)
height = 88

def esc(value):
    return html.escape(str(value), quote=False)

print(
    f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="DS vs local comparison">'
    f'<text x="0" y="18" class="chart-label">DS resolved sites</text>'
    f'<rect x="110" y="6" width="{ds_w}" height="18" rx="3" fill="var(--ds)"/>'
    f'<text x="{110 + ds_w + 8}" y="18" class="chart-value">{esc(resolved)}</text>'
    f'<text x="0" y="58" class="chart-label">Local definitions</text>'
    f'<rect x="110" y="46" width="{local_w}" height="18" rx="3" fill="var(--local)"/>'
    f'<text x="{110 + local_w + 8}" y="58" class="chart-value">{esc(local_defs)}</text>'
    f'</svg>'
)
PY
)"

ds_usage_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
items = sorted(
    data.get("symbol_rollups", {}).get("design_system", []),
    key=lambda x: x.get("count", 0),
    reverse=True,
)
if not items:
    print('<svg viewBox="0 0 400 40" role="img" aria-label="No DS usage data"><text x="0" y="20" class="chart-label">No design system usage detected</text></svg>')
    sys.exit(0)

def esc(value):
    return html.escape(str(value), quote=False)

max_count = max(item.get("count", 0) for item in items) or 1
row_h = 28
label_w = 140
bar_x = label_w + 8
bar_max = chart_width - bar_x - 48
height = len(items) * row_h + 16
parts = [f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="Design system component usage">']
for i, item in enumerate(items):
    y = i * row_h + 20
    count = item.get("count", 0)
    bar_w = int(count / max_count * bar_max)
    symbol = esc(item.get("symbol", ""))
    parts.append(f'<text x="0" y="{y}" class="chart-label">{symbol}</text>')
    parts.append(f'<rect x="{bar_x}" y="{y - 12}" width="{bar_w}" height="16" rx="3" fill="var(--ds)"/>')
    parts.append(f'<text x="{bar_x + bar_w + 6}" y="{y}" class="chart-value">{esc(count)}</text>')
parts.append("</svg>")
print("".join(parts))
PY
)"

ds_symbols_table_html="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

items = sorted(
    data.get("symbol_rollups", {}).get("design_system", []),
    key=lambda x: x.get("count", 0),
    reverse=True,
)
total = sum(item.get("count", 0) for item in items) or 1
rows = []
for item in items:
    symbol = esc(item.get("symbol", ""))
    count = item.get("count", 0)
    share = f"{count / total * 100:.1f}%"
    rows.append(
        f"<tr><td><code>{symbol}</code></td>"
        f'<td class="num">{esc(count)}</td>'
        f"<td class=\"num\">{share}</td></tr>"
    )
if not rows:
    rows.append('<tr><td colspan="3" class="muted">No design system symbols detected</td></tr>')
print(
    "<table><thead><tr><th>Component</th><th>Usages</th><th>Share of DS sites</th></tr></thead>"
    f"<tbody>{''.join(rows)}</tbody></table>"
)
PY
)"

language_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
langs = data.get("per_language", [])
if not langs:
    print('<svg viewBox="0 0 400 40" role="img" aria-label="No language data"><text x="0" y="20" class="chart-label">No per-language data</text></svg>')
    sys.exit(0)

def esc(value):
    return html.escape(str(value), quote=False)

max_total = max(item.get("usage_site_count", 0) for item in langs) or 1
row_h = 36
label_w = 100
bar_x = label_w + 8
bar_max = chart_width - bar_x - 8
height = len(langs) * row_h + 16
parts = [f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="Adoption by language">']
for i, item in enumerate(langs):
    y = i * row_h + 22
    lang = esc(item.get("language_id", ""))
    resolved = item.get("resolved_count", 0)
    candidate = item.get("candidate_count", 0)
    unresolved = item.get("unresolved_count", 0)
    total = item.get("usage_site_count", 0) or 1
    scale = bar_max / max_total
    r_w = int(resolved * scale)
    c_w = int(candidate * scale)
    u_w = int(unresolved * scale)
    parts.append(f'<text x="0" y="{y}" class="chart-label">{lang}</text>')
    bx = bar_x
    if r_w:
        parts.append(f'<rect x="{bx}" y="{y - 12}" width="{r_w}" height="16" rx="2" fill="var(--ds)"/>')
        bx += r_w + 1
    if c_w:
        parts.append(f'<rect x="{bx}" y="{y - 12}" width="{c_w}" height="16" rx="2" fill="var(--local)"/>')
        bx += c_w + 1
    if u_w:
        parts.append(f'<rect x="{bx}" y="{y - 12}" width="{u_w}" height="16" rx="2" fill="var(--unresolved)"/>')
    parts.append(f'<text x="{bar_x + r_w + c_w + u_w + 6}" y="{y}" class="chart-value">{esc(total)} sites</text>')
parts.append("</svg>")
print("".join(parts))
PY
)"

fragmentation_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
items = data.get("fragmentation_candidates", [])
if not items:
    print('<svg viewBox="0 0 400 40" role="img" aria-label="No fragmentation data"><text x="0" y="20" class="chart-label">No fragmentation candidates detected</text></svg>')
    sys.exit(0)

def esc(value):
    return html.escape(str(value), quote=False)

max_count = max(item.get("count", 0) for item in items) or 1
row_h = 32
label_w = 100
bar_x = label_w + 8
bar_max = chart_width - bar_x - 48
height = len(items) * row_h + 16
parts = [f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="Fragmentation candidates">']
for i, item in enumerate(items):
    y = i * row_h + 20
    count = item.get("count", 0)
    bar_w = int(count / max_count * bar_max)
    pattern = esc(item.get("pattern", ""))
    parts.append(f'<text x="0" y="{y}" class="chart-label">{pattern}</text>')
    parts.append(f'<rect x="{bar_x}" y="{y - 12}" width="{bar_w}" height="16" rx="3" fill="var(--local)"/>')
    parts.append(f'<text x="{bar_x + bar_w + 6}" y="{y}" class="chart-value">{esc(count)} symbols</text>')
parts.append("</svg>")
print("".join(parts))
PY
)"

key_findings_html="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

summary = data["repo_summary"]
ds = data.get("symbol_rollups", {}).get("design_system", [])
frag = data.get("fragmentation_candidates", [])
coverage = summary.get("adoption_coverage_ratio")
coverage_pct = f"{coverage * 100:.1f}%" if coverage is not None else "n/a"
ds_vs_local = summary.get("ds_vs_local_ratio")
ds_vs_local_pct = f"{ds_vs_local * 100:.1f}%" if ds_vs_local is not None else "n/a"
top = ds[0] if ds else None
findings = []
if top:
    findings.append(
        f"<li><strong>{esc(top['symbol'])} leads DS usage</strong> — "
        f"{esc(top['count'])} of {esc(summary['resolved_count'])} resolved sites.</li>"
    )
findings.append(
    f"<li><strong>DS vs local is {ds_vs_local_pct}</strong> — "
    f"{esc(summary['resolved_count'])} resolved DS sites vs {esc(summary.get('local_definition_count', 0))} local component definitions.</li>"
)
if frag:
    findings.append(
        f"<li><strong>{esc(len(frag))} fragmentation families detected</strong> — "
        f"review {esc(frag[0]['pattern'])} and similar patterns for consolidation.</li>"
    )
if summary.get("unresolved_count", 0) > 0:
    findings.append(
        f"<li><strong>{esc(summary['unresolved_count'])} unresolved sites</strong> — "
        "investigate registry gaps or import resolution issues.</li>"
    )
print(f"<ul>{''.join(findings)}</ul>")
PY
)"

limits_html="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

for item in data.get("limits", []):
    metric = esc(item["metric"])
    missing = esc(item["missing_capability"])
    print(
        f'<li class="data-gap-notice">Data gap: {metric} requires {missing}. Not computed in this scan.</li>'
    )
PY
)"

fragmentation_items="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

for item in data.get("fragmentation_candidates", []):
    symbols = ", ".join(esc(s) for s in item.get("symbols", []))
    print(f"<li>{esc(item['pattern'])}: {symbols} ({esc(item['count'])} symbols)</li>")
PY
)"

top_ds_symbol="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

ds = data.get("symbol_rollups", {}).get("design_system", [])
print(esc(ds[0].get("symbol", "n/a")) if ds else "n/a")
PY
)"

top_ds_count="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

ds = data.get("symbol_rollups", {}).get("design_system", [])
print(esc(ds[0].get("count", 0)) if ds else "0")
PY
)"

local_count="$(jq -r '.symbol_rollups.local | length' "$FIXTURE")"
fragmentation_count="$(jq -r '.fragmentation_candidates | length' "$FIXTURE")"

section_card() {
  local id="$1" title="$2" severity="$3" body="$4" gap="${5:-false}"
  local gap_class=""
  local badge_class="badge-$severity"
  if [[ "$gap" == "true" ]]; then
    gap_class=' data-gap'
    badge_class="badge-gap"
  fi
  cat <<EOF
    <section class="panel card${gap_class}" id="${id}">
      <div class="card-header">
        <h2>${title}</h2>
        <span class="badge ${badge_class}">${severity}</span>
      </div>
      <div class="card-body">
        ${body}
      </div>
    </section>
EOF
}

section_coverage="$(section_card "design-system-coverage" "Design System Coverage" "medium" \
  "<p><strong>Deterministic:</strong> Overall adoption coverage is ${coverage_pct} (${resolved} resolved of ${total} usage sites).</p><p>Coverage by feature, screen, route, module, and team is not available from current scan facts.</p>")"

section_fragmentation="$(section_card "fragmentation-analysis" "Fragmentation Analysis" "high" \
  "<p><strong>Deterministic:</strong> Found ${fragmentation_count} symbol families suggesting duplication.</p><ul>${fragmentation_items}</ul>")"

section_trend="$(section_card "trend-analysis" "Trend Analysis" "gap" \
  "<p class=\"data-gap-notice\">Data gap: Trends require a prior scan baseline via --baseline. Not computed in this scan.</p>" "true")"

recommendations_html='<li><span class="rec-priority">P1</span> Consolidate Button variants flagged in fragmentation analysis.</li>
          <li><span class="rec-priority">P2</span> Improve React scan completeness (partial language status).</li>'

executive_body='<p><strong>Top wins:</strong> DS symbols in active use across compose and react.</p>
        <p><strong>Top opportunities:</strong> Reduce unresolved usage sites and consolidate fragmented button/modal families.</p>'

cp "$TEMPLATE" "$OUTPUT"

replace() {
  local key="$1" val="$2"
  python3 - "$OUTPUT" "$key" "$val" <<'PY'
import sys
path, key, val = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path, encoding="utf-8").read()
text = text.replace("{{" + key + "}}", val)
open(path, "w", encoding="utf-8").write(text)
PY
}

replace repo_name "wax-scan fixture"
replace generated_at "$generated_at"
replace source_scan "$source_scan"
replace schema_version "$schema_version"
replace coverage_percent "$coverage_pct"
replace debt_score_proxy "$debt_pct"
replace debt_score_explanation "$debt_score_explanation"
replace debt_bar_width "$debt_bar_width"
replace coverage_bar_width "$coverage_bar_width"
replace resolved_count "$resolved"
replace total_usage_sites "$total"
replace kpi_grid_html "$kpi_grid_html"
replace caveat_html "$caveat_html"
replace ds_vs_local_chart_svg "$ds_vs_local_chart_svg"
replace ds_vs_local_percent "$ds_vs_local_pct"
replace local_definition_count "$local_defs"
replace ds_usage_chart_svg "$ds_usage_chart_svg"
replace ds_symbols_table_html "$ds_symbols_table_html"
replace language_chart_svg "$language_chart_svg"
replace fragmentation_chart_svg "$fragmentation_chart_svg"
replace key_findings_html "$key_findings_html"
replace executive_severity_badge '<span class="badge badge-medium">medium</span>'
replace executive_summary_body "$executive_body"
replace recommendations_html "$recommendations_html"
replace limits_html "$limits_html"
replace section_design_system_coverage "$section_coverage"
replace section_design_system_debt "$(section_card "design-system-debt" "Design System Debt" "high" "<p><strong>Inferred (medium confidence):</strong> ${unresolved} unresolved and ${candidate} candidate usage sites indicate adoption debt.</p>")"
replace section_custom_component_analysis "$(section_card "custom-component-analysis" "Custom Component Analysis" "medium" "<p><strong>Deterministic:</strong> ${local_count} local component symbols detected.</p>")"
replace section_component_health_analysis "$(section_card "component-health-analysis" "Component Health Analysis" "low" "<p><strong>Deterministic:</strong> Top DS symbol: ${top_ds_symbol} (${top_ds_count} uses).</p>")"
replace section_override_analysis "$(section_card "override-analysis" "Override Analysis" "gap" "<p class=\"data-gap-notice\">Data gap: Override rate requires override detection in language packs. Not computed in this scan.</p>" "true")"
replace section_deprecated_component_analysis "$(section_card "deprecated-component-analysis" "Deprecated Component Analysis" "gap" "<p class=\"data-gap-notice\">Data gap: Deprecated usage requires deprecation metadata in registry or facts. Not computed in this scan.</p>" "true")"
replace section_version_adoption "$(section_card "version-adoption" "Version Adoption" "gap" "<p class=\"data-gap-notice\">Data gap: Version adoption requires DS package version facts. Not computed in this scan.</p>" "true")"
replace section_fragmentation_analysis "$section_fragmentation"
replace section_wrapper_proliferation_analysis "$(section_card "wrapper-proliferation-analysis" "Wrapper Proliferation Analysis" "gap" "<p class=\"data-gap-notice\">Data gap: Wrapper proliferation requires composition/wrapper edges in facts. Not computed in this scan.</p>" "true")"
replace section_feature_level_coverage "$(section_card "feature-level-coverage" "Feature-Level Coverage" "gap" "<p class=\"data-gap-notice\">Data gap: Feature-level coverage requires feature/module attribution. Not computed in this scan.</p>" "true")"
replace section_design_system_maturity "$(section_card "design-system-maturity" "Design System Maturity" "medium" "<p><strong>Inferred (medium confidence):</strong> Multi-language adoption with partial React scan suggests emerging maturity.</p>")"
replace section_missing_component_detection "$(section_card "missing-component-detection" "Missing Component Detection" "low" "<p><strong>Inferred (low confidence):</strong> Review unresolved symbols for missing DS capabilities.</p>")"
replace section_missing_variant_detection "$(section_card "missing-variant-detection" "Missing Variant Detection" "gap" "<p class=\"data-gap-notice\">Data gap: Variant coverage requires registry variant metadata. Not computed in this scan.</p>" "true")"
replace section_component_api_pain_signals "$(section_card "component-api-pain-signals" "Component API Pain Signals" "gap" "<p class=\"data-gap-notice\">Data gap: API pain signals require usage telemetry beyond symbol counts. Not computed in this scan.</p>" "true")"
replace section_reuse_analysis "$(section_card "reuse-analysis" "Reuse Analysis" "low" "<p><strong>Deterministic:</strong> DS symbol reuse varies; Button leads usage frequency.</p>")"
replace section_design_system_influence "$(section_card "design-system-influence" "Design System Influence" "medium" "<p><strong>Inferred (medium confidence):</strong> DS symbols account for ${coverage_pct} of classified usage sites.</p>")"
replace section_migration_roi_analysis "$(section_card "migration-roi-analysis" "Migration ROI Analysis" "medium" "<p><strong>Inferred (medium confidence):</strong> Consolidating top fragmentation families may reduce maintenance surface.</p>")"
replace section_migration_readiness "$(section_card "migration-readiness" "Migration Readiness" "low" "<p><strong>Inferred (low confidence):</strong> Partial React scan may affect migration readiness estimates.</p>")"
replace section_trend_analysis "$section_trend"

if grep -q '{{' "$OUTPUT"; then
  echo "FAIL: unresolved placeholders remain in $OUTPUT" >&2
  grep '{{' "$OUTPUT" >&2 || true
  exit 1
fi

for token in 'class="card pinned"' 'class="badge badge-' '<svg' 'card data-gap' 'Generated at' 'Source scan:' 'class="panel kpi"' 'DS vs local'; do
  if ! grep -q "$token" "$OUTPUT"; then
    echo "FAIL: missing expected token: $token" >&2
    exit 1
  fi
done

echo "PASS: rendered fixture report to $OUTPUT"
echo "Smoke: open offline in a browser (disable network) and verify KPI cards, charts, tables, and section panels."
