#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

SKILL_DIR="skills/wax-refine-docs"
SKILL="$SKILL_DIR/SKILL.md"

[[ -f "$SKILL" ]] || fail "$SKILL is missing"
grep -Fq -- 'name: wax-refine-docs' "$SKILL" || fail "$SKILL must declare the wax-refine-docs skill name"
grep -Eq -- '^argument-hint:.*compose\|swift\|react' "$SKILL" || fail "$SKILL must declare an argument-hint for the language"

require_text() {
  local file="$1"
  local text="$2"
  grep -Fq -- "$text" "$file" || fail "expected $file to contain: $text"
}

require_text "$SKILL" 'wax registry discover'
require_text "$SKILL" '--dry-run'
require_text "$SKILL" 'docs-coverage.sh'
require_text "$SKILL" 'command -v wax'
require_text "$SKILL" 'npx --yes @waxhq/wax@alpha'
require_text "$SKILL" 'explicit, separate approval'
require_text "$SKILL" 'invent behavior'
require_text "$SKILL" 'wax-registry-discover'
require_text "$SKILL" 'wax-search'
require_text "$SKILL" '240'

for language in compose swift react; do
  reference="$SKILL_DIR/reference/$language.md"
  [[ -f "$reference" ]] || fail "$reference is missing"
  example_dir="$SKILL_DIR/examples/$language"
  source_file="$(find "$example_dir" -name 'AcmeButton.*' ! -name '*.expected.json' | head -n 1)"
  [[ -n "$source_file" ]] || fail "$example_dir needs an AcmeButton source example"
  expected="$example_dir/AcmeButton.expected.json"
  [[ -f "$expected" ]] || fail "$expected is missing"
  require_text "$reference" "examples/$language/$(basename "$source_file")"
  require_text "$reference" "AcmeButton.expected.json"
  require_text "$SKILL" "reference/$language.md"
  test_file="engine/crates/wax-lang-$language/tests/refine_docs_skill_example.rs"
  [[ -f "$test_file" ]] || fail "$test_file must keep the $language example aligned"
  require_text "$test_file" "skills/wax-refine-docs/examples/$language"

  # Golden examples must demonstrate every fact the skill teaches.
  jq -e '
    .status == "complete"
    and (.documentation | type == "string" and length > 0)
    and (.signature.parameters | length > 0)
    and all(.signature.parameters[]; (.description // "") != "")
    and any(.signature.parameters[]; .type_kind == "enum" and (.allowed_values | length) > 1)
    and any(.signature.parameters[]; .default != null)
    and (.best_practices | length) >= 1
    and any(.attachments[]; .kind == "example")
    and ((.field_gaps // []) | length) == 0
  ' "$expected" >/dev/null || fail "$expected must show complete extraction of every taught fact"
done

require_text "$SKILL_DIR/reference/compose.md" '@guidance'
require_text "$SKILL_DIR/reference/swift.md" 'public init'
require_text "$SKILL_DIR/reference/swift.md" '- Guidance:'
require_text "$SKILL_DIR/reference/react.md" '@example'
require_text "$SKILL_DIR/reference/react.md" 'extends'

require_text ".claude-plugin/plugin.json" "source doc refinement"
require_text ".claude-plugin/marketplace.json" "source doc refinement"
require_text "README.md" "skills/wax-refine-docs/SKILL.md"
require_text "CHANGELOG.md" '`wax-refine-docs`'
require_text "website/docs/skills/index.md" "wax-refine-docs"
require_text ".github/workflows/build_engine.yml" 'skills/wax-refine-docs/**'
require_text ".github/workflows/build_engine.yml" 'scripts/test-wax-refine-docs-skill-contract.sh'
require_text ".github/workflows/build_engine.yml" 'Verify wax-refine-docs skill contract'

"$ROOT/$SKILL_DIR/scripts/test-docs-coverage.sh"

echo "OK: wax-refine-docs skill contract"
