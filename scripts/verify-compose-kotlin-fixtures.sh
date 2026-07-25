#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'usage: verify-compose-kotlin-fixtures.sh --version <2.x.y> --compiler </absolute/path/to/kotlinc>\n' >&2
  exit 2
}

version=""
compiler=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || usage
      version="$2"
      shift 2
      ;;
    --compiler)
      [[ $# -ge 2 ]] || usage
      compiler="$2"
      shift 2
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage
      ;;
  esac
done

[[ -n "$version" && -n "$compiler" ]] || usage
[[ "$compiler" == /* ]] || {
  printf 'compiler path must be absolute: %s\n' "$compiler" >&2
  exit 2
}
[[ -x "$compiler" ]] || {
  printf 'compiler is missing or not executable: %s\n' "$compiler" >&2
  exit 2
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
matrix="$repo_root/engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax/compiler-matrix.tsv"
fixture_root="$repo_root/engine/crates/wax-lang-compose/tests/fixtures/kotlin-syntax"
[[ -f "$matrix" ]] || {
  printf 'missing compiler matrix: %s\n' "$matrix" >&2
  exit 2
}

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

matched=0
while IFS=$'\t' read -r matrix_version flags_field fixture_rel; do
  [[ -n "${matrix_version:-}" ]] || continue
  [[ "$matrix_version" == "$version" ]] || continue
  matched=$((matched + 1))

  fixture="$fixture_root/$fixture_rel"
  fixture_name="$(basename "$fixture" .kt)"
  flags=()
  if [[ "$flags_field" != "-" ]]; then
    # shellcheck disable=SC2206
    read -r -a flags <<< "$flags_field"
  fi

  "$compiler" "$fixture" -d "$temp_dir/${fixture_name}.jar" ${flags[@]+"${flags[@]}"}
done < "$matrix"

[[ "$matched" -gt 0 ]] || {
  printf 'no matrix rows for version %s\n' "$version" >&2
  exit 2
}
