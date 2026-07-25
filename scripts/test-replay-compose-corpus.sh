#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="$repo_root/scripts/replay-compose-corpus.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

assert_eq() {
  [[ "$1" == "$2" ]] || { printf '%s: expected %q got %q\n' "$3" "$2" "$1" >&2; exit 1; }
}

repo="$tmp/corpus"
mkdir -p "$repo/.wax/out"
printf '{}\n' > "$repo/.wax/wax.config.json"

fake_wax="$tmp/wax"
cat > "$fake_wax" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cp "${FAKE_SCAN:?}" "${REPO_ROOT:?}/.wax/out/scan-merged.json"
EOF
chmod +x "$fake_wax"

unknown_msg='tree-sitter could not fully parse unknown syntax in App.kt near 1:1; file scanned with gaps'
unknown_key="error|parse_failed|${unknown_msg}||null|null"

write_scan() {
  local status="$1" ms="$2" usage_id="$3" message="${4-}"
  local diagnostics="[]"
  if [[ -n "$message" ]]; then
    diagnostics="$(jq -cn --arg m "$message" \
      '[{severity:"error",code:"parse_failed",message:$m,location:null}]')"
  fi
  jq -n --arg status "$status" --argjson ms "$ms" --arg usage "$usage_id" \
    --argjson diagnostics "$diagnostics" '
    {
      schema_version: 1,
      recorded_at: "1970-01-01T00:00:00Z",
      repo_summary: {
        languages: ["compose"],
        counts: {},
        metrics: {parse_extract_ms: $ms, files_scanned: 1}
      },
      symbol_usage_summary: [],
      token_usage_summary: [],
      token_inference: {schema_version: 1, near_match_threshold: 2.0, rows: [], counts: {}},
      languages: {
        compose: {
          status: $status,
          diagnostics: $diagnostics,
          local_components: [{id: "local:Screen"}],
          usage_sites: [{id: $usage}],
          token_sites: [],
          hardcoded_style_sites: []
        }
      }
    }
  ' > "$tmp/scan.json"
}

write_baseline() {
  local diag_keys_json="${1:-[]}"
  jq -n --argjson keys "$diag_keys_json" '
    {
      status: "complete",
      parse_failure_count: ($keys | length),
      files_scanned: 1,
      usage_site_ids: ["usage:Button"],
      local_component_ids: ["local:Screen"],
      token_site_ids: [],
      hardcoded_style_site_ids: [],
      diagnostic_keys: $keys,
      parse_extract_ms: 100,
      baseline_parse_extract_ms: 100,
      slowdown_percent: 0.0,
      expected_added_ids: [],
      expected_removed_false_positive_ids: [],
      expected_added_diagnostic_keys: [],
      expected_removed_diagnostic_keys: []
    }
  ' > "$tmp/baseline.json"
}

run_replay() {
  REPO_ROOT="$repo" FAKE_SCAN="$tmp/scan.json" \
    "$script" --repo "$repo" --wax-bin "$fake_wax" --baseline "$tmp/baseline.json" \
      --max-slowdown-percent 10
}

write_scan partial 105 "usage:Button" "$unknown_msg"
write_baseline "$(jq -cn --arg k "$unknown_key" '[$k]')"
run_replay >/dev/null

write_scan complete 100 "usage:Other" "$unknown_msg"
write_baseline "$(jq -cn --arg k "$unknown_key" '[$k]')"
set +e
out="$(run_replay 2>&1)"
status=$?
set -e
assert_eq "$status" "1" "lost id exit"
printf '%s\n' "$out" | grep -F "lost ids" >/dev/null

write_scan partial 100 "usage:Button" \
  "tree-sitter could not fully parse when guard syntax in App.kt near 2:1; skipped the uncertain region and continued scanning later source"
write_baseline "$(jq -cn --arg k "$unknown_key" '[$k]')"
set +e
out="$(run_replay 2>&1)"
status=$?
set -e
assert_eq "$status" "1" "known family exit"
printf '%s\n' "$out" | grep -F "known-family" >/dev/null

# Diagnostic delta: baseline expects a diagnostic the scan no longer emits.
write_scan complete 100 "usage:Button"
write_baseline "$(jq -cn --arg k "$unknown_key" '[$k]')"
set +e
out="$(run_replay 2>&1)"
status=$?
set -e
assert_eq "$status" "1" "lost diagnostic exit"
printf '%s\n' "$out" | grep -F "lost diagnostics" >/dev/null

write_scan complete 120 "usage:Button"
write_baseline '[]'
set +e
out="$(run_replay 2>&1)"
status=$?
set -e
assert_eq "$status" "1" "slowdown exit"
printf '%s\n' "$out" | grep -F "slowdown" >/dev/null

printf 'ok: test-replay-compose-corpus\n'
