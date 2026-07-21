#!/usr/bin/env bash
# Fixture test for wax-scan extract-insights.sh (repository maintainer verification).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="$ROOT/scripts/fixtures/wax-scan"
cd "$ROOT"

SCRIPT="skills/wax-scan/scripts/extract-insights.sh"
FIXTURE="$FIXTURES/scan-merged.sample.json"
EXPECTED="$FIXTURES/expected-insights.sample.json"
BASELINE_SCHEMA_V1="$FIXTURES/scan-merged.compose-only.sample.json"
BASELINE_SCHEMA_V2="$FIXTURES/scan-merged.schema-v2.sample.json"

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

PARTIAL_BASELINE_V3="$(mktemp)"
MISSING_JOIN_SCAN="$(mktemp)"
MISSING_TOKEN_INFERENCE_SCAN="$(mktemp)"
UNKNOWN_CLASSIFICATION_SCAN="$(mktemp)"
MISMATCHED_COUNTS_SCAN="$(mktemp)"
DUPLICATE_RAW_SITE_SCAN="$(mktemp)"
MISSING_INFERENCE_ROW_SCAN="$(mktemp)"
DUPLICATE_INFERENCE_KEY_SCAN="$(mktemp)"
EXTRACTOR_ERROR="$(mktemp)"
trap 'rm -f "$PARTIAL_BASELINE_V3" "$MISSING_JOIN_SCAN" "$MISSING_TOKEN_INFERENCE_SCAN" "$UNKNOWN_CLASSIFICATION_SCAN" "$MISMATCHED_COUNTS_SCAN" "$DUPLICATE_RAW_SITE_SCAN" "$MISSING_INFERENCE_ROW_SCAN" "$DUPLICATE_INFERENCE_KEY_SCAN" "$EXTRACTOR_ERROR"' EXIT
jq '.schema_version = 3' "$BASELINE_SCHEMA_V2" >"$PARTIAL_BASELINE_V3"

if [[ ! -x "$SCRIPT" ]]; then
  echo "FAIL: missing executable $SCRIPT" >&2
  exit 1
fi

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

assert_extractor_rejects() {
  local label="$1"
  local scan="$2"
  if "$SCRIPT" "$scan" >/dev/null 2>"$EXTRACTOR_ERROR"; then
    fail "$label"
  fi
}

assert_eq() {
  local label="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: $label" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
  fi
}

normalize() {
  jq -S '
    del(.generated_at)
    | .source_scan = "scripts/fixtures/wax-scan/scan-merged.sample.json"
  '
}

ACTUAL="$("$SCRIPT" "$FIXTURE")"
NORM_ACTUAL="$(printf '%s\n' "$ACTUAL" | normalize)"
NORM_EXPECTED="$(normalize <"$EXPECTED")"

if [[ "$NORM_ACTUAL" != "$NORM_EXPECTED" ]]; then
  echo "FAIL: extract-insights output differs from expected" >&2
  echo "--- expected ---" >&2
  printf '%s\n' "$NORM_EXPECTED" >&2
  echo "--- actual ---" >&2
  printf '%s\n' "$NORM_ACTUAL" >&2
  exit 1
fi
echo "PASS: default extraction matches expected key fields"

assert_eq \
  "confirmed candidates contain the exact row" \
  "$(jq '.token_inference.confirmed_candidates | length' <<<"$ACTUAL")" \
  "1"
assert_eq \
  "possible candidates contain the near row" \
  "$(jq '.token_inference.possible_candidates | length' <<<"$ACTUAL")" \
  "1"
assert_eq \
  "unmatched observations contain the unmatched row" \
  "$(jq '.token_inference.unmatched_observations | length' <<<"$ACTUAL")" \
  "1"
assert_eq \
  "unassessed observations contain the unassessed row" \
  "$(jq '.token_inference.unassessed_observations | length' <<<"$ACTUAL")" \
  "1"
assert_eq \
  "token inference summary passes through core counts" \
  "$(jq '.token_inference.summary.hardcoded_observation_count' <<<"$ACTUAL")" \
  "4"
echo "PASS: token inference classification arrays"

assert_eq \
  "repo summary exposes ds_vs_local_ratio" \
  "$(jq -r '.repo_summary.ds_vs_local_ratio' <<<"$ACTUAL")" \
  "0.6666666666666666"
assert_eq \
  "compose exposes ds_vs_local_ratio" \
  "$(jq -r '.per_language[] | select(.language_id == "compose") | .ds_vs_local_ratio' <<<"$ACTUAL")" \
  "0.75"
assert_eq \
  "react exposes ds_vs_local_ratio" \
  "$(jq -r '.per_language[] | select(.language_id == "react") | .ds_vs_local_ratio' <<<"$ACTUAL")" \
  "0.6"

assert_eq \
  "unused registry components are enumerated by symbol" \
  "$(jq -r '.unused_registry_components[0].symbol' <<<"$ACTUAL")" \
  "Modal"
assert_eq \
  "unused registry components exclude candidate-only usage" \
  "$(jq -r '.unused_registry_components | length' <<<"$ACTUAL")" \
  "1"
assert_eq \
  "parent scope hotspots expose resolved counts" \
  "$(jq -r '.parent_scope_hotspots[] | select(.symbol == "HomeScreen") | .resolved_raw_invocation_count' <<<"$ACTUAL")" \
  "2"
assert_eq \
  "parent scope hotspots expose local counts" \
  "$(jq -r '.parent_scope_hotspots[] | select(.symbol == "HomeScreen") | .local_raw_invocation_count' <<<"$ACTUAL")" \
  "1"
assert_eq \
  "parent scope hotspots expose unresolved counts" \
  "$(jq -r '.parent_scope_hotspots[] | select(.symbol == "HomeScreen") | .unresolved_raw_invocation_count' <<<"$ACTUAL")" \
  "1"

SAME_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$FIXTURE")"
assert_eq \
  "identical baseline yields zero repo deltas" \
  "$(jq -r '.baseline_deltas.raw_invocations.resolved' <<<"$SAME_BASELINE")" \
  "0"
assert_eq \
  "identical baseline yields zero per-language deltas" \
  "$(jq -r '[.baseline_deltas.per_language[].raw_invocations.resolved] | add // 0' <<<"$SAME_BASELINE")" \
  "0"
echo "PASS: identical baseline comparison"

PARTIAL_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$PARTIAL_BASELINE_V3")"
assert_eq \
  "partial baseline still computes repo resolved delta" \
  "$(jq -r '.baseline_deltas.raw_invocations.resolved' <<<"$PARTIAL_BASELINE")" \
  "5"
assert_eq \
  "partial baseline omits per-language deltas when no language facts overlap" \
  "$(jq -r '.baseline_deltas.per_language | length' <<<"$PARTIAL_BASELINE")" \
  "0"
if ! jq -e '.limits[] | select(.metric == "Per-language baseline deltas")' <<<"$PARTIAL_BASELINE" >/dev/null; then
  fail "partial baseline should emit per-language limit when language sets differ"
fi
echo "PASS: partial baseline comparison"

MISSING_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$FIXTURES/does-not-exist.json")"
if jq -e '.baseline_deltas != null' <<<"$MISSING_BASELINE" >/dev/null; then
  fail "missing baseline should leave baseline_deltas null"
fi
if ! jq -e '.limits[] | select(.metric == "Baseline comparison")' <<<"$MISSING_BASELINE" >/dev/null; then
  fail "missing baseline should emit baseline comparison limit"
fi
echo "PASS: missing baseline handling"

SCHEMA_MISMATCH="$("$SCRIPT" "$FIXTURE" --baseline "$BASELINE_SCHEMA_V1")"
if jq -e '.baseline_deltas != null' <<<"$SCHEMA_MISMATCH" >/dev/null; then
  fail "schema mismatch should leave baseline_deltas null"
fi
if ! jq -e '.limits[] | select(.metric == "Baseline comparison" and (.missing_capability | test("schema_version")))' <<<"$SCHEMA_MISMATCH" >/dev/null; then
  fail "schema mismatch should emit schema incompatibility limit"
fi
echo "PASS: schema v1 mismatch handling"

SCHEMA_V2_MISMATCH="$("$SCRIPT" "$FIXTURE" --baseline "$BASELINE_SCHEMA_V2")"
if jq -e '.baseline_deltas != null' <<<"$SCHEMA_V2_MISMATCH" >/dev/null; then
  fail "schema-v2 baseline should leave baseline_deltas null; it lacks inference classifications"
fi
if ! jq -e '.limits[] | select(.metric == "Baseline comparison" and (.missing_capability | test("schema_version")))' <<<"$SCHEMA_V2_MISMATCH" >/dev/null; then
  fail "schema-v2 baseline should emit schema incompatibility limit"
fi
echo "PASS: schema v2 baseline is treated as incompatible"

jq '.token_inference.sites[0].site_id = "does-not-exist"' "$FIXTURE" >"$MISSING_JOIN_SCAN"
assert_extractor_rejects \
  "extractor should fail closed when an inference row has no matching raw site" \
  "$MISSING_JOIN_SCAN"
echo "PASS: extractor fails closed on unresolved inference join"

jq 'del(.token_inference)' "$FIXTURE" >"$MISSING_TOKEN_INFERENCE_SCAN"
assert_extractor_rejects \
  "schema-v3 extraction should require token_inference" \
  "$MISSING_TOKEN_INFERENCE_SCAN"
echo "PASS: extractor rejects missing token inference"

jq '.token_inference.sites[0].classification = "future_classification"' \
  "$FIXTURE" >"$UNKNOWN_CLASSIFICATION_SCAN"
assert_extractor_rejects \
  "extractor should reject unknown token inference classifications" \
  "$UNKNOWN_CLASSIFICATION_SCAN"
echo "PASS: extractor rejects unknown classifications"

jq '.token_inference.counts.exact_replacement_candidate_count = 2' \
  "$FIXTURE" >"$MISMATCHED_COUNTS_SCAN"
assert_extractor_rejects \
  "extractor should reject token inference count and row mismatches" \
  "$MISMATCHED_COUNTS_SCAN"
echo "PASS: extractor rejects mismatched token inference counts"

jq '.languages.react.hardcoded_style_sites += [.languages.react.hardcoded_style_sites[0]]' \
  "$FIXTURE" >"$DUPLICATE_RAW_SITE_SCAN"
assert_extractor_rejects \
  "extractor should reject duplicate language/site raw keys" \
  "$DUPLICATE_RAW_SITE_SCAN"
echo "PASS: extractor rejects duplicate raw-site join keys"

jq '
  .token_inference.sites = .token_inference.sites[1:]
  | .token_inference.counts.hardcoded_observation_count = 3
  | .token_inference.counts.assessed_observation_count = 2
  | .token_inference.counts.exact_replacement_candidate_count = 0
' "$FIXTURE" >"$MISSING_INFERENCE_ROW_SCAN"
assert_extractor_rejects \
  "extractor should reject a raw site without an inference row" \
  "$MISSING_INFERENCE_ROW_SCAN"
echo "PASS: extractor rejects missing inference rows"

jq '
  .token_inference.sites = [
    .token_inference.sites[0],
    .token_inference.sites[0],
    .token_inference.sites[2],
    .token_inference.sites[3]
  ]
  | .token_inference.counts.exact_replacement_candidate_count = 2
  | .token_inference.counts.near_replacement_candidate_count = 0
' "$FIXTURE" >"$DUPLICATE_INFERENCE_KEY_SCAN"
assert_extractor_rejects \
  "extractor should reject duplicate language/site inference keys" \
  "$DUPLICATE_INFERENCE_KEY_SCAN"
echo "PASS: extractor rejects duplicate inference keys"

echo "PASS: all wax-scan extract-insights tests"
