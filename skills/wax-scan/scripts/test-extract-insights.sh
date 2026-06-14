#!/usr/bin/env bash
# Fixture test for wax-scan extract-insights.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

SCRIPT="skills/wax-scan/scripts/extract-insights.sh"
FIXTURE="skills/wax-scan/fixtures/scan-merged.sample.json"
EXPECTED="skills/wax-scan/fixtures/expected-insights.sample.json"

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

if [[ ! -x "$SCRIPT" ]]; then
  echo "FAIL: missing executable $SCRIPT" >&2
  exit 1
fi

ACTUAL="$("$SCRIPT" "$FIXTURE")"

normalize() {
  jq -S '
    del(.generated_at)
    | .source_scan = "skills/wax-scan/fixtures/scan-merged.sample.json"
  '
}

NORM_ACTUAL="$(printf '%s\n' "$ACTUAL" | normalize)"
NORM_EXPECTED="$(normalize <"$EXPECTED")"

if [[ "$NORM_ACTUAL" == "$NORM_EXPECTED" ]]; then
  echo "PASS: extract-insights output matches expected key fields"
  exit 0
fi

echo "FAIL: extract-insights output differs from expected" >&2
echo "--- expected ---" >&2
printf '%s\n' "$NORM_EXPECTED" >&2
echo "--- actual ---" >&2
printf '%s\n' "$NORM_ACTUAL" >&2
exit 1
