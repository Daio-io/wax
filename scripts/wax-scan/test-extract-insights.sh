#!/usr/bin/env bash
# Fixture test for wax-scan extract-insights.sh (repository maintainer verification).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

SCRIPT="skills/wax-scan/scripts/extract-insights.sh"
FIXTURE="$SCRIPT_DIR/fixtures/scan-merged.sample.json"
EXPECTED="$SCRIPT_DIR/fixtures/expected-insights.sample.json"
BASELINE_COMPOSE_ONLY="$SCRIPT_DIR/fixtures/scan-merged.compose-only.sample.json"
BASELINE_SCHEMA_V2="$SCRIPT_DIR/fixtures/scan-merged.schema-v2.sample.json"

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
    | .source_scan = "scripts/wax-scan/fixtures/scan-merged.sample.json"
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

SAME_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$FIXTURE")"
assert_eq \
  "identical baseline yields zero repo deltas" \
  "$(jq -r '.baseline_deltas.resolved_count' <<<"$SAME_BASELINE")" \
  "0"
assert_eq \
  "identical baseline yields zero per-language deltas" \
  "$(jq -r '[.baseline_deltas.per_language[].resolved_count] | add // 0' <<<"$SAME_BASELINE")" \
  "0"
echo "PASS: identical baseline comparison"

PARTIAL_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$BASELINE_COMPOSE_ONLY")"
assert_eq \
  "partial baseline still computes repo resolved delta" \
  "$(jq -r '.baseline_deltas.resolved_count' <<<"$PARTIAL_BASELINE")" \
  "3"
assert_eq \
  "partial baseline includes shared compose per-language delta" \
  "$(jq -r '.baseline_deltas.per_language | length' <<<"$PARTIAL_BASELINE")" \
  "1"
assert_eq \
  "partial baseline compose delta is zero when compose unchanged" \
  "$(jq -r '.baseline_deltas.per_language[0].resolved_count' <<<"$PARTIAL_BASELINE")" \
  "0"
if ! jq -e '.limits[] | select(.metric == "Per-language baseline deltas")' <<<"$PARTIAL_BASELINE" >/dev/null; then
  fail "partial baseline should emit per-language limit when language sets differ"
fi
echo "PASS: partial baseline comparison"

MISSING_BASELINE="$("$SCRIPT" "$FIXTURE" --baseline "$SCRIPT_DIR/fixtures/does-not-exist.json")"
if jq -e '.baseline_deltas != null' <<<"$MISSING_BASELINE" >/dev/null; then
  fail "missing baseline should leave baseline_deltas null"
fi
if ! jq -e '.limits[] | select(.metric == "Baseline comparison")' <<<"$MISSING_BASELINE" >/dev/null; then
  fail "missing baseline should emit baseline comparison limit"
fi
echo "PASS: missing baseline handling"

SCHEMA_MISMATCH="$("$SCRIPT" "$FIXTURE" --baseline "$BASELINE_SCHEMA_V2")"
if jq -e '.baseline_deltas != null' <<<"$SCHEMA_MISMATCH" >/dev/null; then
  fail "schema mismatch should leave baseline_deltas null"
fi
if ! jq -e '.limits[] | select(.metric == "Baseline comparison" and (.missing_capability | test("schema_version")))' <<<"$SCHEMA_MISMATCH" >/dev/null; then
  fail "schema mismatch should emit schema incompatibility limit"
fi
echo "PASS: schema mismatch handling"

echo "PASS: all extract-insights tests"
