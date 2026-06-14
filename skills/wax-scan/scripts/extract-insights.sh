#!/usr/bin/env bash
# Deterministic insights extractor for wax-scan skill.
set -euo pipefail

usage() {
  echo "Usage: extract-insights.sh <scan-merged.json> [--baseline <path>]" >&2
  exit 1
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "extract-insights.sh requires $1 on PATH" >&2
    exit 1
  fi
}

require_cmd jq

SCAN="${1:-}"
if [[ -z "$SCAN" || ! -f "$SCAN" ]]; then
  usage
fi

BASELINE=""
shift || true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --baseline)
      BASELINE="${2:-}"
      if [[ -z "$BASELINE" ]]; then
        usage
      fi
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

if ! jq -e '.schema_version and .languages' "$SCAN" >/dev/null 2>&1; then
  echo "extract-insights.sh: invalid scan-merged.json: $SCAN" >&2
  exit 1
fi

GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
SOURCE_SCAN="$SCAN"

LIMITS_JSON='[
  {"metric":"Coverage by feature/screen/route/module/team","missing_capability":"Reporting boundary metadata in usage sites"},
  {"metric":"Override rate / override patterns","missing_capability":"Override detection in language packs"},
  {"metric":"Deprecated usage","missing_capability":"Deprecation metadata in registry or facts"},
  {"metric":"Version adoption / upgrade lag","missing_capability":"DS package version facts"},
  {"metric":"Wrapper proliferation","missing_capability":"Composition/wrapper edges in facts"},
  {"metric":"Feature-level coverage","missing_capability":"Feature/module attribution"},
  {"metric":"LOC reduction estimates","missing_capability":"Source line metrics beyond usage sites"}
]'

extract_core() {
  local input="$1"
  jq --arg generated_at "$GENERATED_AT" --arg source_scan "$SOURCE_SCAN" --argjson limits "$LIMITS_JSON" '
    def lang_ids:
      [.languages | keys[]] | sort;

    def per_language_entry($lang_id; $facts):
      ($facts.counts.usage_site_count // 0) as $total
      | ($facts.counts.resolved_count // 0) as $resolved
      | ($facts.counts.candidate_count // 0) as $candidate
      | ($total - $resolved - $candidate) as $unresolved
      | {
          language_id: $lang_id,
          status: $facts.status,
          adoption_coverage_ratio: $facts.metrics.adoption_coverage_ratio,
          resolved_count: $resolved,
          candidate_count: $candidate,
          unresolved_count: $unresolved,
          usage_site_count: $total
        };

    def per_language:
      [.languages | to_entries[] | per_language_entry(.key; .value)] | sort_by(.language_id);

    def repo_totals:
      [.languages[] | .counts] as $counts
      | ($counts | map(.usage_site_count // 0) | add // 0) as $total
      | ($counts | map(.resolved_count // 0) | add // 0) as $resolved
      | ($counts | map(.candidate_count // 0) | add // 0) as $candidate
      | ($total - $resolved - $candidate) as $unresolved
      | {
          languages: lang_ids,
          total_usage_sites: $total,
          resolved_count: $resolved,
          candidate_count: $candidate,
          unresolved_count: $unresolved,
          adoption_coverage_ratio: (if $total == 0 then null else ($resolved / $total) end)
        };

    def symbol_rollup($sites; $field):
      [$sites[]
        | select(.match_status == $field.match_status)
        | .symbol_key = (
            if $field.key == "registry" then (.registry_symbol // .symbol)
            else .symbol
            end
          )
        | .symbol_key
      ]
      | group_by(.)
      | map({symbol: .[0], count: length})
      | sort_by(-.count, .symbol);

    def all_usage_sites:
      [.languages[] | .usage_sites[]?];

    def ds_symbol_rollups:
      [all_usage_sites[]
        | select(.match_status == "resolved" or .match_status == "candidate")
        | (.registry_symbol // .symbol)
      ]
      | group_by(.)
      | map({symbol: .[0], count: length})
      | sort_by(-.count, .symbol);

    def local_symbols:
      [.languages[] | .local_components[]? | .symbol] | unique;

    def local_symbol_rollups:
      local_symbols as $local
      | [all_usage_sites[] | select(.symbol as $s | ($local | index($s)) != null) | .symbol]
      | group_by(.)
      | map({symbol: .[0], count: length})
      | sort_by(-.count, .symbol);

    def unresolved_symbol_rollups:
      symbol_rollup(all_usage_sites; {match_status: "unresolved", key: "symbol"});

    def suffix_families:
      [.languages[] | .local_components[]? | .symbol]
      | unique
      | map(
          select(length > 0)
          | . as $sym
          | (
              if ($sym | test("Button$")) then "*Button"
              elif ($sym | test("Modal$")) then "*Modal"
              else empty
              end
            ) as $pattern
          | select($pattern != null)
          | {pattern: $pattern, symbol: $sym}
        )
      | group_by(.pattern)
      | map({
          pattern: .[0].pattern,
          symbols: ([.[].symbol] | unique | sort),
          count: ([.[].symbol] | unique | length)
        })
      | sort_by(-.count, .pattern)
      | map(select(.count >= 2));

    {
      schema_version: 1,
      generated_at: $generated_at,
      source_scan: $source_scan,
      repo_summary: repo_totals,
      per_language: per_language,
      symbol_rollups: {
        design_system: ds_symbol_rollups,
        local: local_symbol_rollups,
        unresolved: unresolved_symbol_rollups
      },
      fragmentation_candidates: suffix_families,
      limits: $limits,
      baseline_deltas: null
    }
  ' "$input"
}

compute_baseline_deltas() {
  local insights_json="$1"
  local current_scan="$2"
  local baseline_file="$3"

  if [[ ! -f "$baseline_file" ]]; then
    jq --arg reason "Baseline file not found: ${baseline_file}" '
      .limits += [{
        metric: "Baseline comparison",
        missing_capability: $reason
      }]
    ' <<<"$insights_json"
    return
  fi

  if ! jq -e '.schema_version and .languages' "$baseline_file" >/dev/null 2>&1; then
    jq --arg reason "Baseline is not a compatible scan-merged.json" '
      .limits += [{
        metric: "Baseline comparison",
        missing_capability: $reason
      }]
    ' <<<"$insights_json"
    return
  fi

  local current_schema baseline_schema
  current_schema="$(jq -r '.schema_version' "$current_scan")"
  baseline_schema="$(jq -r '.schema_version' "$baseline_file")"

  if [[ "$current_schema" != "$baseline_schema" ]]; then
    jq --arg reason "Baseline schema_version ${baseline_schema} is incompatible with current ${current_schema}" '
      .limits += [{
        metric: "Baseline comparison",
        missing_capability: $reason
      }]
    ' <<<"$insights_json"
    return
  fi

  jq --slurpfile baseline "$baseline_file" --slurpfile current "$current_scan" '
    def lang_ids($scan): [$scan.languages | keys[]] | sort;

    def per_language_entry($lang_id; $facts):
      ($facts.counts.usage_site_count // 0) as $total
      | ($facts.counts.resolved_count // 0) as $resolved
      | ($facts.counts.candidate_count // 0) as $candidate
      | ($total - $resolved - $candidate) as $unresolved
      | {
          language_id: $lang_id,
          adoption_coverage_ratio: $facts.metrics.adoption_coverage_ratio,
          resolved_count: $resolved,
          candidate_count: $candidate,
          unresolved_count: $unresolved
        };

    def per_language_delta($lang_id; $current_scan; $baseline_scan):
      per_language_entry($lang_id; $current_scan.languages[$lang_id]) as $cur
      | per_language_entry($lang_id; $baseline_scan.languages[$lang_id]) as $base
      | {
          language_id: $lang_id,
          adoption_coverage_ratio: (
            if ($cur.adoption_coverage_ratio == null or $base.adoption_coverage_ratio == null) then null
            else ($cur.adoption_coverage_ratio - $base.adoption_coverage_ratio)
            end
          ),
          resolved_count: ($cur.resolved_count - $base.resolved_count),
          candidate_count: ($cur.candidate_count - $base.candidate_count),
          unresolved_count: ($cur.unresolved_count - $base.unresolved_count)
        };

    def repo_totals($scan):
      [$scan.languages[] | .counts] as $counts
      | ($counts | map(.usage_site_count // 0) | add // 0) as $total
      | ($counts | map(.resolved_count // 0) | add // 0) as $resolved
      | ($counts | map(.candidate_count // 0) | add // 0) as $candidate
      | ($total - $resolved - $candidate) as $unresolved
      | {
          resolved_count: $resolved,
          candidate_count: $candidate,
          unresolved_count: $unresolved,
          adoption_coverage_ratio: (if $total == 0 then null else ($resolved / $total) end)
        };

    def delta_num($current; $baseline):
      if ($current == null or $baseline == null) then null else ($current - $baseline) end;

    def lang_intersection($current_ids; $baseline_ids):
      [$current_ids[] | select(. as $id | ($baseline_ids | index($id)) != null)];

    def only_in($ids; $other):
      [$ids[] | select(. as $id | ($other | index($id)) == null)];

    . as $insights
    | $current[0] as $current_scan
    | $baseline[0] as $baseline_scan
    | (lang_ids($current_scan)) as $current_lang_ids
    | (lang_ids($baseline_scan)) as $baseline_lang_ids
    | (repo_totals($current_scan)) as $cur_repo
    | (repo_totals($baseline_scan)) as $base_repo
    | (lang_intersection($current_lang_ids; $baseline_lang_ids)) as $shared_lang_ids
    | $insights
    | .baseline_deltas = {
        adoption_coverage_ratio: delta_num($cur_repo.adoption_coverage_ratio; $base_repo.adoption_coverage_ratio),
        resolved_count: ($cur_repo.resolved_count - $base_repo.resolved_count),
        candidate_count: ($cur_repo.candidate_count - $base_repo.candidate_count),
        unresolved_count: ($cur_repo.unresolved_count - $base_repo.unresolved_count),
        per_language: [
          $shared_lang_ids[]
          | per_language_delta(.; $current_scan; $baseline_scan)
        ]
      }
    | if ($current_lang_ids != $baseline_lang_ids) then
        .limits += [{
          metric: "Per-language baseline deltas",
          missing_capability: (
            "Language sets differ between current and baseline scans. "
            + "Per-language deltas include only comparable languages ("
            + ($shared_lang_ids | join(", "))
            + "). Missing from baseline: "
            + ((only_in($current_lang_ids; $baseline_lang_ids) | if length == 0 then "none" else join(", ") end))
            + ". Missing from current: "
            + ((only_in($baseline_lang_ids; $current_lang_ids) | if length == 0 then "none" else join(", ") end))
            + "."
          )
        }]
      else .
      end
  ' <<<"$insights_json"
}

INSIGHTS="$(extract_core "$SCAN")"

if [[ -n "$BASELINE" ]]; then
  INSIGHTS="$(compute_baseline_deltas "$INSIGHTS" "$SCAN" "$BASELINE")"
fi

printf '%s\n' "$INSIGHTS"
