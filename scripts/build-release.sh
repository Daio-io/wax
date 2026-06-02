#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./scripts/build-release.sh [target] [--include-contributor]

Build and package wax release binaries for one target triple.
If no target is provided, uses the host triple.
By default this builds alpha artifacts only:
  wax, wax-lang-compose, wax-lang-basic
Pass --include-contributor to also build contributor-only artifacts.
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$repo_root/engine"
out_dir="${WAX_RELEASE_OUT_DIR:-$repo_root/release/artifacts}"
requested_target=""
include_contributor=0

for arg in "$@"; do
  case "$arg" in
    --help|-h)
      usage
      exit 0
      ;;
    --include-contributor)
      include_contributor=1
      ;;
    *)
      if [[ -z "$requested_target" ]]; then
        requested_target="$arg"
      else
        echo "error: unexpected argument '$arg'" >&2
        usage >&2
        exit 1
      fi
      ;;
  esac
done

read_release_array() {
  local key="$1"
  awk -v key="$key" '
    /^\[workspace\.metadata\.release\]/ { in_section=1; next }
    /^\[/ && in_section { in_section=0 }
    in_section && $0 ~ ("^" key " = \\[") {
      line=$0
      while (line !~ /\]/) {
        if (getline nextline <= 0) break
        line=line " " nextline
      }
      gsub(/.*\[/, "", line)
      gsub(/\].*/, "", line)
      gsub(/"/, "", line)
      gsub(/,/, " ", line)
      gsub(/[[:space:]]+/, " ", line)
      sub(/^ /, "", line)
      sub(/ $/, "", line)
      print line
      exit
    }
  ' "$engine_dir/Cargo.toml"
}

host_target="$(rustc -vV | awk '/host:/ { print $2 }')"
target="${requested_target:-$host_target}"
supported_targets="$(read_release_array "artifact_targets")"

is_supported=0
for candidate in $supported_targets; do
  if [[ "$candidate" == "$target" ]]; then
    is_supported=1
    break
  fi
done
if [[ "$is_supported" -ne 1 ]]; then
  echo "error: unsupported target '$target'" >&2
  echo "supported: $supported_targets" >&2
  exit 1
fi

workspace_version="$(awk -F '\"' '/^\[workspace.package\]/{flag=1;next} flag && /^version =/{print $2; exit}' "$engine_dir/Cargo.toml")"
version="${WAX_RELEASE_VERSION:-}"
if [[ -z "$version" && -n "${WAX_RELEASE_TAG:-}" ]]; then
  version="${WAX_RELEASE_TAG#v}"
fi
version="${version:-$workspace_version}"
version="${version#v}"
if [[ -z "$version" || "$version" == *"/"* ]]; then
  echo "error: invalid release version '$version'" >&2
  exit 1
fi
alpha_index_binaries="$(read_release_array "alpha_index_binaries")"
contributor_only_binaries="$(read_release_array "contributor_only_binaries")"
binaries=($alpha_index_binaries)
if [[ "$include_contributor" -eq 1 ]]; then
  binaries+=($contributor_only_binaries)
fi
packages=()
for binary in "${binaries[@]}"; do
  if [[ "$binary" == "wax" ]]; then
    packages+=("wax-cli")
  else
    packages+=("$binary")
  fi
done

if ! rustup target list --installed | grep -qx "$target"; then
  echo "error: target '$target' is not installed via rustup" >&2
  echo "install with: rustup target add $target" >&2
  exit 1
fi

mkdir -p "$out_dir"
rm -rf "$out_dir/$target"
mkdir -p "$out_dir/$target"

manifest="$out_dir/$target/manifest.json"
{
  printf '{\n'
  printf '  "version": "%s",\n' "$version"
  printf '  "target": "%s",\n' "$target"
  printf '  "artifacts": {\n'
} > "$manifest"

for i in "${!packages[@]}"; do
  package="${packages[$i]}"
  binary="${binaries[$i]}"

  (cd "$engine_dir" && WAX_BUILD_VERSION="$version" cargo build --release --package "$package" --target "$target")

  stage_dir="$out_dir/$target/${binary}-${version}-${target}"
  mkdir -p "$stage_dir"
  cp "$engine_dir/target/$target/release/$binary" "$stage_dir/$binary"

  archive="$out_dir/$target/${binary}-${version}-${target}.tar.gz"
  if [[ "$binary" == "wax" ]]; then
    tar -C "$out_dir/$target" -czf "$archive" "$(basename "$stage_dir")"
  else
    tar -C "$stage_dir" -czf "$archive" "$binary"
  fi
  (cd "$(dirname "$archive")" && shasum -a 256 "$(basename "$archive")") > "$archive.sha256"
  sha256="$(awk '{ print $1 }' "$archive.sha256")"
  comma=","
  if [[ "$i" -eq "$((${#packages[@]} - 1))" ]]; then
    comma=""
  fi
  printf '    "%s": {"url": "https://github.com/Daio-io/wax/releases/download/v%s/%s", "sha256": "%s"}%s\n' \
    "$binary" "$version" "$(basename "$archive")" "$sha256" "$comma" >> "$manifest"
done

{
  printf '  }\n'
  printf '}\n'
} >> "$manifest"

echo "built archives:"
ls -1 "$out_dir/$target"/*.tar.gz
echo "wrote manifest:"
echo "$manifest"
