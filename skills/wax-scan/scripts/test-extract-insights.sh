#!/usr/bin/env bash
# Verifies extract-insights.sh output against the committed fixture expectations.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
EXTRACTOR="$ROOT/skills/wax-scan/scripts/extract-insights.sh"
SCAN="$ROOT/scripts/fixtures/wax-scan/scan-merged.sample.json"
BASELINE_V2="$ROOT/scripts/fixtures/wax-scan/scan-merged.schema-v2.sample.json"
EXPECTED="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"

if [[ ! -x "$EXTRACTOR" ]]; then
  echo "extract-insights.sh is not executable: $EXTRACTOR" >&2
  exit 1
fi

ACTUAL="$(mktemp)"
NO_REPO_SUMMARY="$(mktemp)"
HOTSPOT_SCAN="$(mktemp)"
trap 'rm -f "$ACTUAL" "$NO_REPO_SUMMARY" "$HOTSPOT_SCAN"' EXIT

"$EXTRACTOR" "$SCAN" | jq 'del(.generated_at)' >"$ACTUAL"

diff -u "$EXPECTED" "$ACTUAL"

"$EXTRACTOR" "$SCAN" --baseline "$BASELINE_V2" \
  | jq -e '
      .baseline_deltas.symbol_usage_summary
      | map(select(.symbol_id == "compose:registry:com.ds.Modal" and .raw_invocation_count == 1))
      | length == 1
    ' >/dev/null

jq 'del(.repo_summary)' "$SCAN" >"$NO_REPO_SUMMARY"
"$EXTRACTOR" "$SCAN" --baseline "$NO_REPO_SUMMARY" \
  | jq -e '
      .baseline_deltas.invocation_adoption_ratio == 0
      and .baseline_deltas.registry_resolution_ratio == 0
      and .baseline_deltas.parent_scopes.total == 0
    ' >/dev/null

jq '
  .languages.react.usage_sites[3].parent = {
    parent_id: "react:component:src/App#App",
    symbol: "App",
    scope_kind: "component",
    identity_basis: "module_path_and_symbol",
    identity_stability: "path_sensitive"
  }
  | .symbol_usage_summary = []
  | .languages.react.symbol_usage_summary = []
' "$SCAN" >"$HOTSPOT_SCAN"
"$EXTRACTOR" "$HOTSPOT_SCAN" \
  | jq -e '
      .parent_scope_hotspots
      | map(select(.parent_id == "react:component:src/App#App" and .invocation_count == 1))
      | length == 1
    ' >/dev/null

echo "extract-insights.sh fixture check passed"
