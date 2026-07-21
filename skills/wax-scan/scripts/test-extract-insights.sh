#!/usr/bin/env bash
# Verifies extract-insights.sh output against the committed fixture expectations.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
EXTRACTOR="$ROOT/skills/wax-scan/scripts/extract-insights.sh"
SCAN="$ROOT/scripts/fixtures/wax-scan/scan-merged.sample.json"
BASELINE_SCHEMA_V2="$ROOT/scripts/fixtures/wax-scan/scan-merged.schema-v2.sample.json"
EXPECTED="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"

if [[ ! -x "$EXTRACTOR" ]]; then
  echo "extract-insights.sh is not executable: $EXTRACTOR" >&2
  exit 1
fi

ACTUAL="$(mktemp)"
NO_REPO_SUMMARY="$(mktemp)"
HOTSPOT_SCAN="$(mktemp)"
BASELINE_V3="$(mktemp)"
trap 'rm -f "$ACTUAL" "$NO_REPO_SUMMARY" "$HOTSPOT_SCAN" "$BASELINE_V3"' EXIT

"$EXTRACTOR" "$SCAN" | jq 'del(.generated_at)' >"$ACTUAL"

diff -u "$EXPECTED" "$ACTUAL"

jq -e '
    (.token_inference.confirmed_candidates | length) == 1
    and (.token_inference.possible_candidates | length) == 1
    and (.token_inference.unmatched_observations | length) == 1
    and (.token_inference.unassessed_observations | length) == 1
  ' "$ACTUAL" >/dev/null

# A schema-v2 baseline lacks inference classifications, so it must be treated
# as incompatible rather than silently mixed with v3 denominators.
"$EXTRACTOR" "$SCAN" --baseline "$BASELINE_SCHEMA_V2" \
  | jq -e '.baseline_deltas == null' >/dev/null

jq '.schema_version = 3' "$BASELINE_SCHEMA_V2" >"$BASELINE_V3"
"$EXTRACTOR" "$SCAN" --baseline "$BASELINE_V3" \
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
      | map(select(
          .parent_id == "react:component:src/App#App"
          and .raw_invocation_count == 5
          and .resolved_raw_invocation_count == 3
          and .local_raw_invocation_count == 2
          and .unresolved_raw_invocation_count == 0
        ))
      | length == 1
    ' >/dev/null

echo "extract-insights.sh fixture check passed"
