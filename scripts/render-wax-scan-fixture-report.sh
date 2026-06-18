#!/usr/bin/env bash
# Render wax-scan HTML report from insights JSON (repository maintainer verification).
# Substitutes deterministic placeholders from insights JSON and writes a self-contained HTML file.
#
# Usage:
#   render-wax-scan-fixture-report.sh [--insights PATH] [--repo-name NAME] [OUTPUT]
#
# Defaults:
#   --insights  scripts/fixtures/wax-scan/expected-insights.sample.json
#   --repo-name wax-scan fixture
#   OUTPUT      .wax/out/report/index.html (under repo root)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../skills/wax-scan/scripts/html-escape.sh
source "$ROOT/skills/wax-scan/scripts/html-escape.sh"

DEFAULT_FIXTURE="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"
TEMPLATE="$ROOT/skills/wax-scan/templates/report.html"
REPO_ROOT="$(git -C "$ROOT" rev-parse --show-toplevel 2>/dev/null || echo "$ROOT")"

FIXTURE="$DEFAULT_FIXTURE"
REPO_NAME="wax-scan fixture"
OUTPUT=""

usage() {
  echo "Usage: render-wax-scan-fixture-report.sh [--insights PATH] [--repo-name NAME] [OUTPUT]" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --insights)
      FIXTURE="${2:-}"
      [[ -n "$FIXTURE" ]] || usage
      shift 2
      ;;
    --repo-name)
      REPO_NAME="${2:-}"
      [[ -n "$REPO_NAME" ]] || usage
      shift 2
      ;;
    -h | --help)
      usage
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "FAIL: unknown option: $1" >&2
      usage
      ;;
    *)
      if [[ -n "$OUTPUT" ]]; then
        echo "FAIL: unexpected extra argument: $1" >&2
        usage
      fi
      OUTPUT="$1"
      shift
      ;;
  esac
done

if [[ -z "$OUTPUT" ]]; then
  OUTPUT="$REPO_ROOT/.wax/out/report/index.html"
fi

CHART_WIDTH=360
ROW_H=20
BAR_H=10
LABEL_W=96
BAR_X=104
VALUE_GUTTER=28

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
non_ds_pct="$(awk "BEGIN { printf \"%.1f%%\", (1 - $coverage_ratio) * 100 }")"
local_defs="$(jq -r '.repo_summary.local_definition_count' "$FIXTURE")"
ds_vs_local_ratio="$(jq -r '.repo_summary.ds_vs_local_ratio' "$FIXTURE")"
ds_vs_local_pct="$(awk "BEGIN { printf \"%.1f%%\", $ds_vs_local_ratio * 100 }")"
adopted_components_count="$(jq -r '.symbol_rollups.design_system | length' "$FIXTURE")"
total_registry_components="$adopted_components_count"

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
    (esc(summary["total_usage_sites"]), "Usage sites"),
    (f"{ds_count}", "DS symbols"),
    (esc(summary["unresolved_count"]), "Unresolved"),
]

for num, label in kpis:
    print(
        f'<div class="panel kpi"><div class="num">{num}</div>'
        f'<div class="label">{label}</div></div>'
    )
PY
)"

caveat_html='<div class="caveat"><strong>How to read this report.</strong> Wax matches usage sites against the design system registry. <strong>DS vs local</strong> is resolved DS sites divided by resolved DS sites plus local component definitions — a directional signal, not strict UI coverage.</div>'

ds_vs_local_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" "$LABEL_W" "$BAR_X" "$BAR_H" "$ROW_H" "$VALUE_GUTTER" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
label_w = int(sys.argv[3])
bar_x = int(sys.argv[4])
bar_h = int(sys.argv[5])
row_h = int(sys.argv[6])
value_gutter = int(sys.argv[7])
summary = data["repo_summary"]
resolved = summary.get("resolved_count", 0)
local_defs = summary.get("local_definition_count", 0)
max_val = max(resolved, local_defs, 1)
bar_max = chart_width - bar_x - value_gutter
scale = bar_max / max_val
ds_w = max(int(resolved * scale), 1 if resolved else 0)
local_w = max(int(local_defs * scale), 1 if local_defs else 0)
height = row_h * 2 + 8

def esc(value):
    return html.escape(str(value), quote=False)

y1 = row_h - 2
y2 = row_h * 2 - 2
print(
    f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="DS vs local comparison">'
    f'<text x="0" y="{y1 - 4}" class="chart-label">DS</text>'
    f'<rect x="{bar_x}" y="{y1 - bar_h}" width="{ds_w}" height="{bar_h}" rx="2" fill="var(--ds)"/>'
    f'<text x="{bar_x + ds_w + 4}" y="{y1 - 2}" class="chart-value">{esc(resolved)}</text>'
    f'<text x="0" y="{y2 - 4}" class="chart-label">Local</text>'
    f'<rect x="{bar_x}" y="{y2 - bar_h}" width="{local_w}" height="{bar_h}" rx="2" fill="var(--local)"/>'
    f'<text x="{bar_x + local_w + 4}" y="{y2 - 2}" class="chart-value">{esc(local_defs)}</text>'
    f'</svg>'
)
PY
)"

ds_usage_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" "$LABEL_W" "$BAR_X" "$BAR_H" "$ROW_H" "$VALUE_GUTTER" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
label_w = int(sys.argv[3])
bar_x = int(sys.argv[4])
bar_h = int(sys.argv[5])
row_h = int(sys.argv[6])
value_gutter = int(sys.argv[7])
items = sorted(
    data.get("symbol_rollups", {}).get("design_system", []),
    key=lambda x: x.get("count", 0),
    reverse=True,
)[:12]
if not items:
    print('<svg viewBox="0 0 360 24" role="img" aria-label="No DS usage data"><text x="0" y="14" class="chart-label">No design system usage detected</text></svg>')
    sys.exit(0)

def esc(value):
    return html.escape(str(value), quote=False)

max_count = max(item.get("count", 0) for item in items) or 1
bar_max = chart_width - bar_x - value_gutter
height = len(items) * row_h + 8
parts = [f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="Design system component usage">']
for i, item in enumerate(items):
    y = i * row_h + row_h - 4
    count = item.get("count", 0)
    bar_w = max(int(count / max_count * bar_max), 1 if count else 0)
    symbol = esc(item.get("symbol", ""))[:24]
    parts.append(f'<text x="0" y="{y}" class="chart-label">{symbol}</text>')
    parts.append(f'<rect x="{bar_x}" y="{y - bar_h + 2}" width="{bar_w}" height="{bar_h}" rx="2" fill="var(--ds)"/>')
    parts.append(f'<text x="{bar_x + bar_w + 4}" y="{y}" class="chart-value">{esc(count)}</text>')
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

language_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" "$BAR_X" "$BAR_H" "$ROW_H" "$VALUE_GUTTER" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
bar_x = int(sys.argv[3])
bar_h = int(sys.argv[4])
row_h = int(sys.argv[5])
value_gutter = int(sys.argv[6])
langs = data.get("per_language", [])
if not langs:
    print('<svg viewBox="0 0 360 24" role="img" aria-label="No language data"><text x="0" y="14" class="chart-label">No per-language data</text></svg>')
    sys.exit(0)

def esc(value):
    return html.escape(str(value), quote=False)

max_total = max(item.get("usage_site_count", 0) for item in langs) or 1
bar_max = chart_width - bar_x - value_gutter
height = len(langs) * row_h + 8
parts = [f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="Adoption by language">']
for i, item in enumerate(langs):
    y = i * row_h + row_h - 4
    lang = esc(item.get("language_id", ""))
    resolved = item.get("resolved_count", 0)
    candidate = item.get("candidate_count", 0)
    unresolved = item.get("unresolved_count", 0)
    total = item.get("usage_site_count", 0)
    scale = bar_max / max_total
    r_w = max(int(resolved * scale), 1 if resolved else 0)
    c_w = max(int(candidate * scale), 1 if candidate else 0)
    u_w = max(int(unresolved * scale), 1 if unresolved else 0)
    parts.append(f'<text x="0" y="{y}" class="chart-label">{lang}</text>')
    bx = bar_x
    if resolved:
        parts.append(f'<rect x="{bx}" y="{y - bar_h + 2}" width="{r_w}" height="{bar_h}" rx="2" fill="var(--ds)"/>')
        bx += r_w + 1
    if candidate:
        parts.append(f'<rect x="{bx}" y="{y - bar_h + 2}" width="{c_w}" height="{bar_h}" rx="2" fill="var(--local)"/>')
        bx += c_w + 1
    if unresolved:
        parts.append(f'<rect x="{bx}" y="{y - bar_h + 2}" width="{u_w}" height="{bar_h}" rx="2" fill="var(--unresolved)"/>')
    parts.append(f'<text x="{bar_x + r_w + c_w + u_w + 4}" y="{y}" class="chart-value">{esc(total)}</text>')
parts.append("</svg>")
print("".join(parts))
PY
)"

fragmentation_chart_svg="$(python3 - "$FIXTURE" "$CHART_WIDTH" "$BAR_X" "$BAR_H" "$ROW_H" "$VALUE_GUTTER" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

chart_width = int(sys.argv[2])
bar_x = int(sys.argv[3])
bar_h = int(sys.argv[4])
row_h = int(sys.argv[5])
value_gutter = int(sys.argv[6])
items = data.get("fragmentation_candidates", [])
if not items:
    print('<svg viewBox="0 0 360 24" role="img" aria-label="No fragmentation data"><text x="0" y="14" class="chart-label">No fragmentation candidates detected</text></svg>')
    sys.exit(0)

def esc(value):
    return html.escape(str(value), quote=False)

max_count = max(item.get("count", 0) for item in items) or 1
bar_max = chart_width - bar_x - value_gutter
height = len(items) * row_h + 8
parts = [f'<svg viewBox="0 0 {chart_width} {height}" role="img" aria-label="Fragmentation candidates">']
for i, item in enumerate(items):
    y = i * row_h + row_h - 4
    count = item.get("count", 0)
    bar_w = max(int(count / max_count * bar_max), 1 if count else 0)
    pattern = esc(item.get("pattern", ""))
    parts.append(f'<text x="0" y="{y}" class="chart-label">{pattern}</text>')
    parts.append(f'<rect x="{bar_x}" y="{y - bar_h + 2}" width="{bar_w}" height="{bar_h}" rx="2" fill="var(--local)"/>')
    parts.append(f'<text x="{bar_x + bar_w + 4}" y="{y}" class="chart-value">{esc(count)}</text>')
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
        f"{esc(top['count'])} resolved call sites.</li>"
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

split_area_chart_svg="$(python3 - "$coverage_ratio" <<'PY'
import sys

ratio = max(0.0, min(1.0, float(sys.argv[1])))
height = 170
width = 640
boundary = round((1 - ratio) * height, 2)
control = round(boundary + 8, 2)
print(f'''<svg class="trend-svg" viewBox="0 0 {width} {height}" preserveAspectRatio="none" aria-label="Adoption trend">
  <defs>
    <linearGradient id="waxGreenFill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#8fb17d" stop-opacity="0.95"></stop>
      <stop offset="100%" stop-color="#dbe8cf" stop-opacity="0.75"></stop>
    </linearGradient>
    <linearGradient id="waxSandFill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f3dfa0" stop-opacity="0.95"></stop>
      <stop offset="100%" stop-color="#f7f0d8" stop-opacity="0.75"></stop>
    </linearGradient>
  </defs>
  <path d="M0,0 H{width} V{height} H0 Z" fill="url(#waxSandFill)"></path>
  <path d="M0,{boundary} C160,{control} 320,{control} {width},{boundary} L{width},{height} L0,{height} Z" fill="url(#waxGreenFill)"></path>
  <path d="M0,{boundary} C160,{control} 320,{control} {width},{boundary}" fill="none" stroke="#d6a117" stroke-width="5" stroke-linecap="round"></path>
  <path d="M0,{boundary} C160,{control} 320,{control} {width},{boundary}" fill="none" stroke="#5f8d4e" stroke-width="2.2" stroke-linecap="round"></path>
  <line x1="0" y1="132" x2="{width}" y2="132" stroke="#efe6d0" stroke-width="1"></line>
  <line x1="0" y1="86" x2="{width}" y2="86" stroke="#f3ecd8" stroke-width="1"></line>
  <line x1="0" y1="40" x2="{width}" y2="40" stroke="#f6f1e1" stroke-width="1"></line>
</svg>''')
PY
)"

trend_axis_html='<span>Current</span>'
project_package_rows_html="$(python3 - "$coverage_pct" <<'PY'
import html
import sys

coverage = sys.argv[1]
width = coverage.rstrip("%")
print(
    '<div class="mini-row">'
    '<div class="name">Repository</div>'
    f'<div class="track" style="background:#e6efdd;"><div class="fill" style="width:{html.escape(width)}%;background:linear-gradient(90deg,#5f8d4e,#d6a117);"></div></div>'
    f'<div style="text-align:right;font-weight:900;color:#5f8d4e;">{html.escape(coverage)}</div>'
    '</div>'
)
PY
)"

migration_opportunity_rows_html="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

items = sorted(
    data.get("symbol_rollups", {}).get("local", []),
    key=lambda item: item.get("count", 0),
    reverse=True,
)[:4]
if not items:
    print('<div class="issue-row"><div class="name">No local candidates</div><div class="track"><div class="fill" style="width:0%;"></div></div><div class="score">0</div></div>')
    sys.exit(0)

max_count = max(item.get("count", 0) for item in items) or 1
for item in items:
    symbol = html.escape(str(item.get("symbol", "")), quote=False)
    count = int(item.get("count", 0))
    width = max(6, round(count / max_count * 100))
    print(
        '<div class="issue-row">'
        f'<div class="name">{symbol}</div>'
        f'<div class="track"><div class="fill" style="width:{width}%;background:linear-gradient(90deg,#6b9658,#d6a117);"></div></div>'
        f'<div class="score">{count}</div>'
        '</div>'
    )
PY
)"

visible_limits_html="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

limits = data.get("limits", [])[:2]
if not limits:
    print("<p>No visible report limits for this scan.</p>")
else:
    text = "; ".join(
        f"{item.get('metric', 'Metric')} requires {item.get('missing_capability', 'more data')}"
        for item in limits
    )
    print(f"<p>{html.escape(text, quote=False)}</p>")
PY
)"

diagnostics_summary_html="<p>${unresolved} unresolved usage sites, ${candidate} candidate usage sites, and ${local_defs} local definitions. Diagnostics stay secondary so the main screen remains visual and action-oriented.</p>"

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
  "<p><strong>Deterministic:</strong> See Adoption section for DS vs local mix.</p><p>Coverage by feature, screen, route, module, and team is not available from current scan facts.</p>")"

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

replace repo_name "$REPO_NAME"
replace generated_at "$generated_at"
replace source_scan "$source_scan"
replace schema_version "$schema_version"
replace coverage_percent "$coverage_pct"
replace non_ds_percent "$non_ds_pct"
replace resolved_count "$resolved"
replace total_usage_sites "$total"
replace adopted_components_count "$adopted_components_count"
replace total_registry_components "$total_registry_components"
replace trend_delta "First scan"
replace trend_context "History starts with this scan"
replace trend_status "History starts here"
replace split_area_chart_svg "$split_area_chart_svg"
replace trend_axis_html "$trend_axis_html"
replace project_package_rows_html "$project_package_rows_html"
replace migration_opportunity_rows_html "$migration_opportunity_rows_html"
replace visible_limits_html "$visible_limits_html"
replace diagnostics_summary_html "$diagnostics_summary_html"
replace debt_score_proxy "$debt_pct"
replace debt_score_explanation "$debt_score_explanation"
replace debt_bar_width "$debt_bar_width"
replace kpi_grid_html "$kpi_grid_html"
replace caveat_html "$caveat_html"
replace ds_vs_local_chart_svg "$ds_vs_local_chart_svg"
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
replace section_design_system_influence "$(section_card "design-system-influence" "Design System Influence" "medium" "<p><strong>Deterministic:</strong> ${top_ds_symbol} is the most-used DS symbol (${top_ds_count} call sites).</p>")"
replace section_migration_roi_analysis "$(section_card "migration-roi-analysis" "Migration ROI Analysis" "medium" "<p><strong>Inferred (medium confidence):</strong> Consolidating top fragmentation families may reduce maintenance surface.</p>")"
replace section_migration_readiness "$(section_card "migration-readiness" "Migration Readiness" "low" "<p><strong>Inferred (low confidence):</strong> Partial React scan may affect migration readiness estimates.</p>")"
replace section_trend_analysis "$section_trend"

if grep -q '{{' "$OUTPUT"; then
  echo "FAIL: unresolved placeholders remain in $OUTPUT" >&2
  grep '{{' "$OUTPUT" >&2 || true
  exit 1
fi

for token in 'Current adoption' 'Adopted components' 'Adoption over time' 'Adoption by project/package' 'Top non-DS components to tackle' '<svg' 'Visible limits' 'Diagnostics'; do
  if ! grep -q "$token" "$OUTPUT"; then
    echo "FAIL: missing expected token: $token" >&2
    exit 1
  fi
done

echo "PASS: rendered fixture report to $OUTPUT"
echo "Smoke: open offline in a browser (disable network) and verify hero, split-area trend, ranked bars, limits, and diagnostics."
