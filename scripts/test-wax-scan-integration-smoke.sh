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
WAX_BIN="${WAX_BIN:-$ROOT/engine/target/debug/wax}"
WAX_COMPOSE_BIN="${WAX_COMPOSE_BIN:-$ROOT/engine/target/debug/wax-lang-compose}"

if ! command -v "$WAX_BIN" >/dev/null 2>&1; then
  echo "FAIL: workspace wax CLI is required via WAX_BIN; run cargo build --manifest-path engine/Cargo.toml -p wax-cli" >&2
  exit 1
fi

WAX_BIN="$(cd "$(dirname "$(command -v "$WAX_BIN")")" && pwd)/$(basename "$WAX_BIN")"

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

if [[ ! -x "$WAX_COMPOSE_BIN" ]]; then
  echo "FAIL: workspace Compose pack is required via WAX_COMPOSE_BIN; run cargo build --manifest-path engine/Cargo.toml -p wax-lang-compose" >&2
  exit 1
fi
WAX_COMPOSE_BIN="$(cd "$(dirname "$WAX_COMPOSE_BIN")" && pwd)/$(basename "$WAX_COMPOSE_BIN")"

if ! command -v rustc >/dev/null 2>&1 || ! command -v tar >/dev/null 2>&1; then
  echo "FAIL: rustc and tar are required" >&2
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

PACK_ARCHIVE="$WORK_DIR/wax-lang-compose.tar.gz"
PACK_INDEX="$WORK_DIR/pack-index.json"
tar -C "$(dirname "$WAX_COMPOSE_BIN")" -czf "$PACK_ARCHIVE" "$(basename "$WAX_COMPOSE_BIN")"

if command -v sha256sum >/dev/null 2>&1; then
  PACK_SHA256="$(sha256sum "$PACK_ARCHIVE" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  PACK_SHA256="$(shasum -a 256 "$PACK_ARCHIVE" | awk '{print $1}')"
else
  fail "sha256sum or shasum is required"
fi

HOST_TARGET="$(rustc -vV | awk '/^host:/ {print $2}')"
PACK_VERSION="$(awk -F '"' '
  /^\[workspace.package\]/ { in_workspace_package = 1; next }
  in_workspace_package && /^version = / { print $2; exit }
' "$ROOT/engine/Cargo.toml")"
if [[ -z "$HOST_TARGET" || -z "$PACK_VERSION" ]]; then
  fail "could not resolve workspace pack target or version"
fi

jq -n \
  --arg version "$PACK_VERSION" \
  --arg target "$HOST_TARGET" \
  --arg url "file://$PACK_ARCHIVE" \
  --arg sha256 "$PACK_SHA256" \
  '[{
    id: "compose",
    version: $version,
    api_version: 1,
    targets: {($target): {url: $url, sha256: $sha256}}
  }]' >"$PACK_INDEX"

cp -R "$FIXTURE_SRC/." "$WORK_DIR/"

(
  cd "$WORK_DIR"
  export WAX_HOME="$WORK_DIR/.wax-home"
  export WAX_PACK_INDEX="file://$PACK_INDEX"

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
