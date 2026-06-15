#!/usr/bin/env bash
# End-to-end integration smoke for wax-scan skill workflow (Task 5).
# Exercises validate → scan → extract → HTML report on the compose smoke fixture.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

SKILL_DIR="skills/wax-scan"
EXTRACTOR="$SKILL_DIR/scripts/extract-insights.sh"
RENDER="$SKILL_DIR/scripts/render-fixture-smoke.sh"
FIXTURE_SRC="engine/fixtures/smoke/compose/repo"
SKILL_MD="$SKILL_DIR/SKILL.md"

if ! command -v wax >/dev/null 2>&1; then
  echo "FAIL: wax CLI is required on PATH" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

for path in "$EXTRACTOR" "$RENDER" "$FIXTURE_SRC/design-system/registry.json" "$SKILL_MD"; do
  if [[ ! -e "$path" ]]; then
    fail "missing required path: $path"
  fi
done

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/wax-scan-integration-smoke.XXXXXX")"
cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

cp -R "$FIXTURE_SRC/." "$WORK_DIR/"

(
  cd "$WORK_DIR"

  wax init --non-interactive --language compose --repo-root . >/dev/null
  cp design-system/registry.json .wax/compose.registry.json
  wax language update compose --repo-root . >/dev/null

  if ! wax validate --repo-root . >/dev/null 2>&1; then
    fail "wax validate failed on smoke fixture"
  fi

  if ! wax scan --no-auto-install --repo-root . >/dev/null 2>&1; then
    fail "wax scan failed on smoke fixture"
  fi

  if [[ ! -f .wax/out/scan-merged.json ]]; then
    fail "scan did not write .wax/out/scan-merged.json"
  fi

  INSIGHTS="$("$ROOT/$EXTRACTOR" .wax/out/scan-merged.json)"
  schema_version="$(printf '%s' "$INSIGHTS" | jq -r '.schema_version')"
  if [[ "$schema_version" != "1" ]]; then
    fail "expected insights schema_version 1, got ${schema_version}"
  fi

  resolved="$(printf '%s' "$INSIGHTS" | jq -r '.repo_summary.resolved_count')"
  if [[ "$resolved" -lt 1 ]]; then
    fail "expected at least one resolved usage site, got ${resolved}"
  fi

  # Minimal terminal summary (agent would expand this in the skill workflow).
  coverage="$(printf '%s' "$INSIGHTS" | jq -r '.repo_summary.adoption_coverage_ratio')"
  printf 'Terminal summary: %s resolved usage site(s), adoption coverage %.0f%%\n' \
    "$resolved" "$(awk "BEGIN { printf \"%.0f\", $coverage * 100 }")"

  "$ROOT/$RENDER" .wax/out/report/index.html >/dev/null
  if [[ ! -s .wax/out/report/index.html ]]; then
    fail "HTML report was not written to .wax/out/report/index.html"
  fi
)

# Guardrail documentation checks (skill docs, not runtime).
guardrail_checks=(
  "wax init"
  "Run \`wax validate\`"
  "fresh \`wax scan\`"
  "Data gap:"
  "Skip trend analysis unless \`--baseline\`"
)

for phrase in "${guardrail_checks[@]}"; do
  if ! grep -Fq "$phrase" "$SKILL_MD"; then
    fail "SKILL.md missing guardrail phrase: $phrase"
  fi
done

echo "PASS: wax-scan integration smoke (validate → scan → extract → HTML)"
