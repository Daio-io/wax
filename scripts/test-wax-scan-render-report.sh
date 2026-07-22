#!/usr/bin/env bash
# Fixture test for wax-scan HTML rendering (repository maintainer verification).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RENDER="$ROOT/scripts/render-wax-scan-fixture-report.sh"
FIXTURE="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"
OUTPUT="$(mktemp "${TMPDIR:-/tmp}/wax-scan-render.XXXXXX.html")"
trap 'rm -f "$OUTPUT"' EXIT

cd "$ROOT"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

assert_contains() {
  local needle="$1"
  if ! grep -Fq -- "$needle" "$OUTPUT"; then
    fail "expected rendered report to contain: $needle"
  fi
}

assert_not_contains() {
  local needle="$1"
  if grep -Fq -- "$needle" "$OUTPUT"; then
    fail "expected rendered report to omit: $needle"
  fi
}

"$RENDER" --insights "$FIXTURE" --repo-name "wax-render-test" "$OUTPUT" >/dev/null

assert_contains "--bg: #000000;"
assert_contains "--accent: #FCC457;"
assert_contains "class=\"report-logo\""
assert_contains "aria-label=\"Wax logo\""
assert_contains "Design System Adoption"
assert_contains "DS vs local UI coverage"
assert_contains "Adoption gaps"
assert_contains "Key findings"
assert_contains "Modal"
assert_contains "PrimaryButton"
assert_contains "IconButton"
assert_contains "Area</th><th>DS</th><th>Local</th><th>Unresolved</th><th>Total</th>"
assert_contains "<td><code>HomeScreen</code></td>"
assert_contains "<td><code>PrimaryButton</code></td><td>HomeScreen</td>"
assert_contains "Confirmed token migrations"
assert_contains "Possible token migrations"
assert_contains "Token comparison coverage"
assert_contains "3 of 4 hard-coded observations were assessed"
assert_contains "Raw hard-coded observations are inventory, not debt"
assert_contains "spacing.s"
assert_contains "spacing.m"
assert_contains "very high"
assert_contains "informational, not migration debt"
assert_contains "wax-registry-discover"
assert_not_contains "Invocation adoption"
assert_not_contains "No exact-name duplicate detected"
assert_not_contains "UnknownWidget"
assert_not_contains "This fixture does not include the full registry symbol list"
assert_not_contains "Action queue"
assert_not_contains "hero-metric"

if grep -Fq "{{" "$OUTPUT"; then
  fail "rendered report still contains unresolved template placeholders"
fi

echo "PASS: wax-scan HTML renderer emits the updated report contract"
