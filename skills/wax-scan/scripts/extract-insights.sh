#!/usr/bin/env bash
# Deterministic insights extractor for wax-scan skill (scan schema v3).
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

if ! jq -e '
  (.schema_version | type == "number")
  and (.languages | type == "object")
  and (.token_inference | type == "object")
  and (.token_inference.counts | type == "object")
  and (.token_inference.sites | type == "array")
' "$SCAN" >/dev/null 2>&1; then
  echo "extract-insights.sh: invalid scan-merged.json: $SCAN" >&2
  exit 1
fi

SCAN_SCHEMA="$(jq -r '.schema_version' "$SCAN")"
if [[ "$SCAN_SCHEMA" != "3" ]]; then
  echo "extract-insights.sh: unsupported scan schema_version ${SCAN_SCHEMA}; expected 3" >&2
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
        ds_vs_local_ratio: (
          if (($counts.raw_invocations.resolved // 0) + ($counts.raw_invocations.local // 0)) == 0 then null
          else (($counts.raw_invocations.resolved // 0) / (($counts.raw_invocations.resolved // 0) + ($counts.raw_invocations.local // 0)))
          end
        ),
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
        .repo_summary.counts as $counts
        | {
            languages: (.repo_summary.languages | map(tostring)),
            ds_vs_local_ratio: (repo_metrics_from_counts($counts).ds_vs_local_ratio),
            invocation_adoption_ratio: .repo_summary.metrics.invocation_adoption_ratio,
            registry_resolution_ratio: .repo_summary.metrics.registry_resolution_ratio,
            raw_invocations: $counts.raw_invocations,
            definitions: $counts.definitions,
            registry: $counts.registry,
            adoption: $counts.adoption,
            parent_scopes: $counts.parent_scopes
          }
      else
        repo_counts_from_languages as $counts
        | {
            languages: lang_ids,
            ds_vs_local_ratio: (repo_metrics_from_counts($counts).ds_vs_local_ratio),
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
        ds_vs_local_ratio: (repo_metrics_from_counts($facts.counts).ds_vs_local_ratio),
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

    def unused_registry_components:
      [
        .languages[]
        | .design_system_components[]?
        | {
            symbol: (.symbol // .registry_symbol),
            registry_symbol: (.registry_symbol // .symbol),
            package: (.package // null)
          }
      ] as $registry_components
      | [
          all_usage_sites[]
          | select(.match_status == "resolved")
          | (.registry_symbol // .symbol)
        ] as $resolved_symbols
      | $registry_components
      | unique_by(.registry_symbol)
      | map(select((.registry_symbol as $symbol | ($resolved_symbols | index($symbol))) == null))
      | sort_by(.symbol, .registry_symbol);

    def synthesized_symbol_usage_summary_rows:
      [
        all_usage_sites[]
        | select(.match_status == "local" or .match_status == "unresolved")
        | {
            key: (
              if .match_status == "local" then (.local_definition_id // .symbol)
              else .symbol
              end
            ),
            symbol_id: (
              if .match_status == "local" then
                ("local:" + (.local_definition_id // .symbol))
              else
                ("unresolved:" + .symbol)
              end
            ),
            symbol: .symbol,
            qualified_symbol: (.qualified_symbol // null),
            symbol_kind: .match_status,
            match_status: .match_status,
            registry_symbol: (.registry_symbol // null),
            local_definition_id: (.local_definition_id // null),
            identity_basis: (
              if .match_status == "local" and (.local_definition_id? != null) then
                "local_definition_id"
              else
                "symbol"
              end
            ),
            identity_stability: (
              if .match_status == "local" and (.local_definition_id? != null) then
                "path_sensitive"
              else
                "scan_local"
              end
            ),
            location_file: (.location.file // null),
            parent: (.parent // null)
          }
      ]
      | group_by(.key)
      | map({
          symbol_id: .[0].symbol_id,
          symbol: .[0].symbol,
          qualified_symbol: .[0].qualified_symbol,
          symbol_kind: .[0].symbol_kind,
          match_status: .[0].match_status,
          registry_symbol: .[0].registry_symbol,
          local_definition_id: .[0].local_definition_id,
          identity_basis: .[0].identity_basis,
          identity_stability: .[0].identity_stability,
          raw_invocation_count: length,
          parent_scope_count: ([.[].parent.parent_id?] | map(select(. != null)) | unique | length),
          file_count: ([.[].location_file] | map(select(. != null)) | unique | length),
          parent_scopes: (
            [.[]
              | select(.parent != null)
              | {
                  parent_id: .parent.parent_id,
                  symbol: .parent.symbol,
                  qualified_symbol: (.parent.qualified_symbol // null),
                  scope_kind: .parent.scope_kind,
                  identity_basis: (.parent.identity_basis // null),
                  identity_stability: (.parent.identity_stability // null),
                  invocation_count: 1
                }
            ]
            | group_by(.parent_id)
            | map({
                parent_id: .[0].parent_id,
                symbol: .[0].symbol,
                qualified_symbol: .[0].qualified_symbol,
                scope_kind: .[0].scope_kind,
                identity_basis: .[0].identity_basis,
                identity_stability: .[0].identity_stability,
                invocation_count: length
              })
            | sort_by(-.invocation_count, .symbol)
          ),
          parent_scope_limit: null,
          parent_scopes_truncated: false
        });

    def symbol_usage_summary_rows:
      ((.symbol_usage_summary // []) + ([.languages[] | .symbol_usage_summary[]?])) as $reported_rows
      | synthesized_symbol_usage_summary_rows as $synthetic_rows
      | (($reported_rows + $synthetic_rows)
        | unique_by(
            [
              .symbol_kind,
              (.local_definition_id // ""),
              .symbol,
              (.registry_symbol // "")
            ]
          ));

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
            raw_invocation_count: 1,
            resolved_raw_invocation_count: (if .match_status == "resolved" then 1 else 0 end),
            local_raw_invocation_count: (if .match_status == "local" then 1 else 0 end),
            candidate_raw_invocation_count: (if .match_status == "candidate" then 1 else 0 end),
            unresolved_raw_invocation_count: (if .match_status == "unresolved" then 1 else 0 end)
          }
      ]
      | group_by(.parent_id)
      | map({
          parent_id: .[0].parent_id,
          symbol: .[0].symbol,
          scope_kind: .[0].scope_kind,
          raw_invocation_count: (map(.raw_invocation_count) | add),
          resolved_raw_invocation_count: (map(.resolved_raw_invocation_count) | add),
          local_raw_invocation_count: (map(.local_raw_invocation_count) | add),
          candidate_raw_invocation_count: (map(.candidate_raw_invocation_count) | add),
          unresolved_raw_invocation_count: (map(.unresolved_raw_invocation_count) | add)
        })
      | sort_by(-.raw_invocation_count, .symbol)
      | .[0:$limit];

    def confidence_rank:
      if . == "very_high" then 0
      elif . == "high" then 1
      elif . == "medium" then 2
      elif . == "low" then 3
      else 4
      end;

    def validate_token_inference:
      .token_inference as $inference
      | $inference.sites as $rows
      | ($rows | group_by([.language, .site_id]) | map(select(length > 1))) as $duplicate_keys
      | if ($duplicate_keys | length) > 0 then
          error(
            "duplicate token inference key(s): "
            + ($duplicate_keys
              | map(.[0].language + ":" + .[0].site_id)
              | join(", "))
          )
        else
          .
        end
      | ([$rows[].classification | select(
          . != "exact"
          and . != "near"
          and . != "unmatched"
          and . != "unassessed"
        )] | unique) as $invalid_classifications
      | if ($invalid_classifications | length) > 0 then
          error(
            "unknown token inference classification(s): "
            + ($invalid_classifications | join(", "))
          )
        else
          [
            {
              name: "hardcoded_observation_count",
              reported: $inference.counts.hardcoded_observation_count,
              actual: ($rows | length)
            },
            {
              name: "raw_hardcoded_style_site_count",
              reported: $inference.counts.hardcoded_observation_count,
              actual: ([.languages[] | .hardcoded_style_sites[]?] | length)
            },
            {
              name: "assessed_observation_count",
              reported: $inference.counts.assessed_observation_count,
              actual: ([$rows[] | select(.classification != "unassessed")] | length)
            },
            {
              name: "exact_replacement_candidate_count",
              reported: $inference.counts.exact_replacement_candidate_count,
              actual: ([$rows[] | select(.classification == "exact")] | length)
            },
            {
              name: "near_replacement_candidate_count",
              reported: $inference.counts.near_replacement_candidate_count,
              actual: ([$rows[] | select(.classification == "near")] | length)
            },
            {
              name: "unmatched_observation_count",
              reported: $inference.counts.unmatched_observation_count,
              actual: ([$rows[] | select(.classification == "unmatched")] | length)
            },
            {
              name: "unassessed_observation_count",
              reported: $inference.counts.unassessed_observation_count,
              actual: ([$rows[] | select(.classification == "unassessed")] | length)
            }
          ] as $checks
          | ([$checks[] | select(.reported != .actual)]) as $mismatches
          | if ($mismatches | length) > 0 then
              error(
                "token inference count mismatch(es): "
                + ($mismatches
                  | map(.name + " reported=" + (.reported | tostring) + " actual=" + (.actual | tostring))
                  | join(", "))
              )
            else
              .
            end
        end;

    def raw_style_site_rows:
      [.languages | to_entries[] as $entry
        | $entry.key as $lang
        | $entry.value.hardcoded_style_sites[]?
        | {
            lang: $lang,
            site_id: .id,
            location: .location,
            context: .context,
            value: .value
          }
      ];

    def raw_style_site_index:
      raw_style_site_rows as $rows
      | ($rows | group_by([.lang, .site_id]) | map(select(length > 1))) as $dupes
      | if ($dupes | length) > 0 then
          error(
            "duplicate raw hard-coded style site key(s): "
            + ($dupes | map(.[0].lang + ":" + .[0].site_id) | join(", "))
          )
        else
          (
            $rows
            | map({
                key: (.lang + "\u0000" + .site_id),
                value: { location: .location, context: .context, value: .value }
              })
            | from_entries
          )
        end;

    def enrich_inference_row($index):
      ($index[.language + "\u0000" + .site_id]) as $raw
      | if $raw == null then
          error(
            "token inference row for "
            + .language + ":" + .site_id
            + " did not resolve to exactly one raw hard-coded style site"
          )
        else
          . + { location: $raw.location, context: $raw.context, value: $raw.value }
        end;

    def token_inference_block:
      raw_style_site_index as $index
      | [.token_inference.sites[] | enrich_inference_row($index)] as $enriched
      | {
          summary: .token_inference.counts,
          confirmed_candidates: (
            [$enriched[] | select(.classification == "exact")]
            | sort_by([(.confidence | confidence_rank), .language, .location.file, .location.line])
          ),
          possible_candidates: (
            [$enriched[] | select(.classification == "near")]
            | sort_by([(.confidence | confidence_rank), .language, .location.file, .location.line])
          ),
          unmatched_observations: (
            [$enriched[] | select(.classification == "unmatched")]
            | sort_by([.language, .location.file, .location.line])
          ),
          unassessed_observations: (
            [$enriched[] | select(.classification == "unassessed")]
            | sort_by([.language, .location.file, .location.line])
          )
        };

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

    validate_token_inference
    | {
      schema_version: 3,
      generated_at: $generated_at,
      source_scan: $source_scan,
      repo_summary: repo_summary_block,
      per_language: per_language,
      symbol_rollups: symbol_rollups_block,
      top_local_symbols: top_symbols_by_kind("local"; 5),
      top_unresolved_symbols: top_symbols_by_kind("unresolved"; 5),
      unused_registry_components: unused_registry_components,
      parent_scope_hotspots: parent_scope_hotspots(5),
      fragmentation_candidates: suffix_families,
      token_inference: token_inference_block,
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

  if [[ "$baseline_schema" != "3" ]]; then
    jq --arg reason "Baseline schema_version ${baseline_schema} is incompatible with current v3 scan output; older baselines lack inference classifications and cannot be mixed with v3 denominators" '
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
        $scan.repo_summary.counts as $counts
        | {
            ds_vs_local_ratio: (
              if (($counts.raw_invocations.resolved // 0) + ($counts.raw_invocations.local // 0)) == 0 then null
              else (($counts.raw_invocations.resolved // 0) / (($counts.raw_invocations.resolved // 0) + ($counts.raw_invocations.local // 0)))
              end
            ),
            invocation_adoption_ratio: $scan.repo_summary.metrics.invocation_adoption_ratio,
            registry_resolution_ratio: $scan.repo_summary.metrics.registry_resolution_ratio,
            raw_invocations: $counts.raw_invocations,
            parent_scopes: $counts.parent_scopes
          }
      else
        (repo_counts_from_languages($scan)) as $counts
        | $counts.raw_invocations as $raw
        | $counts.adoption as $adoption
        | {
            ds_vs_local_ratio: (
              if (($raw.resolved // 0) + ($raw.local // 0)) == 0 then null
              else (($raw.resolved // 0) / (($raw.resolved // 0) + ($raw.local // 0)))
              end
            ),
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
        ds_vs_local_ratio: (
          if (($facts.counts.raw_invocations.resolved // 0) + ($facts.counts.raw_invocations.local // 0)) == 0 then null
          else (($facts.counts.raw_invocations.resolved // 0) / (($facts.counts.raw_invocations.resolved // 0) + ($facts.counts.raw_invocations.local // 0)))
          end
        ),
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
        ds_vs_local_ratio: delta_num($cur_repo.ds_vs_local_ratio; $base_repo.ds_vs_local_ratio),
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
