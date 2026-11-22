#!/usr/bin/env bash
# Verifies docs-coverage.sh against the golden skill examples plus sparse entries.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
COVERAGE="$ROOT/skills/wax-refine-docs/scripts/docs-coverage.sh"
EXAMPLES="$ROOT/skills/wax-refine-docs/examples"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

[[ -x "$COVERAGE" ]] || fail "docs-coverage.sh is not executable: $COVERAGE"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/wax-refine-docs.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT
registry="$tmp_dir/registry.json"

jq -n \
  --slurpfile compose "$EXAMPLES/compose/AcmeButton.expected.json" \
  --slurpfile swift "$EXAMPLES/swift/AcmeButton.expected.json" \
  --slurpfile react "$EXAMPLES/react/AcmeButton.expected.json" '
  {
    schema_version: 1,
    components: [
      {id: "compose.button", symbol: "AcmeButton", details: $compose[0]},
      {id: "swift.button", symbol: "AcmeButton", details: $swift[0]},
      {id: "react.button", symbol: "AcmeButton", details: $react[0]},
      {
        id: "react.card",
        symbol: "Card",
        details: {
          status: "partial",
          signature: {parameters: [{name: "title"}, {name: "tone", description: "Surface tone."}]},
          field_gaps: [{field: "signature.parameters", reason: "inherited props"}]
        }
      },
      {id: "react.badge", symbol: "Badge"}
    ]
  }' >"$registry"

actual="$tmp_dir/coverage.json"
"$COVERAGE" "$registry" >"$actual"

jq -e '
  .totals == {
    components: 5,
    complete: 3,
    documented: 3,
    parameters: 15,
    described_parameters: 14,
    with_guidance: 3,
    with_examples: 3,
    fully_refined: 3
  }
' "$actual" >/dev/null || fail "unexpected totals: $(jq -c .totals "$actual")"

jq -e '
  [.components[].id] == ["react.card", "react.badge", "compose.button", "react.button", "swift.button"]
' "$actual" >/dev/null || fail "components must sort worst coverage first, then id"

jq -e '
  (.components[] | select(.id == "react.card")) as $card
  | $card.issues == [
      "extraction_partial",
      "missing_documentation",
      "undocumented_parameters",
      "missing_guidance",
      "missing_example"
    ]
  and $card.parameters.undocumented == ["title"]
  and ($card.field_gaps | length) == 1
' "$actual" >/dev/null || fail "partial component issues were not reported"

jq -e '
  (.components[] | select(.id == "react.badge")) as $badge
  | $badge.status == "missing" and ($badge.issues | first) == "extraction_missing"
' "$actual" >/dev/null || fail "component without details must report extraction_missing"

jq -e 'all(.components[] | select(.id | endswith(".button")); .issues == [])' "$actual" >/dev/null \
  || fail "golden examples must be fully refined"

if "$COVERAGE" >/dev/null 2>&1; then
  fail "missing argument must fail"
fi

echo "OK: wax-refine-docs coverage"
