#!/usr/bin/env bash
# Deterministic insights extractor for wax-scan skill (Adoption Metrics v2).
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

SCAN_SCHEMA="$(jq -r '.schema_version' "$SCAN")"
if [[ "$SCAN_SCHEMA" != "2" ]]; then
  echo "extract-insights.sh: unsupported scan schema_version ${SCAN_SCHEMA}; expected 2" >&2
  exit 1
fi

GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
SOURCE_SCAN="$SCAN"
REPO_ROOT="$(git -C "$(dirname "$SCAN")" rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -n "$REPO_ROOT" && "$SCAN" == "$REPO_ROOT"/* ]]; then
  SOURCE_SCAN="${SCAN#"$REPO_ROOT"/}"
fi

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

    def repo_counts_from_languages:
      reduce (.languages[] | .counts) as $counts (
        {
          registry: {
            component_count: 0,
            used_component_count: 0,
            resolved_raw_invocation_count: 0,
            candidate_raw_invocation_count: 0
          },
          definitions: {
            local_definition_count: 0,
            invoked_local_definition_count: 0,
            unused_local_definition_count: 0
          },
          raw_invocations: {
            total: 0,
            resolved: 0,
            local: 0,
            candidate: 0,
            unresolved: 0
          },
          adoption: {
            eligible_invocation_count: 0,
            adopted_invocation_count: 0,
            non_adopted_invocation_count: 0
          },
          parent_scopes: {
            total: 0,
            with_resolved_invocations: 0,
            with_local_invocations: 0,
            with_unresolved_invocations: 0
          }
        };
        .registry.component_count += ($counts.registry.component_count // 0)
        | .registry.used_component_count += ($counts.registry.used_component_count // 0)
        | .registry.resolved_raw_invocation_count += ($counts.registry.resolved_raw_invocation_count // 0)
        | .registry.candidate_raw_invocation_count += ($counts.registry.candidate_raw_invocation_count // 0)
        | .definitions.local_definition_count += ($counts.definitions.local_definition_count // 0)
        | .definitions.invoked_local_definition_count += ($counts.definitions.invoked_local_definition_count // 0)
        | .definitions.unused_local_definition_count += ($counts.definitions.unused_local_definition_count // 0)
        | .raw_invocations.total += ($counts.raw_invocations.total // 0)
        | .raw_invocations.resolved += ($counts.raw_invocations.resolved // 0)
        | .raw_invocations.local += ($counts.raw_invocations.local // 0)
        | .raw_invocations.candidate += ($counts.raw_invocations.candidate // 0)
        | .raw_invocations.unresolved += ($counts.raw_invocations.unresolved // 0)
        | .adoption.eligible_invocation_count += ($counts.adoption.eligible_invocation_count // 0)
        | .adoption.adopted_invocation_count += ($counts.adoption.adopted_invocation_count // 0)
        | .adoption.non_adopted_invocation_count += ($counts.adoption.non_adopted_invocation_count // 0)
        | .parent_scopes.total += ($counts.parent_scopes.total // 0)
        | .parent_scopes.with_resolved_invocations += ($counts.parent_scopes.with_resolved_invocations // 0)
        | .parent_scopes.with_local_invocations += ($counts.parent_scopes.with_local_invocations // 0)
        | .parent_scopes.with_unresolved_invocations += ($counts.parent_scopes.with_unresolved_invocations // 0)
      );

    def repo_metrics_from_counts($counts):
      {
        invocation_adoption_ratio: (
          if ($counts.adoption.eligible_invocation_count // 0) == 0 then null
          else (($counts.adoption.adopted_invocation_count // 0) / $counts.adoption.eligible_invocation_count)
          end
        ),
        registry_resolution_ratio: (
          if ($counts.raw_invocations.total // 0) == 0 then null
          else (($counts.raw_invocations.resolved // 0) / $counts.raw_invocations.total)
          end
        )
      };

    def repo_summary_block:
      if .repo_summary? != null then
        {
          languages: (.repo_summary.languages | map(tostring)),
          invocation_adoption_ratio: .repo_summary.metrics.invocation_adoption_ratio,
          registry_resolution_ratio: .repo_summary.metrics.registry_resolution_ratio,
          raw_invocations: .repo_summary.counts.raw_invocations,
          definitions: .repo_summary.counts.definitions,
          registry: .repo_summary.counts.registry,
          adoption: .repo_summary.counts.adoption,
          parent_scopes: .repo_summary.counts.parent_scopes
        }
      else
        repo_counts_from_languages as $counts
        | {
            languages: lang_ids,
            invocation_adoption_ratio: (repo_metrics_from_counts($counts).invocation_adoption_ratio),
            registry_resolution_ratio: (repo_metrics_from_counts($counts).registry_resolution_ratio),
            raw_invocations: $counts.raw_invocations,
            definitions: $counts.definitions,
            registry: $counts.registry,
            adoption: $counts.adoption,
            parent_scopes: $counts.parent_scopes
          }
      end;

    def per_language_entry($lang_id; $facts):
      {
        language_id: $lang_id,
        status: $facts.status,
        invocation_adoption_ratio: $facts.metrics.invocation_adoption_ratio,
        registry_resolution_ratio: $facts.metrics.registry_resolution_ratio,
        raw_invocations: $facts.counts.raw_invocations,
        definitions: $facts.counts.definitions,
        parent_scopes: $facts.counts.parent_scopes
      };

    def per_language:
      [.languages | to_entries[] | per_language_entry(.key; .value)] | sort_by(.language_id);

    def all_usage_sites:
      [.languages[] | .usage_sites[]?];

    def symbol_rollup($sites; $status; $key):
      [$sites[]
        | select(.match_status == $status)
        | if $key == "registry" then (.registry_symbol // .symbol) else .symbol end
      ]
      | group_by(.)
      | map({symbol: .[0], count: length})
      | sort_by(-.count, .symbol);

    def symbol_rollups_block:
      {
        design_system: symbol_rollup(all_usage_sites; "resolved"; "registry"),
        candidate: symbol_rollup(all_usage_sites; "candidate"; "registry"),
        local: symbol_rollup(all_usage_sites; "local"; "symbol"),
        unresolved: symbol_rollup(all_usage_sites; "unresolved"; "symbol")
      };

    def symbol_usage_summary_rows:
      if (.symbol_usage_summary? | length) > 0 then
        .symbol_usage_summary
      else
        [.languages[] | .symbol_usage_summary[]?]
      end;

    def top_symbols_by_kind($kind; $limit):
      [symbol_usage_summary_rows[]
        | select(.symbol_kind == $kind)
      ]
      | sort_by(-.raw_invocation_count, .symbol)
      | .[0:$limit];

    def parent_scope_hotspots($limit):
      [all_usage_sites[]
        | select(.parent? != null)
        | {
            parent_id: .parent.parent_id,
            symbol: .parent.symbol,
            scope_kind: .parent.scope_kind,
            invocation_count: 1
          }
      ]
      | group_by(.parent_id)
      | map({
          parent_id: .[0].parent_id,
          symbol: .[0].symbol,
          scope_kind: .[0].scope_kind,
          invocation_count: (map(.invocation_count) | add)
        })
      | sort_by(-.invocation_count, .symbol)
      | .[0:$limit];

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
      schema_version: 2,
      generated_at: $generated_at,
      source_scan: $source_scan,
      repo_summary: repo_summary_block,
      per_language: per_language,
      symbol_rollups: symbol_rollups_block,
      top_local_symbols: top_symbols_by_kind("local"; 5),
      top_unresolved_symbols: top_symbols_by_kind("unresolved"; 5),
      parent_scope_hotspots: parent_scope_hotspots(5),
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

  local baseline_schema
  baseline_schema="$(jq -r '.schema_version' "$baseline_file")"

  if [[ "$baseline_schema" != "2" ]]; then
    jq --arg reason "Baseline schema_version ${baseline_schema} is incompatible with current v2 scan output; v1 baselines cannot be mixed with v2 denominators" '
      .limits += [{
        metric: "Baseline comparison",
        missing_capability: $reason
      }]
    ' <<<"$insights_json"
    return
  fi

  jq --slurpfile baseline "$baseline_file" --slurpfile current "$current_scan" '
    def lang_ids($scan): [$scan.languages | keys[]] | sort;

    def repo_counts_from_languages($scan):
      reduce ($scan.languages[] | .counts) as $counts (
        {
          raw_invocations: {total: 0, resolved: 0, local: 0, candidate: 0, unresolved: 0},
          adoption: {eligible_invocation_count: 0, adopted_invocation_count: 0, non_adopted_invocation_count: 0},
          parent_scopes: {total: 0, with_resolved_invocations: 0, with_local_invocations: 0, with_unresolved_invocations: 0}
        };
        .raw_invocations.total += ($counts.raw_invocations.total // 0)
        | .raw_invocations.resolved += ($counts.raw_invocations.resolved // 0)
        | .raw_invocations.local += ($counts.raw_invocations.local // 0)
        | .raw_invocations.candidate += ($counts.raw_invocations.candidate // 0)
        | .raw_invocations.unresolved += ($counts.raw_invocations.unresolved // 0)
        | .adoption.eligible_invocation_count += ($counts.adoption.eligible_invocation_count // 0)
        | .adoption.adopted_invocation_count += ($counts.adoption.adopted_invocation_count // 0)
        | .adoption.non_adopted_invocation_count += ($counts.adoption.non_adopted_invocation_count // 0)
        | .parent_scopes.total += ($counts.parent_scopes.total // 0)
        | .parent_scopes.with_resolved_invocations += ($counts.parent_scopes.with_resolved_invocations // 0)
        | .parent_scopes.with_local_invocations += ($counts.parent_scopes.with_local_invocations // 0)
        | .parent_scopes.with_unresolved_invocations += ($counts.parent_scopes.with_unresolved_invocations // 0)
      );

    def repo_block($scan):
      if $scan.repo_summary? != null then
        {
          invocation_adoption_ratio: $scan.repo_summary.metrics.invocation_adoption_ratio,
          registry_resolution_ratio: $scan.repo_summary.metrics.registry_resolution_ratio,
          raw_invocations: $scan.repo_summary.counts.raw_invocations,
          parent_scopes: $scan.repo_summary.counts.parent_scopes
        }
      else
        (repo_counts_from_languages($scan)) as $counts
        | $counts.raw_invocations as $raw
        | $counts.adoption as $adoption
        | {
            invocation_adoption_ratio: (
              if ($adoption.eligible_invocation_count // 0) == 0 then null
              else (($adoption.adopted_invocation_count // 0) / $adoption.eligible_invocation_count)
              end
            ),
            registry_resolution_ratio: (
              if $raw.total == 0 then null else ($raw.resolved / $raw.total) end
            ),
            raw_invocations: $raw,
            parent_scopes: $counts.parent_scopes
          }
      end;

    def per_language_entry($lang_id; $facts):
      {
        language_id: $lang_id,
        invocation_adoption_ratio: $facts.metrics.invocation_adoption_ratio,
        registry_resolution_ratio: $facts.metrics.registry_resolution_ratio,
        raw_invocations: $facts.counts.raw_invocations
      };

    def per_language_delta($lang_id; $current_scan; $baseline_scan):
      per_language_entry($lang_id; $current_scan.languages[$lang_id]) as $cur
      | per_language_entry($lang_id; $baseline_scan.languages[$lang_id]) as $base
      | {
          language_id: $lang_id,
          invocation_adoption_ratio: (
            if ($cur.invocation_adoption_ratio == null or $base.invocation_adoption_ratio == null) then null
            else ($cur.invocation_adoption_ratio - $base.invocation_adoption_ratio)
            end
          ),
          registry_resolution_ratio: (
            if ($cur.registry_resolution_ratio == null or $base.registry_resolution_ratio == null) then null
            else ($cur.registry_resolution_ratio - $base.registry_resolution_ratio)
            end
          ),
          raw_invocations: {
            total: (($cur.raw_invocations.total // 0) - ($base.raw_invocations.total // 0)),
            resolved: (($cur.raw_invocations.resolved // 0) - ($base.raw_invocations.resolved // 0)),
            local: (($cur.raw_invocations.local // 0) - ($base.raw_invocations.local // 0)),
            candidate: (($cur.raw_invocations.candidate // 0) - ($base.raw_invocations.candidate // 0)),
            unresolved: (($cur.raw_invocations.unresolved // 0) - ($base.raw_invocations.unresolved // 0))
          }
        };

    def delta_num($current; $baseline):
      if ($current == null or $baseline == null) then null else ($current - $baseline) end;

    def lang_intersection($current_ids; $baseline_ids):
      [$current_ids[] | select(. as $id | ($baseline_ids | index($id)) != null)];

    def only_in($ids; $other):
      [$ids[] | select(. as $id | ($other | index($id)) == null)];

    def symbol_rows($scan):
      if ($scan.symbol_usage_summary? | length) > 0 then
        $scan.symbol_usage_summary
      else
        [$scan.languages[] | .symbol_usage_summary[]?]
      end;

    def symbol_map($scan):
      symbol_rows($scan)
      | map({key: .symbol_id, value: {
          symbol: .symbol,
          symbol_kind: .symbol_kind,
          match_status: .match_status,
          raw_invocation_count: (.raw_invocation_count // 0),
          file_count: (.file_count // 0),
          parent_scope_count: (.parent_scope_count // 0)
        }})
      | from_entries;

    def symbol_delta($symbol_id; $cur_symbols; $base_symbols):
      ($cur_symbols[$symbol_id] // {}) as $cur
      | ($base_symbols[$symbol_id] // {}) as $base
      | {
          symbol_id: $symbol_id,
          symbol: ($cur.symbol // $base.symbol),
          symbol_kind: ($cur.symbol_kind // $base.symbol_kind),
          match_status: ($cur.match_status // $base.match_status),
          raw_invocation_count: (($cur.raw_invocation_count // 0) - ($base.raw_invocation_count // 0)),
          file_count: (($cur.file_count // 0) - ($base.file_count // 0)),
          parent_scope_count: (($cur.parent_scope_count // 0) - ($base.parent_scope_count // 0))
        };

    . as $insights
    | $current[0] as $current_scan
    | $baseline[0] as $baseline_scan
    | (lang_ids($current_scan)) as $current_lang_ids
    | (lang_ids($baseline_scan)) as $baseline_lang_ids
    | (repo_block($current_scan)) as $cur_repo
    | (repo_block($baseline_scan)) as $base_repo
    | (lang_intersection($current_lang_ids; $baseline_lang_ids)) as $shared_lang_ids
    | (symbol_map($current_scan)) as $cur_symbols
    | (symbol_map($baseline_scan)) as $base_symbols
    | ((($cur_symbols | keys) + ($base_symbols | keys)) | unique | sort) as $symbol_ids
    | $insights
    | .baseline_deltas = {
        invocation_adoption_ratio: delta_num($cur_repo.invocation_adoption_ratio; $base_repo.invocation_adoption_ratio),
        registry_resolution_ratio: delta_num($cur_repo.registry_resolution_ratio; $base_repo.registry_resolution_ratio),
        raw_invocations: {
          total: (($cur_repo.raw_invocations.total // 0) - ($base_repo.raw_invocations.total // 0)),
          resolved: (($cur_repo.raw_invocations.resolved // 0) - ($base_repo.raw_invocations.resolved // 0)),
          local: (($cur_repo.raw_invocations.local // 0) - ($base_repo.raw_invocations.local // 0)),
          candidate: (($cur_repo.raw_invocations.candidate // 0) - ($base_repo.raw_invocations.candidate // 0)),
          unresolved: (($cur_repo.raw_invocations.unresolved // 0) - ($base_repo.raw_invocations.unresolved // 0))
        },
        parent_scopes: {
          total: (($cur_repo.parent_scopes.total // 0) - ($base_repo.parent_scopes.total // 0))
        },
        symbol_usage_summary: [
          $symbol_ids[]
          | symbol_delta(.; $cur_symbols; $base_symbols)
        ],
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
