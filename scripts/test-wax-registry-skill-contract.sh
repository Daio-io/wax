#!/usr/bin/env bash
# Skill-contract smoke test for reviewed registry maintenance.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v jq >/dev/null 2>&1; then
  echo "FAIL: jq is required" >&2
  exit 1
fi

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

require_text() {
  local file="$1"
  local text="$2"
  grep -Fq -- "$text" "$file" || {
    echo "FAIL: expected $file to contain: $text" >&2
    exit 1
  }
}

require_text "skills/wax-registry-discover/SKILL.md" "structured diff"
require_text "skills/wax-registry-discover/SKILL.md" "explicit approval"
require_text "skills/wax-registry-discover/SKILL.md" "Never delete"
require_text "skills/wax-registry-discover/SKILL.md" 'For an existing registry, use `apply_patch` only'
require_text "skills/wax-registry-discover/SKILL.md" "Aliases/tags proposed"
require_text "skills/wax-registry-discover/SKILL.md" 'symbol, id, package path, documentation in `details`, and common UI vocabulary'
require_text "skills/wax-registry-discover/SKILL.md" "Never treat component aliases as alternate code symbols or scan authority"
require_text "skills/wax-registry-discover/SKILL.md" "Component aliases are search synonyms"
require_text "skills/wax-registry-discover/SKILL.md" "Token aliases are exact source identifiers"
require_text "skills/wax-registry-discover/SKILL.md" "examples/component-alias-enrichment.md"
require_text "skills/wax-registry-discover/token-value-maintenance.md" "source evidence"
require_text "skills/wax-registry-discover/token-value-maintenance.md" 'For an existing registry, use `apply_patch` only'
require_text "skills/wax-registry-discover/token-value-maintenance.md" "Source file / line"
require_text "skills/wax-registry-discover/token-value-maintenance.md" "Resolution explanation"
require_text "skills/wax-registry-discover/token-value-maintenance.md" "Confidence"
require_text "skills/wax-registry-discover/token-value-maintenance.md" "Computed expressions"
require_text "skills/wax-registry-discover/token-value-maintenance.md" "separate approval for removals"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Before registry"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Proposed diff"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Explicit approval"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "After registry"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Before maintenance result"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "After maintenance result"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Unassessed observations: 1"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Confirmed migration candidates: 1"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "Unassessed observations: 0"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "validation passed"
require_text "skills/wax-registry-discover/examples/token-value-refresh.md" "updated compose registry from"
require_text "skills/wax-registry-discover/examples/component-alias-enrichment.md" "Aliases/tags proposed"
require_text "skills/wax-registry-discover/examples/component-alias-enrichment.md" "Explicit approval"
require_text "skills/wax-registry-discover/examples/component-alias-enrichment.md" 'wax search "navbar"'
require_text "skills/wax-registry-discover/examples/component-alias-enrichment.md" "ds.navigation-bar"
require_text "skills/wax-scan/SKILL.md" "unassessed"
require_text "skills/wax-scan/SKILL.md" "wax-registry-discover"
require_text "skills/wax-scan/SKILL.md" 'Inspect each unassessed row'
require_text "skills/wax-scan/SKILL.md" "unsupported_canonical_format"

TOKEN_GOLDEN="skills/wax-registry-discover/examples/token-value-refresh.md"
ALIAS_GOLDEN="skills/wax-registry-discover/examples/component-alias-enrichment.md"
EXPECTED_CANONICAL_VALUE="16.dp"
TOKEN_ID="space.medium"

extract_labeled_json() {
  local source="$1"
  local label="$2"
  local dest="$3"
  awk -v label="$label" '
    $0 ~ label { found = 1; next }
    found && /^```json$/ { in_block = 1; next }
    found && in_block && /^```$/ { exit }
    found && in_block { print }
  ' "$source" >"$dest"
  if [[ ! -s "$dest" ]]; then
    fail "could not extract JSON block after label: $label"
  fi
}

BEFORE_JSON="$(mktemp)"
AFTER_JSON="$(mktemp)"
trap 'rm -f "$BEFORE_JSON" "$AFTER_JSON"' EXIT

extract_labeled_json "$TOKEN_GOLDEN" "Before registry" "$BEFORE_JSON"
extract_labeled_json "$TOKEN_GOLDEN" "After registry" "$AFTER_JSON"

jq -e . "$BEFORE_JSON" >/dev/null || fail "before registry JSON does not parse"
jq -e . "$AFTER_JSON" >/dev/null || fail "after registry JSON does not parse"

jq -e --arg id "$TOKEN_ID" '
  .tokens
  | map(select(.id == $id))
  | length == 1
  and (.[0] | has("value") | not)
' "$BEFORE_JSON" >/dev/null \
  || fail "before registry token $TOKEN_ID must exist and must not have value"

jq -e --arg id "$TOKEN_ID" --arg expected "$EXPECTED_CANONICAL_VALUE" '
  .tokens
  | map(select(.id == $id))
  | length == 1
  and .[0].value == $expected
' "$AFTER_JSON" >/dev/null \
  || fail "after registry token $TOKEN_ID must have canonical value $EXPECTED_CANONICAL_VALUE"

jq -e --arg id "$TOKEN_ID" --slurpfile after "$AFTER_JSON" '
  (.tokens | map(select(.id == $id)) | .[0]) as $before
  | ($after[0].tokens | map(select(.id == $id)) | .[0]) as $after_tok
  | $before.id == $after_tok.id
  and $before.key == $after_tok.key
  and $before.category == $after_tok.category
  and (($before.aliases // []) == ($after_tok.aliases // []))
  and (($before.metadata // {}) == ($after_tok.metadata // {}))
' "$BEFORE_JSON" >/dev/null \
  || fail "ids, keys, categories, aliases, and metadata must be unchanged for $TOKEN_ID"

jq -e --arg id "$TOKEN_ID" --arg expected "$EXPECTED_CANONICAL_VALUE" \
  --slurpfile after "$AFTER_JSON" '
  (.tokens |= map(if .id == $id then . + {value: $expected} else . end))
  == $after[0]
' "$BEFORE_JSON" >/dev/null \
  || fail "after registry must equal before registry plus only the approved value for $TOKEN_ID"

BEFORE_TOKEN_COUNT="$(jq '.tokens | length' "$BEFORE_JSON")"
AFTER_TOKEN_COUNT="$(jq '.tokens | length' "$AFTER_JSON")"
BEFORE_COMPONENT_COUNT="$(jq '.components | length' "$BEFORE_JSON")"
AFTER_COMPONENT_COUNT="$(jq '.components | length' "$AFTER_JSON")"

if (( AFTER_TOKEN_COUNT < BEFORE_TOKEN_COUNT )); then
  fail "after token count ($AFTER_TOKEN_COUNT) is lower than before ($BEFORE_TOKEN_COUNT)"
fi
if (( AFTER_COMPONENT_COUNT < BEFORE_COMPONENT_COUNT )); then
  fail "after component count ($AFTER_COMPONENT_COUNT) is lower than before ($BEFORE_COMPONENT_COUNT)"
fi

BEFORE_ALIAS_JSON="$(mktemp)"
AFTER_ALIAS_JSON="$(mktemp)"
trap 'rm -f "$BEFORE_JSON" "$AFTER_JSON" "$BEFORE_ALIAS_JSON" "$AFTER_ALIAS_JSON"' EXIT

extract_labeled_json "$ALIAS_GOLDEN" "Before registry" "$BEFORE_ALIAS_JSON"
extract_labeled_json "$ALIAS_GOLDEN" "After registry" "$AFTER_ALIAS_JSON"

jq -e . "$BEFORE_ALIAS_JSON" >/dev/null || fail "before alias registry JSON does not parse"
jq -e . "$AFTER_ALIAS_JSON" >/dev/null || fail "after alias registry JSON does not parse"

jq -e '
  .components
  | map(select(.id == "ds.navigation-bar"))
  | length == 1
  and (.[0].aliases | index("navbar") | not)
' "$BEFORE_ALIAS_JSON" >/dev/null \
  || fail "before alias registry must not contain the proposed navbar synonym"

jq -e '
  .components
  | map(select(.id == "ds.navigation-bar"))
  | length == 1
  and (["navbar", "nav bar", "topbar", "app bar"] - .[0].aliases | length == 0)
  and (.[0].tags | index("navigation") != null)
' "$AFTER_ALIAS_JSON" >/dev/null \
  || fail "after alias registry must contain the approved aliases and navigation tag"

jq -e --slurpfile after "$AFTER_ALIAS_JSON" '
  (.components |= map(
    if .id == "ds.navigation-bar"
    then .aliases += ["navbar", "nav bar", "topbar", "app bar"]
      | .tags += ["navigation"]
    else .
    end
  )) == $after[0]
' "$BEFORE_ALIAS_JSON" >/dev/null \
  || fail "after alias registry must equal before registry plus only the approved aliases and tag"

echo "OK: wax-registry skill contract"
