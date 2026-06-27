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
BASELINE_COMPOSE_ONLY_V2="$FIXTURES/scan-merged.schema-v2.sample.json"

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

if [[ ! -x "$SCRIPT" ]]; then
  echo "FAIL: missing executable $SCRIPT" >&2
  exit 1
fi

fail() {
  echo "FAIL: $1" >&2
  exit 1
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

PARTIAL_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$BASELINE_COMPOSE_ONLY_V2")"
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
echo "PASS: schema mismatch handling"

echo "PASS: all wax-scan extract-insights tests"
