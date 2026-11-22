#!/usr/bin/env bash
# Summarizes how much source documentation Wax extracted for each registry component.
#
# Usage: docs-coverage.sh <registry.json>
#
# The input is the registry JSON printed by `wax registry discover --dry-run` (or a
# committed registry file). Output is deterministic JSON on stdout: a totals object
# and one entry per component, worst coverage first.
set -euo pipefail

if [[ $# -ne 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  echo "usage: $(basename "$0") <registry.json>" >&2
  exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

jq '
  def entry:
    (.details // {}) as $details
    | ($details.signature.parameters // []) as $parameters
    | ([$parameters[] | select((.description // "") == "") | .name]) as $undocumented
    | ([$details.attachments[]? | select(.kind == "example")] | length) as $examples
    | {
        id,
        symbol,
        status: ($details.status // "missing"),
        documentation: (($details.documentation // "") != ""),
        parameters: {
          total: ($parameters | length),
          described: (($parameters | length) - ($undocumented | length)),
          undocumented: $undocumented
        },
        guidance: ($details.best_practices // [] | length),
        examples: $examples,
        deprecated: ($details.deprecation != null),
        field_gaps: ($details.field_gaps // [])
      }
    | .issues = [
        (if .status != "complete" then "extraction_" + .status else empty end),
        (if .documentation | not then "missing_documentation" else empty end),
        (if .parameters.undocumented != [] then "undocumented_parameters" else empty end),
        (if .guidance == 0 then "missing_guidance" else empty end),
        (if .examples == 0 then "missing_example" else empty end)
      ];

  [.components[]? | entry] as $entries
  | {
      totals: {
        components: ($entries | length),
        complete: ([$entries[] | select(.status == "complete")] | length),
        documented: ([$entries[] | select(.documentation)] | length),
        parameters: ([$entries[].parameters.total] | add // 0),
        described_parameters: ([$entries[].parameters.described] | add // 0),
        with_guidance: ([$entries[] | select(.guidance > 0)] | length),
        with_examples: ([$entries[] | select(.examples > 0)] | length),
        fully_refined: ([$entries[] | select(.issues == [])] | length)
      },
      components: ($entries | sort_by(-(.issues | length), .id))
    }
' "$1"
