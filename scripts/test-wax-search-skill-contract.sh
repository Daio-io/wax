#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

SKILL="skills/wax-search/SKILL.md"
EXAMPLE="skills/wax-search/examples/search-and-use.md"

[[ -f "$SKILL" ]] || fail "$SKILL is missing"
grep -Fq -- 'name: wax-search' "$SKILL" || fail "$SKILL must declare the wax-search skill name"
grep -Eq -- '^argument-hint:.*design-system' "$SKILL" || fail "$SKILL must declare argument-hint for the design-system / registry key"
[[ -f "$EXAMPLE" ]] || fail "$EXAMPLE is missing"

require_text() {
  local file="$1"
  local text="$2"
  grep -Fq -- "$text" "$file" || fail "expected $file to contain: $text"
}

require_text "$SKILL" 'wax search'
require_text "$SKILL" 'wax component'
require_text "$SKILL" 'wax token'
require_text "$SKILL" 'wax registry list'
require_text "$SKILL" '--compact'
require_text "$SKILL" '`aliases` and `tags`'
require_text "$SKILL" 'command -v wax'
require_text "$SKILL" 'npx --yes @waxhq/wax@alpha'
require_text "$SKILL" 'Do not execute'
require_text "$SKILL" 'unavailable'
require_text "$EXAMPLE" 'npx --yes @waxhq/wax@alpha'
require_text "$EXAMPLE" 'Configured app'
require_text "$EXAMPLE" 'Direct URL'
require_text "$EXAMPLE" 'Git source'
require_text "$EXAMPLE" '--offline'
require_text "$EXAMPLE" 'wax registry refresh'
require_text "$EXAMPLE" '--snapshot "$snapshot_digest"'
require_text "$EXAMPLE" 'No matches'
require_text "$EXAMPLE" 'Ambiguous'
require_text "$EXAMPLE" 'Structured parameter and guidance facts'
require_text "$EXAMPLE" 'type_text'
require_text ".claude-plugin/plugin.json" "registry search"
require_text ".claude-plugin/marketplace.json" "registry search"
require_text "README.md" "skills/wax-search/SKILL.md"
require_text "packages/cli/README.md" "skills/wax-search/SKILL.md"
require_text ".github/workflows/build_engine.yml" 'skills/wax-search/**'
require_text ".github/workflows/build_engine.yml" 'skills/wax-registry-discover/**'
require_text ".github/workflows/build_engine.yml" 'scripts/test-wax-search-skill-contract.sh'
require_text ".github/workflows/build_engine.yml" 'scripts/test-wax-registry-skill-contract.sh'
require_text ".github/workflows/build_engine.yml" '.claude-plugin/**'
require_text ".github/workflows/build_engine.yml" 'Verify wax-search skill contract'
require_text "skills/wax-registry-discover/SKILL.md" 'explicit source association'
require_text "skills/wax-registry-discover/SKILL.md" 'lossless rediscovery'
require_text "skills/wax-registry-discover/SKILL.md" 'wax-search'
require_text "skills/wax-scan/SKILL.md" 'wax-search'
require_text "skills/wax-scan/SKILL.md" 'independent lookup workflow'
require_text "CHANGELOG.md" '`wax-search`'
require_text "docs/plans/README.md" 'Registry search and agent workflow'
require_text "docs/plans/README.md" '| `complete` |'
require_text "docs/plans/archive/README.md" '2026-10-22-registry-search-plan.md'
require_text "docs/adr/README.md" '2026-10-22-registry-search.md'

PLAN="docs/plans/archive/2026-10-22-registry-search-plan.md"
ADR="docs/adr/2026-10-22-registry-search.md"
[[ -f "$PLAN" ]] || fail "$PLAN is missing"
[[ -f "$ADR" ]] || fail "$ADR is missing"
if grep -Fq -- '- [ ]' "$PLAN"; then
  fail "$PLAN still contains incomplete checkboxes"
fi
require_text "$PLAN" 'scripts/test-wax-search-skill-contract.sh'
require_text "$PLAN" 'cargo test --workspace'
require_text "$ADR" 'Status:** Accepted'
require_text "$ADR" '`RegistrySearchResponse`'
require_text "$ADR" '`RegistryDetailResponse`'

if ! command -v jq >/dev/null 2>&1; then
  fail "jq is required"
fi

WAX_BIN="${WAX_BIN:-$ROOT/engine/target/debug/wax}"
if [[ ! -x "$WAX_BIN" ]]; then
  cargo build --quiet --manifest-path "$ROOT/engine/Cargo.toml" -p wax-cli
fi
[[ -x "$WAX_BIN" ]] || fail "wax binary is unavailable at $WAX_BIN"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
wax_home="$tmp_dir/wax-home"
repo="$tmp_dir/repo"
registry="$tmp_dir/registry.json"
mkdir -p "$wax_home" "$repo"
cp "$ROOT/engine/fixtures/registry-search/enriched-registry.json" "$registry"

run_wax() {
  WAX_HOME="$wax_home" "$WAX_BIN" "$@"
}

assert_search_envelope() {
  local file="$1"
  jq -e '
    .schema_version == 1
    and (.query | type == "string")
    and (.filters | type == "object")
    and (.snapshots | type == "array")
    and (.results | type == "array")
    and all(.snapshots[];
      (.source | type == "string")
      and (.language | type == "string")
      and (.digest | type == "string" and length == 64))
    and all(.results[];
      (.snapshot | type == "object")
      and (.kind == "component" or .kind == "token")
      and (.id | type == "string")
      and (.label | type == "string")
      and (.summary | type == "string"))
  ' "$file" >/dev/null || fail "$file is not a RegistrySearchResponse-shaped envelope"
}

assert_detail_envelope() {
  local file="$1"
  local kind="$2"
  local id="$3"
  local digest="$4"
  jq -e --arg kind "$kind" --arg id "$id" --arg digest "$digest" '
    .schema_version == 1
    and .kind == $kind
    and .entry.id == $id
    and .snapshot.digest == $digest
    and (.snapshot.source | type == "string")
    and (.snapshot.language | type == "string")
    and (.entry | type == "object")
  ' "$file" >/dev/null || fail "$file is not the expected RegistryDetailResponse-shaped envelope"
}

run_wax registry add acme "$registry" \
  --language compose \
  --name "Acme Design System" >/dev/null

component_compact="$tmp_dir/component-search.compact"
run_wax search "touch-first" \
  --design-system acme \
  --kind component \
  --compact \
  --repo-root "$repo" >"$component_compact"
grep -Eq '^results=1$' "$component_compact" \
  || fail "compact component search did not emit its result count"
grep -Eq $'^component\tid=ds\.button\tlabel=Button\tlanguage=compose\tdigest=[0-9a-f]{64}\tsource=.+$' "$component_compact" \
  || fail "compact component search omitted snapshot-actionable fields"

component_search="$tmp_dir/component-search.json"
run_wax search "touch-first" \
  --design-system acme \
  --kind component \
  --format json \
  --repo-root "$repo" >"$component_search"
assert_search_envelope "$component_search"
jq -e '.results | length == 1 and .[0].id == "ds.button"' "$component_search" >/dev/null \
  || fail "component example did not select ds.button"
jq -e '.results[0].summary == "A button used for important actions."' "$component_search" >/dev/null \
  || fail "component search did not use documentation from details as its summary"
jq -e '
  .results[0].aliases == ["Btn"]
  and .results[0].tags == ["action", "form"]
' "$component_search" >/dev/null \
  || fail "component search did not expose aliases and tags"

enum_search="$tmp_dir/enum-search.json"
run_wax search "Comfortable" \
  --design-system acme \
  --kind component \
  --format json \
  --repo-root "$repo" >"$enum_search"
assert_search_envelope "$enum_search"
jq -e '.results | length == 1 and .[0].id == "ds.button"' "$enum_search" >/dev/null \
  || fail "enum value search did not select ds.button"
component_digest="$(jq -er '.results[0].snapshot.digest' "$component_search")"

token_search="$tmp_dir/token-search.json"
run_wax search "#0B5FFF" \
  --design-system acme \
  --kind token \
  --format json \
  --repo-root "$repo" >"$token_search"
assert_search_envelope "$token_search"
jq -e '.results | length == 1 and .[0].id == "color.primary"' "$token_search" >/dev/null \
  || fail "token example did not select color.primary"
token_digest="$(jq -er '.results[0].snapshot.digest' "$token_search")"

# Change the source and refresh the current selection. Exact detail must still
# resolve the immutable digests returned by the earlier searches.
jq '
  .components[0].symbol = "ButtonV2"
  | .tokens[0].value = "#00AAFF"
' "$registry" >"$tmp_dir/refreshed-registry.json"
mv "$tmp_dir/refreshed-registry.json" "$registry"
run_wax registry refresh acme >/dev/null

component_detail="$tmp_dir/component-detail.json"
run_wax component ds.button \
  --design-system acme \
  --snapshot "$component_digest" \
  --format json \
  --repo-root "$repo" >"$component_detail"
assert_detail_envelope "$component_detail" component ds.button "$component_digest"
jq -e '.entry.symbol == "Button" and .entry.package == "com.acme.ui"' "$component_detail" >/dev/null \
  || fail "snapshot-pinned component detail changed after refresh"
jq -e '
  .entry.details.signature.parameters[0].type_kind == "enum"
  and (.entry.details.signature.parameters[0].allowed_values | length == 2)
' "$component_detail" >/dev/null \
  || fail "snapshot-pinned detail omitted structured enum values"
jq -e '.entry.details.best_practices | length == 2' "$component_detail" >/dev/null \
  || fail "snapshot-pinned detail omitted structured guidance"

token_detail="$tmp_dir/token-detail.json"
run_wax token color.primary \
  --design-system acme \
  --snapshot "$token_digest" \
  --format json \
  --repo-root "$repo" >"$token_detail"
assert_detail_envelope "$token_detail" token color.primary "$token_digest"
jq -e '.entry.key == "Theme.colors.primary" and .entry.value == "#0B5FFF"' "$token_detail" >/dev/null \
  || fail "snapshot-pinned token detail changed after refresh"

offline_search="$tmp_dir/offline-search.json"
run_wax search "ButtonV2" \
  --design-system acme \
  --offline \
  --format json \
  --repo-root "$repo" >"$offline_search"
assert_search_envelope "$offline_search"
jq -e '.results[0].id == "ds.button"' "$offline_search" >/dev/null \
  || fail "offline example did not use the refreshed cached snapshot"

no_matches="$tmp_dir/no-matches.json"
run_wax search "unrelated-query-with-no-match" \
  --design-system acme \
  --format json \
  --repo-root "$repo" >"$no_matches"
assert_search_envelope "$no_matches"
jq -e '.results == []' "$no_matches" >/dev/null \
  || fail "no-match example must succeed with an empty results array"

# Exercise the documented ambiguity path with the same entry id in two
# configured language registries.
ambiguous_registry="$tmp_dir/ambiguous-registry.json"
jq '{schema_version, components: [{id: "shared", symbol: "Shared"}], tokens: []}' \
  "$registry" >"$ambiguous_registry"
ambiguous_repo="$tmp_dir/ambiguous-repo"
mkdir -p "$ambiguous_repo/.wax" "$ambiguous_repo/src"
cp "$ambiguous_registry" "$ambiguous_repo/first.json"
cp "$ambiguous_registry" "$ambiguous_repo/second.json"
if command -v sha256sum >/dev/null 2>&1; then
  ambiguous_first_digest="$(sha256sum "$ambiguous_repo/first.json" | awk '{print $1}')"
  ambiguous_second_digest="$(sha256sum "$ambiguous_repo/second.json" | awk '{print $1}')"
else
  ambiguous_first_digest="$(shasum -a 256 "$ambiguous_repo/first.json" | awk '{print $1}')"
  ambiguous_second_digest="$(shasum -a 256 "$ambiguous_repo/second.json" | awk '{print $1}')"
fi
cat >"$ambiguous_repo/.wax/wax.config.json" <<'JSON'
{
  "schema_version": 2,
  "languages": {
    "react": {"roots": ["src"], "registry": {"source": "first.json"}},
    "compose": {"roots": ["src"], "registry": {"source": "second.json"}}
  }
}
JSON
cat >"$ambiguous_repo/.wax/wax.lock.json" <<JSON
{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {
    "react": {"source": "first.json", "sha256": "$ambiguous_first_digest"},
    "compose": {"source": "second.json", "sha256": "$ambiguous_second_digest"}
  },
  "languages": {}
}
JSON
ambiguous_error="$tmp_dir/ambiguous-error.json"
if WAX_HOME="$wax_home" "$WAX_BIN" component shared --format json \
  --repo-root "$ambiguous_repo" >"$ambiguous_error" 2>&1; then
  fail "ambiguous component lookup must fail"
fi
jq -e '.code == "ambiguous_entry"' "$ambiguous_error" >/dev/null \
  || fail "ambiguous component lookup must return ambiguous_entry"

# Exercise the documented unavailable-facts path against a real detail
# response whose optional details object is absent.
unavailable_registry="$tmp_dir/unavailable-registry.json"
jq '{schema_version, languages, components: [{id: "ds.sparse", symbol: "Sparse"}], tokens: []}' \
  "$registry" >"$unavailable_registry"
run_wax registry add sparse "$unavailable_registry" \
  --language compose \
  --name "Sparse Design System" >/dev/null
unavailable_detail="$tmp_dir/unavailable-detail.json"
run_wax component ds.sparse \
  --design-system sparse \
  --format json \
  --repo-root "$repo" >"$unavailable_detail"
assert_detail_envelope "$unavailable_detail" component ds.sparse \
  "$(jq -er '.snapshot.digest' "$unavailable_detail")"
jq -e '(.entry | has("details") | not)' "$unavailable_detail" >/dev/null \
  || fail "unavailable detail must omit the absent details object"

# Exercise the documented configured-app form without a design-system selector.
configured_repo="$tmp_dir/configured-app"
configured_home="$tmp_dir/configured-home"
mkdir -p "$configured_repo/.wax" "$configured_repo/src" "$configured_home"
cp "$ROOT/engine/fixtures/registry-search/enriched-registry.json" \
  "$configured_repo/.wax/compose.registry.json"
if command -v sha256sum >/dev/null 2>&1; then
  configured_digest="$(sha256sum "$configured_repo/.wax/compose.registry.json" | awk '{print $1}')"
else
  configured_digest="$(shasum -a 256 "$configured_repo/.wax/compose.registry.json" | awk '{print $1}')"
fi
cat >"$configured_repo/.wax/wax.config.json" <<'JSON'
{
  "schema_version": 2,
  "languages": {
    "compose": {
      "roots": ["src"],
      "registry": {"source": ".wax/compose.registry.json"}
    }
  }
}
JSON
cat >"$configured_repo/.wax/wax.lock.json" <<JSON
{
  "schema_version": 2,
  "engine_api_version": 1,
  "wax_version": "0.1.0",
  "registries": {
    "compose": {
      "source": ".wax/compose.registry.json",
      "sha256": "$configured_digest"
    }
  },
  "languages": {}
}
JSON

configured_search="$tmp_dir/configured-search.json"
WAX_HOME="$configured_home" "$WAX_BIN" search "primary action" \
  --kind component \
  --format json \
  --repo-root "$configured_repo" >"$configured_search"
assert_search_envelope "$configured_search"
jq -e '.results | length == 1 and .[0].id == "ds.button"' "$configured_search" >/dev/null \
  || fail "configured-app example did not select its committed registry"
configured_snapshot="$(jq -er '.results[0].snapshot.digest' "$configured_search")"
configured_detail="$tmp_dir/configured-detail.json"
WAX_HOME="$configured_home" "$WAX_BIN" component ds.button \
  --snapshot "$configured_snapshot" \
  --format json \
  --repo-root "$configured_repo" >"$configured_detail"
assert_detail_envelope "$configured_detail" component ds.button "$configured_snapshot"

echo "OK: wax-search skill contract"
