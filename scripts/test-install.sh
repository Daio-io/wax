#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

assert_contains() {
  local haystack="$1"
  local needle="$2"
  printf '%s\n' "$haystack" | grep -F "$needle" >/dev/null
}

host_target() {
  local os arch os_part arch_part
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin) os_part="apple-darwin" ;;
    Linux) os_part="unknown-linux-gnu" ;;
    *)
      printf 'unsupported test host os: %s\n' "$os" >&2
      exit 1
      ;;
  esac

  case "$arch" in
    arm64|aarch64) arch_part="aarch64" ;;
    x86_64|amd64) arch_part="x86_64" ;;
    *)
      printf 'unsupported test host arch: %s\n' "$arch" >&2
      exit 1
      ;;
  esac

  printf '%s-%s\n' "$arch_part" "$os_part"
}

INSTALL_DIR="$TEST_TMP/dry-run-install"
mkdir -p "$INSTALL_DIR"

dry_run_output="$(
  "$ROOT_DIR/scripts/install.sh" \
    --dry-run \
    --version 0.1.0-alpha.1 \
    --install-dir "$INSTALL_DIR" \
    2>&1
)"

assert_contains "$dry_run_output" "[dry-run] would install wax to $INSTALL_DIR/wax"
assert_contains "$dry_run_output" "language update --all --repo-root"
assert_contains "$dry_run_output" "Verify with: $INSTALL_DIR/wax --help"

version="0.1.0-alpha.1"
target="$(host_target)"
release_tag="v${version}"
repo="wax-fixture/test"
archive_name="wax-${version}-${target}.tar.gz"
release_dir="$TEST_TMP/release"
archive_root="$release_dir/wax-${version}-${target}"
install_root="$TEST_TMP/install-root"
curl_bin_dir="$TEST_TMP/bin"
refresh_pwd_log="$TEST_TMP/refresh-pwd.log"
refresh_args_log="$TEST_TMP/refresh-args.log"

mkdir -p "$archive_root" "$install_root" "$curl_bin_dir"

cat > "$archive_root/wax" <<EOF
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "\$PWD" > "$refresh_pwd_log"
printf '%s\n' "\$*" > "$refresh_args_log"
if [[ "\${1-}" == "language" && "\${2-}" == "update" && "\${3-}" == "--all" ]]; then
  printf 'simulated refresh failure\n' >&2
  exit 17
fi
printf 'unexpected args: %s\n' "\$*" >&2
exit 19
EOF
chmod +x "$archive_root/wax"

tar -C "$release_dir" -czf "$release_dir/$archive_name" "$(basename "$archive_root")"
(cd "$release_dir" && shasum -a 256 "$archive_name") > "$release_dir/$archive_name.sha256"

cat > "$curl_bin_dir/curl" <<EOF
#!/usr/bin/env bash
set -euo pipefail
output=""
url=""
while [[ \$# -gt 0 ]]; do
  case "\$1" in
    -o)
      shift
      output="\$1"
      ;;
    http://*|https://*|file:*)
      url="\$1"
      ;;
  esac
  shift
done

case "\$url" in
  "https://github.com/$repo/releases/download/$release_tag/$archive_name")
    cp "$release_dir/$archive_name" "\$output"
    ;;
  "https://github.com/$repo/releases/download/$release_tag/$archive_name.sha256")
    cp "$release_dir/$archive_name.sha256" "\$output"
    ;;
  *)
    printf 'unexpected curl url: %s\n' "\$url" >&2
    exit 1
    ;;
esac
EOF
chmod +x "$curl_bin_dir/curl"

full_output="$(
  cd "$ROOT_DIR" &&
  PATH="$curl_bin_dir:$PATH" \
    "$ROOT_DIR/scripts/install.sh" \
      --version "$version" \
      --repo "$repo" \
      --install-dir "$install_root" \
      2>&1
)"

test -x "$install_root/wax"
assert_contains "$full_output" "Warning: unable to refresh installed wax language packs after install."
assert_contains "$full_output" "simulated refresh failure"
assert_contains "$(cat "$refresh_args_log")" "language update --all --repo-root"
if [[ "$(cat "$refresh_pwd_log")" == "$ROOT_DIR" ]]; then
  printf 'refresh ran from caller repo root, expected neutral directory\n' >&2
  exit 1
fi
