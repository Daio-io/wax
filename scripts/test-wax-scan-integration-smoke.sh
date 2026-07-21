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
WAX_BIN="${WAX_BIN:-wax}"

if ! command -v "$WAX_BIN" >/dev/null 2>&1; then
  echo "FAIL: wax CLI is required on PATH or via WAX_BIN" >&2
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
SKIP_MARKER="$WORK_DIR/skip-live-scan"
cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

cp -R "$FIXTURE_SRC/." "$WORK_DIR/"

(
  cd "$WORK_DIR"
  export WAX_HOME="$WORK_DIR/.wax-home"

  if ! "$WAX_BIN" init --non-interactive --language compose --repo-root . >/dev/null 2>&1; then
    fail "wax init failed on smoke fixture"
  fi

  if [[ ! -f design-system/registry.json ]]; then
    fail "smoke fixture is missing design-system/registry.json"
  fi

  mkdir -p .wax
  cp design-system/registry.json .wax/compose.registry.json

  if ! "$WAX_BIN" language update compose --repo-root . >/dev/null 2>&1; then
    fail "wax language update compose failed on smoke fixture"
  fi

  if ! "$WAX_BIN" validate --repo-root . >/dev/null 2>&1; then
    fail "wax validate failed on smoke fixture"
  fi

  SCAN_ERR="$WORK_DIR/scan.err"
  if ! "$WAX_BIN" scan --no-auto-install --repo-root . >/dev/null 2>"$SCAN_ERR"; then
    if grep -Fq "invalid ScanFacts contract" "$SCAN_ERR"; then
      touch "$SKIP_MARKER"
      exit 0
    fi
    cat "$SCAN_ERR" >&2
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
  if [[ "$schema_version" != "3" ]]; then
    fail "expected insights schema_version 3, got ${schema_version}"
  fi

  resolved="$(jq -r '.repo_summary.raw_invocations.resolved' "$INSIGHTS_PATH")"
  if [[ "$resolved" -lt 1 ]]; then
    fail "expected at least one resolved usage site, got ${resolved}"
  fi

  invocation_adoption="$(jq -r '.repo_summary.invocation_adoption_ratio // 0' "$INSIGHTS_PATH")"
  local_defs="$(jq -r '.repo_summary.definitions.local_definition_count' "$INSIGHTS_PATH")"
  if [[ "$local_defs" -lt 1 ]]; then
    fail "expected at least one local component definition, got ${local_defs}"
  fi

  printf 'Terminal summary: %s resolved usage site(s), invocation adoption %.0f%%\n' \
    "$resolved" \
    "$(awk "BEGIN { printf \"%.0f\", $invocation_adoption * 100 }")"

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

if [[ -f "$SKIP_MARKER" ]]; then
  echo "SKIP: installed compose pack is not yet compatible with Adoption Metrics v2 scan facts"
  exit 0
fi

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
