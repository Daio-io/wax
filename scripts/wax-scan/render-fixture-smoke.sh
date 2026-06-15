#!/usr/bin/env bash
# Fixture-driven smoke render for wax-scan HTML template (Task 3).
# Substitutes deterministic placeholders from expected-insights.sample.json
# and writes a self-contained HTML file to the real report output path.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../../skills/wax-scan/scripts/html-escape.sh
source "$ROOT/skills/wax-scan/scripts/html-escape.sh"

FIXTURE="$SCRIPT_DIR/fixtures/expected-insights.sample.json"
TEMPLATE="$ROOT/skills/wax-scan/templates/report.html"
REPO_ROOT="$(git -C "$ROOT" rev-parse --show-toplevel 2>/dev/null || echo "$ROOT")"
OUTPUT="${1:-$REPO_ROOT/.wax/out/report/index.html}"

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
coverage_bar_width="$(awk "BEGIN { printf \"%.0f\", $coverage_ratio * 320 }")"

# Debt proxy: share of usage sites not fully resolved to DS (candidate + unresolved).
debt_ratio="$(awk "BEGIN { n=$candidate+$unresolved; t=$total; if (t>0) print n/t; else print 0 }")"
debt_pct="$(awk "BEGIN { printf \"%.1f%%\", $debt_ratio * 100 }")"
debt_bar_width="$(awk "BEGIN { printf \"%.0f\", $debt_ratio * 320 }")"
debt_score_explanation="${candidate} candidate + ${unresolved} unresolved of ${total} usage sites"

fragmentation_svg="$(python3 - "$FIXTURE" <<'PY'
import html
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)

def esc(value):
    return html.escape(str(value), quote=False)

parts = []
for i, item in enumerate(data.get("fragmentation_candidates", [])[:3]):
    y = i * 32 + 8
    width = item["count"] * 40
    pattern = esc(item["pattern"])
    count = esc(item["count"])
    parts.append(
        f'<text x="0" y="{y}" class="chart-label">{pattern}</text>'
        f'<rect x="80" y="{y - 10}" width="{width}" height="14" rx="3" fill="var(--chart-fill)"/>'
        f'<text x="{width + 88}" y="{y}" class="chart-value">{count}</text>'
    )
print(" ".join(parts))
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

for item in data.get("limits", [])[:5]:
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
    <section class="card${gap_class}" id="${id}">
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
replace health_score "72/100"
replace coverage_percent "$coverage_pct"
replace maturity_level "Emerging"
replace debt_score_proxy "$debt_pct"
replace debt_score_explanation "$debt_score_explanation"
replace debt_bar_width "$debt_bar_width"
replace coverage_bar_width "$coverage_bar_width"
replace resolved_count "$resolved"
replace total_usage_sites "$total"
replace fragmentation_chart_svg "$fragmentation_svg"
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

for token in 'class="card pinned"' 'class="badge badge-' '<svg' 'class="card data-gap"' 'Generated at' 'Source scan:'; do
  if ! grep -q "$token" "$OUTPUT"; then
    echo "FAIL: missing expected token: $token" >&2
    exit 1
  fi
done

echo "PASS: rendered fixture report to $OUTPUT"
echo "Smoke: open offline in a browser (disable network) and verify cards, badges, and SVG charts."
