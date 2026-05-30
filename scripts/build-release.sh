#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./scripts/build-release.sh [target]

Build and package wax release binaries for one target triple.
If no target is provided, uses the host triple.
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$repo_root/engine"
out_dir="${WAX_RELEASE_OUT_DIR:-$repo_root/release/artifacts}"
requested_target="${1:-}"

if [[ "${requested_target:-}" == "--help" || "${requested_target:-}" == "-h" ]]; then
  usage
  exit 0
fi

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

version="$(awk -F '\"' '/^\[workspace.package\]/{flag=1;next} flag && /^version =/{print $2; exit}' "$engine_dir/Cargo.toml")"
alpha_index_binaries="$(read_release_array "alpha_index_binaries")"
contributor_only_binaries="$(read_release_array "contributor_only_binaries")"
binaries=($alpha_index_binaries $contributor_only_binaries)
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

for i in "${!packages[@]}"; do
  package="${packages[$i]}"
  binary="${binaries[$i]}"

  (cd "$engine_dir" && cargo build --release --package "$package" --target "$target")

  stage_dir="$out_dir/$target/${binary}-${version}-${target}"
  mkdir -p "$stage_dir"
  cp "$engine_dir/target/$target/release/$binary" "$stage_dir/$binary"

  archive="$out_dir/$target/${binary}-${version}-${target}.tar.gz"
  tar -C "$out_dir/$target" -czf "$archive" "$(basename "$stage_dir")"
  (cd "$(dirname "$archive")" && shasum -a 256 "$(basename "$archive")") > "$archive.sha256"
done

echo "built archives:"
ls -1 "$out_dir/$target"/*.tar.gz
