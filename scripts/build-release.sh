#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$repo_root/engine"
out_dir="${WAX_RELEASE_OUT_DIR:-$repo_root/release/artifacts}"
requested_target="${1:-}"

host_target="$(rustc -vV | awk '/host:/ { print $2 }')"
target="${requested_target:-$host_target}"

case "$target" in
  aarch64-apple-darwin|x86_64-apple-darwin|x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu)
    ;;
  *)
    echo "error: unsupported target '$target'" >&2
    echo "supported: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu" >&2
    exit 1
    ;;
esac

packages=(wax-cli wax-lang-compose wax-lang-basic wax-lang-react)
binaries=(wax wax-lang-compose wax-lang-basic wax-lang-react)
version="$(awk -F '\"' '/^\[workspace.package\]/{flag=1;next} flag && /^version =/{print $2; exit}' "$engine_dir/Cargo.toml")"

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
  shasum -a 256 "$archive" > "$archive.sha256"
done

echo "built archives:"
ls -1 "$out_dir/$target"/*.tar.gz
