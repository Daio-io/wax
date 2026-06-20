#!/usr/bin/env bash
# Verifies extract-insights.sh output against the committed fixture expectations.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
EXTRACTOR="$ROOT/skills/wax-scan/scripts/extract-insights.sh"
SCAN="$ROOT/scripts/fixtures/wax-scan/scan-merged.sample.json"
EXPECTED="$ROOT/scripts/fixtures/wax-scan/expected-insights.sample.json"

if [[ ! -x "$EXTRACTOR" ]]; then
  chmod +x "$EXTRACTOR"
fi

ACTUAL="$(mktemp)"
trap 'rm -f "$ACTUAL"' EXIT

"$EXTRACTOR" "$SCAN" | jq 'del(.generated_at)' >"$ACTUAL"

diff -u "$EXPECTED" "$ACTUAL"
echo "extract-insights.sh fixture check passed"
