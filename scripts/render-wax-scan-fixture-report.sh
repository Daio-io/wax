#!/usr/bin/env bash
# Render wax-scan HTML report from insights JSON (repository maintainer verification).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

if [[ ! -f "$FIXTURE" || ! -f "$TEMPLATE" ]]; then
  echo "FAIL: missing fixture or template" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"

python3 - "$FIXTURE" "$TEMPLATE" "$OUTPUT" "$REPO_NAME" <<'PY'
import html
import json
import pathlib
import sys

fixture_path = pathlib.Path(sys.argv[1])
template_path = pathlib.Path(sys.argv[2])
output_path = pathlib.Path(sys.argv[3])
repo_name = sys.argv[4]

data = json.loads(fixture_path.read_text(encoding="utf-8"))
template = template_path.read_text(encoding="utf-8")


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


def section(title, lead, body):
    return (
        '<section class="section">'
        f"<h2>{esc(title)}</h2>"
        f'<p class="lead">{esc(lead)}</p>'
        f"{body}"
        "</section>"
    )


def bar_rows(items, value_key, fill_class, formatter=str, label_key="symbol", max_items=None):
    rows = items[:max_items] if max_items else items
    if not rows:
        return '<p class="footer-note">No ranked data available for this scan.</p>'
    max_value = max(max(float(item.get(value_key, 0) or 0), 0) for item in rows) or 1
    parts = ['<div class="bars">']
    for item in rows:
        value = float(item.get(value_key, 0) or 0)
        width = 0 if max_value == 0 else max(4, round(value / max_value * 100))
        parts.append(
            '<div class="bar-row">'
            f'<div class="bar-label">{esc(item.get(label_key, "Item"))}</div>'
            f'<div class="track"><div class="{fill_class}" style="width:{width}%"></div></div>'
            f'<div class="bar-value">{esc(formatter(item.get(value_key, 0)))}</div>'
            '</div>'
        )
    parts.append("</div>")
    return "".join(parts)


summary = data["repo_summary"]
raw = summary["raw_invocations"]
definitions = summary["definitions"]
registry = summary["registry"]

coverage_percent = pct(summary.get("ds_vs_local_ratio"))
coverage_note = (
    f"{num(raw.get('resolved', 0))} design-system invocations versus "
    f"{num(raw.get('local', 0))} local invocations. "
    "Unresolved calls stay in diagnostics so the main score reflects real migration opportunity."
)

kpis = [
    (num(raw.get("resolved", 0)), "DS invocations"),
    (num(raw.get("local", 0)), "Local invocations"),
    (num(definitions.get("local_definition_count", 0)), "Local UI definitions"),
    (f"{num(registry.get('used_component_count', 0))}/{num(registry.get('component_count', 0))}", "Registry components used"),
    (num(raw.get("unresolved", 0)), "Unresolved invocations"),
    (pct(summary.get("registry_resolution_ratio")), "Registry resolution"),
]
kpi_grid_html = "".join(
    '<div class="kpi-card">'
    f'<div class="value">{esc(value)}</div>'
    f'<div class="label">{esc(label)}</div>'
    '</div>'
    for value, label in kpis
)

ds_symbols = data.get("symbol_rollups", {}).get("design_system", [])
usage_chart = bar_rows(ds_symbols, "count", "fill-ds", formatter=lambda v: num(v), max_items=10)
usage_rows = []
total_ds = sum(int(item.get("count", 0) or 0) for item in ds_symbols) or 1
for item in ds_symbols:
    count = int(item.get("count", 0) or 0)
    share = f"{(count / total_ds) * 100:.1f}%"
    usage_rows.append(
        "<tr>"
        f"<td><code>{esc(item.get('symbol', ''))}</code></td>"
        f'<td class="num">{num(count)}</td>'
        f'<td class="num">{share}</td>'
        "</tr>"
    )
usage_table_rows = "".join(usage_rows) or '<tr><td colspan="3">No design system symbols detected.</td></tr>'
usage_table = (
    "<table><thead><tr><th>Component</th><th>Usages</th><th>Share of DS sites</th></tr></thead>"
    f"<tbody>{usage_table_rows}</tbody></table>"
)
usage_overview_section = section(
    "Design system component usage",
    "The table is the authoritative inventory. The compact chart above it is only for quick scanning.",
    '<div class="split">'
    f'<div class="mini-card"><h3>Top used DS symbols</h3>{usage_chart}</div>'
    f'<div class="mini-card"><h3>Full usage table</h3>{usage_table}</div>'
    "</div>",
)

unused_registry_components = data.get("unused_registry_components", [])
if unused_registry_components:
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
    unused_registry_section = section(
        "Unused design system components",
        f"{num(len(unused_registry_components))} registry component(s) have no resolved usage in this scan.",
        "<table><thead><tr><th>Component</th><th>Package</th><th>Suggested action</th></tr></thead>"
        f"<tbody>{''.join(unused_rows)}</tbody></table>",
    )
else:
    unused_registry_section = ""

hotspots = data.get("parent_scope_hotspots", [])
visible_hotspots = [
    item for item in hotspots
    if int(item.get("resolved_raw_invocation_count", 0) or 0) > 0
    or int(item.get("local_raw_invocation_count", 0) or 0) > 0
]
if visible_hotspots:
    area_rows = []
    table_rows = []
    max_total = max(int(item.get("raw_invocation_count", 0) or 0) for item in visible_hotspots) or 1
    for item in visible_hotspots:
        resolved = int(item.get("resolved_raw_invocation_count", 0) or 0)
        local = int(item.get("local_raw_invocation_count", 0) or 0)
        unresolved = int(item.get("unresolved_raw_invocation_count", 0) or 0)
        total = int(item.get("raw_invocation_count", 0) or 0)
        ds_width = 0 if max_total == 0 else max(4, round(resolved / max_total * 100)) if resolved else 0
        local_width = 0 if max_total == 0 else max(4, round(local / max_total * 100)) if local else 0
        label = item.get("symbol") or item.get("parent_id") or "Scope"
        area_rows.append(
            '<div class="bar-row">'
            f'<div class="bar-label">{esc(label)}</div>'
            '<div class="track">'
            f'<div class="fill-ds" style="width:{ds_width}%"></div>'
            f'<div class="fill-local" style="width:{local_width}%"></div>'
            "</div>"
            f'<div class="bar-value">{num(total)}</div>'
            "</div>"
        )
        table_rows.append(
            "<tr>"
            f"<td><code>{esc(label)}</code></td>"
            f'<td class="num">{num(resolved)}</td>'
            f'<td class="num">{num(local)}</td>'
            f'<td class="num">{num(unresolved)}</td>'
            f'<td class="num">{num(total)}</td>'
            "</tr>"
        )
    adoption_by_area_section = section(
        "Adoption by area",
        "Resolved versus local invocations by parent scope. Unresolved counts stay visible in the table as secondary context.",
        '<div class="split">'
        f'<div class="mini-card"><h3>Area hotspots</h3><div class="bars">{"".join(area_rows)}</div></div>'
        f'<div class="mini-card"><h3>Scope counts</h3><table><thead><tr><th>Area</th><th>DS</th><th>Local</th><th>Unresolved</th><th>Total</th></tr></thead><tbody>{"".join(table_rows)}</tbody></table></div>'
        "</div>",
    )
else:
    adoption_by_area_section = ""

per_language = data.get("per_language", [])
if len(per_language) > 1:
    language_rows = sorted(per_language, key=lambda item: item.get("ds_vs_local_ratio") or 0)
    language_bars = bar_rows(
        [{"symbol": item["language_id"], "ratio": (item.get("ds_vs_local_ratio") or 0) * 100} for item in language_rows],
        "ratio",
        "fill-ds",
        formatter=lambda v: f"{float(v):.1f}%",
    )
    table_rows = []
    for item in language_rows:
        raw_invocations = item.get("raw_invocations", {})
        table_rows.append(
            "<tr>"
            f"<td><code>{esc(item.get('language_id', ''))}</code></td>"
            f'<td class="num">{pct(item.get("ds_vs_local_ratio"))}</td>'
            f'<td class="num">{num(raw_invocations.get("resolved", 0))}</td>'
            f'<td class="num">{num(raw_invocations.get("local", 0))}</td>'
            "</tr>"
        )
    adoption_by_language_section = section(
        "Adoption by language",
        "Only shown for multi-language repositories so the report does not waste space on single-pack scans.",
        '<div class="split">'
        f'<div class="mini-card"><h3>Coverage ranking</h3>{language_bars}</div>'
        f'<div class="mini-card"><h3>Language counts</h3><table><thead><tr><th>Language</th><th>Coverage</th><th>DS</th><th>Local</th></tr></thead><tbody>{"".join(table_rows)}</tbody></table></div>'
        "</div>",
    )
else:
    adoption_by_language_section = ""

fragmentation_candidates = data.get("fragmentation_candidates", [])
if fragmentation_candidates:
    frag_rows = []
    for item in fragmentation_candidates:
        frag_rows.append(
            "<tr>"
            f"<td><code>{esc(item.get('pattern', ''))}</code></td>"
            f"<td>{esc(', '.join(item.get('symbols', [])))}</td>"
            f'<td class="num">{num(item.get("count", 0))}</td>'
            "</tr>"
        )
    fragmentation_section = section(
        "Fragmentation analysis",
        "Families with repeated local variants are stronger consolidation signals than isolated one-off components.",
        "<table><thead><tr><th>Pattern</th><th>Symbols</th><th>Count</th></tr></thead>"
        f"<tbody>{''.join(frag_rows)}</tbody></table>",
    )
else:
    fragmentation_section = ""

fragmentation_lookup = {}
for family in fragmentation_candidates:
    for symbol in family.get("symbols", []):
        fragmentation_lookup[symbol] = family.get("pattern")

local_rollups = data.get("symbol_rollups", {}).get("local", [])
local_scope_lookup = {}
for item in data.get("top_local_symbols", []):
    scopes = item.get("parent_scopes") or []
    if scopes:
        local_scope_lookup[item.get("symbol")] = scopes[0].get("symbol")

if local_rollups:
    candidate_rows = []
    for item in local_rollups:
        symbol = item.get("symbol", "")
        target = fragmentation_lookup.get(symbol) or "Review registry fit"
        scope = local_scope_lookup.get(symbol, "Scope unavailable")
        rationale = (
            f"Part of {fragmentation_lookup[symbol]} family." if symbol in fragmentation_lookup
            else "Repeated local invocation in the current scan."
        )
        candidate_rows.append(
            "<tr>"
            f"<td><code>{esc(symbol)}</code></td>"
            f"<td>{esc(scope)}</td>"
            f"<td>{esc(target)}</td>"
            f'<td class="num">{num(item.get("count", 0))}</td>'
            f"<td>{esc(rationale)}</td>"
            "</tr>"
        )
    migration_candidates_section = section(
        "Candidates to bring into the design system",
        "This queue stays focused on local symbols and fragmentation opportunities. Unresolved calls are tracked separately in diagnostics.",
        "<table><thead><tr><th>Local component</th><th>Scope</th><th>Suggested target</th><th>Invocations</th><th>Why it matters</th></tr></thead>"
        f"<tbody>{''.join(candidate_rows)}</tbody></table>",
    )
else:
    migration_candidates_section = ""

actions = []
if fragmentation_candidates:
    family = fragmentation_candidates[0]
    actions.append(
        f'<li><span class="pill">P1</span> Consolidate {esc(family.get("pattern", "fragmented family"))} '
        f'({num(family.get("count", 0))} symbols) into one clearer design-system path.</li>'
    )
if local_rollups:
    top_local = max(local_rollups, key=lambda item: int(item.get("count", 0) or 0))
    actions.append(
        f'<li><span class="pill">P1</span> Review <code>{esc(top_local.get("symbol", ""))}</code> as a migration candidate '
        f'because it still appears in local UI flows.</li>'
    )
if unused_registry_components:
    names = ", ".join(item.get("symbol", "") for item in unused_registry_components[:3])
    actions.append(
        f'<li><span class="pill">P2</span> Revisit unused registry coverage for <code>{esc(names)}</code> '
        'to decide whether docs or deprecation is the better move.</li>'
    )
if visible_hotspots:
    hotspot = max(
        visible_hotspots,
        key=lambda item: int(item.get("local_raw_invocation_count", 0) or 0),
    )
    if int(hotspot.get("local_raw_invocation_count", 0) or 0) > 0:
        actions.append(
            f'<li><span class="pill">P2</span> Start migration work in <code>{esc(hotspot.get("symbol") or hotspot.get("parent_id"))}</code>, '
            f'which carries {num(hotspot.get("local_raw_invocation_count", 0))} local invocation(s).</li>'
        )
action_queue_section = section(
    "Action queue",
    "Deterministic follow-up items ranked from clearest migration opportunity to softer cleanup work.",
    f'<ol class="action-list">{"".join(actions) or "<li>No immediate actions identified from this fixture.</li>"}</ol>',
)

limits = data.get("limits", [])
limit_items = "".join(
    f"<li>{esc(item.get('metric', 'Metric'))}: {esc(item.get('missing_capability', 'Not computed in this scan.'))}</li>"
    for item in limits[:4]
)
diagnostics_section = section(
    "Diagnostics and data gaps",
    "Scanner-quality context stays here so the main report can stay focused on design-system rollout decisions.",
    '<div class="stack">'
    f'<div class="mini-card"><h3>Diagnostics</h3><p class="footer-note">{num(raw.get("unresolved", 0))} unresolved invocation(s), '
    f'{pct(summary.get("registry_resolution_ratio"))} registry resolution, '
    f'and {num(raw.get("candidate", 0))} candidate invocation(s) needing review.</p></div>'
    f'<div class="mini-card"><h3>Visible limits</h3><ul class="footer-note">{limit_items}</ul></div>'
    "</div>",
)

replacements = {
    "repo_name": esc(repo_name),
    "generated_at": esc(data.get("generated_at", "")),
    "source_scan": esc(data.get("source_scan", "")),
    "schema_version": esc(data.get("schema_version", "")),
    "coverage_percent": esc(coverage_percent),
    "coverage_note": esc(coverage_note),
    "kpi_grid_html": kpi_grid_html,
    "usage_overview_section": usage_overview_section,
    "unused_registry_section": unused_registry_section,
    "adoption_by_area_section": adoption_by_area_section,
    "adoption_by_language_section": adoption_by_language_section,
    "fragmentation_section": fragmentation_section,
    "migration_candidates_section": migration_candidates_section,
    "action_queue_section": action_queue_section,
    "diagnostics_section": diagnostics_section,
}

rendered = template
for key, value in replacements.items():
    rendered = rendered.replace("{{" + key + "}}", value)

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
