#!/usr/bin/env bash
# Render wax-scan HTML report from insights JSON (repository maintainer verification).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_FIXTURE="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"
TEMPLATE="$ROOT/skills/wax-scan/templates/report.html"
LOGO="$ROOT/skills/wax-scan/assets/wax-logo-icon.svg"
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
    -h|--help)
      usage
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

if ! command -v python3 >/dev/null 2>&1; then
  echo "FAIL: python3 is required" >&2
  exit 1
fi

if [[ ! -f "$FIXTURE" || ! -f "$TEMPLATE" || ! -f "$LOGO" ]]; then
  echo "FAIL: missing fixture, template, or logo" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"

python3 - "$FIXTURE" "$TEMPLATE" "$LOGO" "$OUTPUT" "$REPO_NAME" <<'PY'
import html
import json
import pathlib
import re
import sys

fixture_path = pathlib.Path(sys.argv[1])
template_path = pathlib.Path(sys.argv[2])
logo_path = pathlib.Path(sys.argv[3])
output_path = pathlib.Path(sys.argv[4])
repo_name = sys.argv[5]

data = json.loads(fixture_path.read_text(encoding="utf-8"))
template = template_path.read_text(encoding="utf-8")
logo_svg = logo_path.read_text(encoding="utf-8").strip()

CHART_WIDTH = 360
ROW_H = 20
BAR_H = 10
BAR_X = 104
VALUE_GUTTER = 28


def esc(value):
    if value is None:
        return ""
    return html.escape(str(value), quote=False)


def pct(value):
    if value is None:
        return "n/a"
    return f"{value * 100:.1f}%"


def num(value):
    return f"{int(value):,}"


def svg_message(label):
    return (
        '<svg viewBox="0 0 360 24" role="img" aria-label="No data">'
        f'<text x="0" y="14" class="chart-label">{esc(label)}</text>'
        "</svg>"
    )


def bar_chart(items, value_key, aria_label, fill_key, label_key="symbol"):
    if not items:
        return svg_message("No data available for this chart")
    max_count = max(float(item.get(value_key, 0) or 0) for item in items) or 1
    bar_max = CHART_WIDTH - BAR_X - VALUE_GUTTER
    height = len(items) * ROW_H + 8
    parts = [f'<svg viewBox="0 0 {CHART_WIDTH} {height}" role="img" aria-label="{esc(aria_label)}">']
    for i, item in enumerate(items):
        y = i * ROW_H + ROW_H - 4
        count = float(item.get(value_key, 0) or 0)
        bar_w = max(int(count / max_count * bar_max), 1 if count else 0)
        label = esc(item.get(label_key, ""))[:28]
        parts.append(f'<text x="0" y="{y}" class="chart-label">{label}</text>')
        parts.append(
            f'<rect x="{BAR_X}" y="{y - BAR_H + 2}" width="{bar_w}" height="{BAR_H}" rx="2" fill="{fill_key}"/>'
        )
        parts.append(f'<text x="{BAR_X + bar_w + 4}" y="{y}" class="chart-value">{esc(int(count))}</text>')
    parts.append("</svg>")
    return "".join(parts)


summary = data["repo_summary"]
raw = summary["raw_invocations"]
definitions = summary["definitions"]
registry = summary["registry"]

ds_vs_local_pct = pct(summary.get("ds_vs_local_ratio"))
registry_resolution_pct = pct(summary.get("registry_resolution_ratio"))

ds_symbols = sorted(
    data.get("symbol_rollups", {}).get("design_system", []),
    key=lambda item: item.get("count", 0),
    reverse=True,
)
ds_usage_chart_svg = bar_chart(
    ds_symbols[:12],
    "count",
    "Design system component usage",
    "var(--ds)",
)

total_ds = sum(int(item.get("count", 0) or 0) for item in ds_symbols) or 1
ds_rows = []
for item in ds_symbols:
    count = int(item.get("count", 0) or 0)
    share = f"{(count / total_ds) * 100:.1f}%"
    ds_rows.append(
        "<tr>"
        f"<td><code>{esc(item.get('symbol', ''))}</code></td>"
        f'<td class="num">{num(count)}</td>'
        f'<td class="num">{share}</td>'
        "</tr>"
    )
ds_table_rows = "".join(ds_rows) or '<tr><td colspan="3" class="muted">No design system symbols detected</td></tr>'
ds_symbols_table_html = (
    "<table><thead><tr><th>Component</th><th>Usages</th><th>Share of DS sites</th></tr></thead>"
    f"<tbody>{ds_table_rows}</tbody></table>"
)

unused_registry_components = data.get("unused_registry_components", [])
unused_registry_count = len(unused_registry_components)
unused_rows = []
for item in unused_registry_components:
    action = "Review for docs promotion or possible deprecation."
    unused_rows.append(
        "<tr>"
        f"<td><code>{esc(item.get('symbol', ''))}</code></td>"
        f"<td>{esc(item.get('package') or '—')}</td>"
        f"<td>{esc(action)}</td>"
        "</tr>"
    )
unused_components_table_html = (
    "<table><thead><tr><th>Component</th><th>Package</th><th>Suggested action</th></tr></thead>"
    f"<tbody>{''.join(unused_rows)}</tbody></table>"
)

visible_hotspots = [
    item for item in data.get("parent_scope_hotspots", [])
    if int(item.get("resolved_raw_invocation_count", 0) or 0) > 0
    or int(item.get("local_raw_invocation_count", 0) or 0) > 0
]

if visible_hotspots:
    max_total = max(int(item.get("raw_invocation_count", 0) or 0) for item in visible_hotspots) or 1
    bar_max = CHART_WIDTH - BAR_X - VALUE_GUTTER
    height = len(visible_hotspots) * ROW_H + 8
    parts = [f'<svg viewBox="0 0 {CHART_WIDTH} {height}" role="img" aria-label="Adoption by area">']
    hotspot_rows = []
    for i, item in enumerate(visible_hotspots):
        y = i * ROW_H + ROW_H - 4
        label = esc(item.get("symbol") or item.get("parent_id") or "Scope")
        resolved = int(item.get("resolved_raw_invocation_count", 0) or 0)
        local = int(item.get("local_raw_invocation_count", 0) or 0)
        unresolved = int(item.get("unresolved_raw_invocation_count", 0) or 0)
        total = int(item.get("raw_invocation_count", 0) or 0)
        resolved_w = max(int(resolved / max_total * bar_max), 1 if resolved else 0)
        remaining = max(local + unresolved, 0)
        remaining_w = max(int(remaining / max_total * bar_max), 1 if remaining else 0)
        parts.append(f'<text x="0" y="{y}" class="chart-label">{label[:28]}</text>')
        parts.append(
            f'<rect x="{BAR_X}" y="{y - BAR_H + 2}" width="{resolved_w}" height="{BAR_H}" rx="2" fill="var(--wax)"/>'
        )
        if remaining_w:
            parts.append(
                f'<rect x="{BAR_X + resolved_w + 1}" y="{y - BAR_H + 2}" width="{remaining_w}" height="{BAR_H}" rx="2" fill="var(--wax-soft)"/>'
            )
        parts.append(f'<text x="{BAR_X + resolved_w + remaining_w + 5}" y="{y}" class="chart-value">{num(total)}</text>')
        hotspot_rows.append(
            "<tr>"
            f"<td><code>{label}</code></td>"
            f'<td class="num">{num(resolved)}</td>'
            f'<td class="num">{num(local)}</td>'
            f'<td class="num">{num(unresolved)}</td>'
            f'<td class="num">{num(total)}</td>'
            "</tr>"
        )
    parts.append("</svg>")
    parent_scope_chart_svg = "".join(parts)
    parent_scope_table_html = (
        "<table><thead><tr><th>Area</th><th>DS</th><th>Local</th><th>Unresolved</th><th>Total</th></tr></thead>"
        f"<tbody>{''.join(hotspot_rows)}</tbody></table>"
    )
else:
    parent_scope_chart_svg = svg_message("No parent-scope adoption data in this scan")
    parent_scope_table_html = ""

per_language = sorted(data.get("per_language", []), key=lambda item: item.get("ds_vs_local_ratio") or 0)
if per_language:
    bar_max = CHART_WIDTH - BAR_X - VALUE_GUTTER
    height = len(per_language) * ROW_H + 8
    parts = [f'<svg viewBox="0 0 {CHART_WIDTH} {height}" role="img" aria-label="Adoption gaps">']
    gap_rows = []
    for i, item in enumerate(per_language):
        y = i * ROW_H + ROW_H - 4
        ratio = float(item.get("ds_vs_local_ratio", 0) or 0)
        width = max(int(ratio * bar_max), 1 if ratio > 0 else 0)
        label = esc(item.get("language_id", ""))
        color = "var(--high)" if ratio < 0.34 else "var(--medium)" if ratio < 0.67 else "var(--low)"
        raw_invocations = item.get("raw_invocations", {})
        parts.append(f'<text x="0" y="{y}" class="chart-label">{label}</text>')
        parts.append(
            f'<rect x="{BAR_X}" y="{y - BAR_H + 2}" width="{width}" height="{BAR_H}" rx="2" fill="{color}"/>'
        )
        parts.append(f'<text x="{BAR_X + width + 4}" y="{y}" class="chart-value">{ratio * 100:.1f}%</text>')
        gap_rows.append(
            "<tr>"
            f"<td><code>{label}</code></td>"
            f'<td class="num">{num(raw_invocations.get("resolved", 0))}</td>'
            f'<td class="num">{num(raw_invocations.get("local", 0))}</td>'
            f'<td class="num">{ratio * 100:.1f}%</td>'
            "</tr>"
        )
    parts.append("</svg>")
    adoption_gaps_chart_svg = "".join(parts)
    adoption_gaps_table_html = (
        "<table><thead><tr><th>Area</th><th>Design system</th><th>Local</th><th>Adoption</th></tr></thead>"
        f"<tbody>{''.join(gap_rows)}</tbody></table>"
    )
else:
    adoption_gaps_chart_svg = svg_message("No per-language adoption data")
    adoption_gaps_table_html = ""

ds_names = {item.get("symbol") for item in ds_symbols}
duplicate_rows = []
for item in data.get("top_local_symbols", []):
    symbol = item.get("symbol", "")
    if symbol in ds_names:
        definition = item.get("local_definition_id") or item.get("symbol_id") or "local definition"
        duplicate_rows.append(
            "<tr>"
            f"<td><code>{esc(symbol)}</code></td>"
            f"<td><code>{esc(definition)}</code></td>"
            "</tr>"
        )
duplicate_components_table_html = (
    "<table><thead><tr><th>Symbol</th><th>Definition</th></tr></thead>"
    f"<tbody>{''.join(duplicate_rows)}</tbody></table>"
)

fragmentation_lookup = {}
for family in data.get("fragmentation_candidates", []):
    for symbol in family.get("symbols", []):
        fragmentation_lookup[symbol] = family.get("pattern")

scope_lookup = {}
for item in data.get("top_local_symbols", []):
    scopes = item.get("parent_scopes") or []
    if scopes:
        scope_lookup[item.get("symbol")] = scopes[0].get("symbol")

migration_rows = []
for item in data.get("symbol_rollups", {}).get("local", []):
    symbol = item.get("symbol", "")
    scope = scope_lookup.get(symbol, "Scope unavailable")
    priority = "medium" if int(item.get("count", 0) or 0) > 0 else "low"
    target = fragmentation_lookup.get(symbol) or "Review registry fit"
    rationale = "Repeated local invocation in the current scan."
    if symbol in fragmentation_lookup:
        priority = "high"
        rationale = f"Appears in fragmentation family {fragmentation_lookup[symbol]}, suggesting consolidation opportunity."
    migration_rows.append(
        "<tr>"
        f"<td><code>{esc(symbol)}</code></td>"
        f"<td>{esc(scope)}</td>"
        f"<td>{esc(target)}</td>"
        f'<td><span class="pill {priority}">{priority}</span></td>'
        f"<td>{esc(rationale)}</td>"
        "</tr>"
    )
migration_candidates_table_html = (
    "<table><thead><tr><th>Local component</th><th>Module</th><th>Design system target</th><th>Priority</th><th>Rationale</th></tr></thead>"
    f"<tbody>{''.join(migration_rows)}</tbody></table>"
)

CONFIDENCE_LABELS = {
    "very_high": "very high",
    "high": "high",
    "medium": "medium",
    "low": "low",
}


def confidence_label(value):
    if not value:
        return "—"
    return CONFIDENCE_LABELS.get(value, value)


def evidence_label(value):
    return str(value).replace("_", " ")


def location_label(row):
    location = row.get("location") or {}
    file = location.get("file", "")
    line = location.get("line", "")
    if not file:
        return "—"
    return f"{file}:{line}" if line != "" else file


def suggestion_field(row, key, formatter=str):
    suggestions = row.get("suggestions") or []
    values = [formatter(item.get(key)) for item in suggestions if item.get(key) is not None]
    return ", ".join(values) if values else "—"


def distance_label(row):
    suggestions = row.get("suggestions") or []
    values = [item.get("distance") for item in suggestions if item.get("distance") is not None]
    if not values:
        return "—"
    return ", ".join(f"{value:g}" for value in values)


def evidence_cell(row):
    evidence = row.get("evidence") or []
    return ", ".join(evidence_label(item) for item in evidence) if evidence else "—"


def token_candidate_rows(rows):
    out = []
    for row in rows:
        out.append(
            "<tr>"
            f"<td>{esc(location_label(row))}</td>"
            f"<td>{esc(row.get('context', ''))}</td>"
            f"<td><code>{esc(row.get('value', ''))}</code></td>"
            f"<td><code>{esc(suggestion_field(row, 'token_key'))}</code></td>"
            f"<td><code>{esc(suggestion_field(row, 'canonical_value'))}</code></td>"
            f'<td class="num">{esc(distance_label(row))}</td>'
            f"<td>{esc(confidence_label(row.get('confidence')))}</td>"
            f"<td>{esc(evidence_cell(row))}</td>"
            "</tr>"
        )
    return "".join(out)


token_inference = data.get("token_inference", {})
confirmed_candidates = token_inference.get("confirmed_candidates", [])
possible_candidates = token_inference.get("possible_candidates", [])
unassessed_observations = token_inference.get("unassessed_observations", [])
unmatched_observations = token_inference.get("unmatched_observations", [])
token_summary = token_inference.get("summary", {})
unassessed_count = int(
    token_summary.get("unassessed_observation_count", len(unassessed_observations)) or 0
)
unmatched_count = int(
    token_summary.get("unmatched_observation_count", len(unmatched_observations)) or 0
)

candidate_head = (
    "<table><thead><tr>"
    "<th>Location</th><th>Context</th><th>Observed value</th><th>Token</th>"
    "<th>Canonical value</th><th>Distance</th><th>Confidence</th><th>Evidence</th>"
    "</tr></thead><tbody>{rows}</tbody></table>"
)
confirmed_rows_html = token_candidate_rows(confirmed_candidates)
confirmed_candidates_table_html = candidate_head.format(
    rows=confirmed_rows_html or '<tr><td colspan="8" class="muted">No confirmed migration candidates in this scan</td></tr>'
)
possible_rows_html = token_candidate_rows(possible_candidates)
possible_candidates_table_html = candidate_head.format(
    rows=possible_rows_html or '<tr><td colspan="8" class="muted">No possible migration candidates in this scan</td></tr>'
)

unassessed_rows = []
for row in unassessed_observations:
    unassessed_rows.append(
        "<tr>"
        f"<td>{esc(location_label(row))}</td>"
        f"<td>{esc(row.get('context', ''))}</td>"
        f"<td><code>{esc(row.get('value', ''))}</code></td>"
        f"<td>{esc(evidence_cell(row))}</td>"
        "</tr>"
    )
unassessed_rows_html = "".join(unassessed_rows) or (
    '<tr><td colspan="4" class="muted">No unassessed observations in this scan</td></tr>'
)
unassessed_table_html = (
    "<table><thead><tr><th>Location</th><th>Context</th><th>Observed value</th><th>Evidence</th></tr></thead>"
    f"<tbody>{unassessed_rows_html}</tbody></table>"
)

unmatched_count_note = ""
if unmatched_count > 0:
    unmatched_count_note = (
        f"{num(unmatched_count)} observation(s) were assessed and found unmatched "
        "(informational, not migration debt)."
    )
if unassessed_count > 0:
    unmatched_count_note = (
        unmatched_count_note
        + " Run the wax-registry-discover skill to review missing canonical token values."
    ).strip()

top_ds = ds_symbols[0] if ds_symbols else None
findings = []
if top_ds:
    findings.append(
        f"<li><strong>{esc(top_ds.get('symbol'))} leads DS usage</strong> — {num(top_ds.get('count', 0))} resolved call sites.</li>"
    )
if data.get("fragmentation_candidates"):
    family = data["fragmentation_candidates"][0]
    findings.append(
        f"<li><strong>{esc(family.get('pattern'))} is the clearest consolidation target</strong> — {num(family.get('count', 0))} local symbols appear in that family.</li>"
    )
if unused_registry_components:
    sample = ", ".join(item.get("symbol", "") for item in unused_registry_components[:3])
    findings.append(
        f"<li><strong>Unused registry components are now named</strong> — review <code>{esc(sample)}</code> for docs promotion or deprecation.</li>"
    )
if visible_hotspots:
    hotspot = max(visible_hotspots, key=lambda item: int(item.get("local_raw_invocation_count", 0) or 0))
    if int(hotspot.get("local_raw_invocation_count", 0) or 0) > 0:
        findings.append(
            f"<li><strong>{esc(hotspot.get('symbol') or hotspot.get('parent_id'))} is the best migration entry point</strong> — {num(hotspot.get('local_raw_invocation_count', 0))} local invocation(s) remain there.</li>"
        )
key_findings_html = f"<ul>{''.join(findings)}</ul>"

caveat_html = (
    "<strong>How to read this report.</strong> "
    "<strong>DS vs local</strong> compares resolved design system invocations with local UI component invocations. "
    "<strong>Registry resolution</strong> is secondary scanner-health context. "
    "<strong>Unresolved</strong> counts are informational and are not treated as migration debt in the candidate table."
)

replacements = {
    "repo_name": esc(repo_name),
    "logo_svg": logo_svg,
    "generated_at": esc(data.get("generated_at", "")),
    "source_scan": esc(data.get("source_scan", "")),
    "schema_version": esc(data.get("schema_version", "")),
    "resolved_count": num(raw.get("resolved", 0)),
    "adopted_components_count": num(registry.get("used_component_count", 0)),
    "total_registry_components": num(registry.get("component_count", 0)),
    "invocation_adoption_percent": ds_vs_local_pct,
    "raw_invocation_total": num(raw.get("total", 0)),
    "registry_resolution_percent": registry_resolution_pct,
    "local_definition_count": num(definitions.get("local_definition_count", 0)),
    "unresolved_count": num(raw.get("unresolved", 0)),
    "caveat_html": caveat_html,
    "ds_usage_chart_svg": ds_usage_chart_svg,
    "ds_symbols_table_html": ds_symbols_table_html,
    "unused_registry_count": num(unused_registry_count),
    "unused_components_table_html": unused_components_table_html,
    "parent_scope_chart_svg": parent_scope_chart_svg,
    "parent_scope_table_html": parent_scope_table_html,
    "adoption_gaps_chart_svg": adoption_gaps_chart_svg,
    "adoption_gaps_table_html": adoption_gaps_table_html,
    "duplicate_components_table_html": duplicate_components_table_html,
    "migration_candidates_table_html": migration_candidates_table_html,
    "confirmed_candidates_table_html": confirmed_candidates_table_html,
    "possible_candidates_table_html": possible_candidates_table_html,
    "unassessed_table_html": unassessed_table_html,
    "unassessed_count": num(unassessed_count),
    "unmatched_count_note": esc(unmatched_count_note),
    "key_findings_html": key_findings_html,
}

rendered = template
for key, value in replacements.items():
    rendered = rendered.replace("{{" + key + "}}", value)

def remove_section(text, heading):
    pattern = rf'\s*<h2>{re.escape(heading)}</h2>.*?(?=\n\s*<h2>|\n</div>\n</body>)'
    return re.sub(pattern, "", text, flags=re.S)

if unused_registry_count == 0:
    rendered = remove_section(rendered, "Unused design system components")
if not visible_hotspots:
    rendered = remove_section(rendered, "Adoption by area")
if len(per_language) <= 1:
    rendered = remove_section(rendered, "Adoption gaps")
if not duplicate_rows:
    rendered = remove_section(rendered, "Exact-name duplicates in shared UI")
if not confirmed_candidates:
    rendered = remove_section(rendered, "Confirmed token migrations")
if not possible_candidates:
    rendered = remove_section(rendered, "Possible token migrations")
if unassessed_count == 0 and unmatched_count == 0:
    rendered = remove_section(rendered, "Registry metadata gaps")

if "{{" in rendered:
    unresolved = sorted(set(part.split("}}", 1)[0] for part in rendered.split("{{")[1:]))
    raise SystemExit(f"FAIL: unresolved placeholders remain: {', '.join(unresolved)}")

output_path.write_text(rendered, encoding="utf-8")
PY

if grep -q '{{' "$OUTPUT"; then
  echo "FAIL: unresolved placeholders remain in $OUTPUT" >&2
  exit 1
fi

echo "PASS: rendered fixture report to $OUTPUT"
