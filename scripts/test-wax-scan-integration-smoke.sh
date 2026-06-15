#!/usr/bin/env bash
# End-to-end integration smoke for wax-scan skill workflow (repository maintainer verification).
# Exercises validate → scan → extract → HTML report on the compose smoke fixture.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

EXTRACTOR="skills/wax-scan/scripts/extract-insights.sh"
RENDER="$SCRIPT_DIR/render-wax-scan-fixture-report.sh"
FIXTURE_SRC="engine/fixtures/smoke/compose/repo"
SKILL_MD="skills/wax-scan/SKILL.md"

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

  if ! wax init --non-interactive --language compose --repo-root . >/dev/null 2>&1; then
    fail "wax init failed on smoke fixture"
  fi

  if [[ ! -f design-system/registry.json ]]; then
    fail "smoke fixture is missing design-system/registry.json"
  fi

  mkdir -p .wax
  cp design-system/registry.json .wax/compose.registry.json

  if ! wax language update compose --repo-root . >/dev/null 2>&1; then
    fail "wax language update compose failed on smoke fixture"
  fi

  if ! wax validate --repo-root . >/dev/null 2>&1; then
    fail "wax validate failed on smoke fixture"
  fi

  if ! wax scan --no-auto-install --repo-root . >/dev/null 2>&1; then
    fail "wax scan failed on smoke fixture"
  fi

  if [[ ! -f .wax/out/scan-merged.json ]]; then
    fail "scan did not write .wax/out/scan-merged.json"
  fi

  INSIGHTS_PATH=".wax/out/insights.json"
  if ! "$ROOT/$EXTRACTOR" .wax/out/scan-merged.json >"$INSIGHTS_PATH"; then
    fail "extract-insights failed on scan output"
  fi

  schema_version="$(jq -r '.schema_version' "$INSIGHTS_PATH")"
  if [[ "$schema_version" != "1" ]]; then
    fail "expected insights schema_version 1, got ${schema_version}"
  fi

  resolved="$(jq -r '.repo_summary.resolved_count' "$INSIGHTS_PATH")"
  if [[ "$resolved" -lt 1 ]]; then
    fail "expected at least one resolved usage site, got ${resolved}"
  fi

  ds_vs_local="$(jq -r '.repo_summary.ds_vs_local_ratio' "$INSIGHTS_PATH")"
  local_defs="$(jq -r '.repo_summary.local_definition_count' "$INSIGHTS_PATH")"
  if [[ "$local_defs" -lt 1 ]]; then
    fail "expected at least one local component definition, got ${local_defs}"
  fi

  coverage="$(jq -r '.repo_summary.adoption_coverage_ratio' "$INSIGHTS_PATH")"
  printf 'Terminal summary: %s resolved usage site(s), adoption coverage %.0f%%, DS vs local %.0f%%\n' \
    "$resolved" \
    "$(awk "BEGIN { printf \"%.0f\", $coverage * 100 }")" \
    "$(awk "BEGIN { printf \"%.0f\", $ds_vs_local * 100 }")"

  if ! "$RENDER" \
    --insights "$INSIGHTS_PATH" \
    --repo-name "compose-smoke" \
    .wax/out/report/index.html >/dev/null; then
    fail "HTML render failed"
  fi

  if [[ ! -s .wax/out/report/index.html ]]; then
    fail "HTML report was not written to .wax/out/report/index.html"
  fi

  if ! grep -Fq "compose-smoke" .wax/out/report/index.html; then
    fail "rendered HTML missing repo name from live insights render"
  fi

  if ! grep -Fq "$resolved" .wax/out/report/index.html; then
    fail "rendered HTML missing resolved count from live insights"
  fi

  if ! grep -Fq "$local_defs" .wax/out/report/index.html; then
    fail "rendered HTML missing local definition count from live insights"
  fi
)

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
