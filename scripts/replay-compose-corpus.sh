#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'usage: replay-compose-corpus.sh --repo <absolute-path> --wax-bin <absolute-path> --baseline <json> [--max-slowdown-percent 10]\n' >&2
  exit 2
}

repo="" wax_bin="" baseline="" max_slowdown_percent=10
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) [[ $# -ge 2 ]] || usage; repo="$2"; shift 2 ;;
    --wax-bin) [[ $# -ge 2 ]] || usage; wax_bin="$2"; shift 2 ;;
    --baseline) [[ $# -ge 2 ]] || usage; baseline="$2"; shift 2 ;;
    --max-slowdown-percent) [[ $# -ge 2 ]] || usage; max_slowdown_percent="$2"; shift 2 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; usage ;;
  esac
done

[[ -n "$repo" && -n "$wax_bin" && -n "$baseline" ]] || usage
[[ "$repo" == /* && "$wax_bin" == /* && "$baseline" == /* ]] || {
  printf 'repo, wax-bin, and baseline must be absolute paths\n' >&2
  exit 2
}
[[ -x "$wax_bin" && -f "$baseline" && -f "$repo/.wax/wax.config.json" ]] || {
  printf 'missing wax-bin, baseline, or %s/.wax/wax.config.json\n' "$repo" >&2
  exit 2
}
command -v jq >/dev/null || { printf 'jq is required\n' >&2; exit 2; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

"$wax_bin" scan --repo-root "$repo" --no-auto-install >/dev/null
scan_out="$repo/.wax/out/scan-merged.json"
[[ -f "$scan_out" ]] || { printf 'scan did not write %s\n' "$scan_out" >&2; exit 1; }
cp "$scan_out" "$tmp/scan.json"

jq -n --slurpfile s "$tmp/scan.json" --slurpfile b "$baseline" '
  def lang_ids(key): [$s[0].languages[]?[key][]?.id] | unique | sort;
  def diag_key:
    [
      (.severity // ""),
      (.code // ""),
      (.message // ""),
      (.location.file // ""),
      ((.location.line // null) | tostring),
      ((.location.column // null) | tostring)
    ] | join("|");
  def statuses: [$s[0].languages[]?.status // empty] | unique;
  def status_label:
    if (statuses | index("failed")) then "failed"
    elif (statuses | index("partial")) then "partial"
    else "complete" end;
  ($s[0].repo_summary.metrics.parse_extract_ms // 0) as $ms
  | ($b[0].parse_extract_ms // $b[0].baseline_parse_extract_ms // 0) as $base_ms
  | {
      status: status_label,
      parse_failure_count: (
        [$s[0].languages[]?.diagnostics[]? | select(.code == "parse_failed")] | length
      ),
      files_scanned: ($s[0].repo_summary.metrics.files_scanned // 0),
      usage_site_ids: lang_ids("usage_sites"),
      local_component_ids: lang_ids("local_components"),
      token_site_ids: lang_ids("token_sites"),
      hardcoded_style_site_ids: lang_ids("hardcoded_style_sites"),
      diagnostic_keys: ([$s[0].languages[]?.diagnostics[]? | diag_key] | unique | sort),
      parse_extract_ms: $ms,
      baseline_parse_extract_ms: $base_ms,
      slowdown_percent: (if $base_ms == 0 then 0.0 else (($ms - $base_ms) * 100.0 / $base_ms) end)
    }
' | tee "$tmp/report.json"

fail() { printf '%s\n' "$1" >&2; exit 1; }

known="$(jq '
  [.languages[]?.diagnostics[]?
    | select(.code == "parse_failed")
    | select(.message | test("could not fully parse (?!unknown )"; "i"))
  ] | length
' "$tmp/scan.json")"
[[ "$known" == "0" ]] || fail "known-family parse_failed diagnostics present"

set +e
gate_err="$(jq -n --slurpfile r "$tmp/report.json" --slurpfile b "$baseline" --argjson max "$max_slowdown_percent" '
  def all_ids:
    (.usage_site_ids // [])
    + (.local_component_ids // [])
    + (.token_site_ids // [])
    + (.hardcoded_style_site_ids // []);
  def baseline_diag_keys:
    if ($b[0] | has("diagnostic_keys")) then ($b[0].diagnostic_keys // [])
    else
      [
        $b[0].languages[]?.diagnostics[]?
        | [
            (.severity // ""),
            (.code // ""),
            (.message // ""),
            (.location.file // ""),
            ((.location.line // null) | tostring),
            ((.location.column // null) | tostring)
          ] | join("|")
      ]
    end | unique | sort;
  ($r[0] | all_ids | unique) as $have
  | ($b[0] | all_ids | unique) as $want
  | ($b[0].expected_added_ids // []) as $exp_add
  | ($b[0].expected_removed_false_positive_ids // []) as $exp_rem
  | ($r[0].diagnostic_keys // []) as $have_diag
  | (baseline_diag_keys) as $want_diag
  | ($b[0].expected_added_diagnostic_keys // []) as $exp_diag_add
  | ($b[0].expected_removed_diagnostic_keys // []) as $exp_diag_rem
  | ($want - $have - $exp_rem) as $lost
  | ($have - $want - $exp_add) as $added
  | ($want_diag - $have_diag - $exp_diag_rem) as $lost_diag
  | ($have_diag - $want_diag - $exp_diag_add) as $added_diag
  | if $r[0].status == "failed" then error("pack status failed")
    elif ($lost | length) > 0 then error("lost ids: \($lost)")
    elif ($added | length) > 0 then error("unattributed added ids: \($added)")
    elif ($lost_diag | length) > 0 then error("lost diagnostics: \($lost_diag)")
    elif ($added_diag | length) > 0 then error("unattributed added diagnostics: \($added_diag)")
    elif ($r[0].slowdown_percent > ($max | tonumber)) then
      error("slowdown \($r[0].slowdown_percent)% exceeds \($max)%")
    else empty end
' 2>&1)"
gate_status=$?
set -e
[[ "$gate_status" -eq 0 ]] || fail "$gate_err"
