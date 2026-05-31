#!/usr/bin/env bash
set -euo pipefail

REPO="Daio-io/wax"
VERSION=""
INSTALL_DIR=""
DRY_RUN=0

usage() {
  cat <<'USAGE'
Usage: ./scripts/install.sh [options]

Install the wax CLI from a GitHub release archive.

Options:
  --version <semver>   Version to install (for example 0.1.0-alpha.1)
  --repo <owner/repo>  GitHub repository to download from (default: Daio-io/wax)
  --install-dir <path> Install destination directory
  --dry-run            Print planned actions without changing files
  -h, --help           Show this help
USAGE
}

log() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

run() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '[dry-run] %s\n' "$*"
  else
    "$@"
  fi
}

resolve_latest_tag() {
  local api_url="https://api.github.com/repos/${REPO}/releases"
  local response
  response="$(curl -fsSL "$api_url")" || die "failed to fetch releases from ${REPO}"
  local tag
  tag="$(printf '%s\n' "$response" | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [[ -n "$tag" ]] || die "could not determine latest release tag; pass --version explicitly"
  printf '%s\n' "$tag"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      shift
      [[ $# -gt 0 ]] || die "--version requires a value"
      VERSION="$1"
      ;;
    --repo)
      shift
      [[ $# -gt 0 ]] || die "--repo requires a value"
      REPO="$1"
      ;;
    --install-dir)
      shift
      [[ $# -gt 0 ]] || die "--install-dir requires a value"
      INSTALL_DIR="$1"
      ;;
    --dry-run)
      DRY_RUN=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
  shift
done

need_cmd uname
need_cmd tar
need_cmd curl

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin) os_part="apple-darwin" ;;
  Linux) os_part="unknown-linux-gnu" ;;
  *) die "unsupported operating system: $os" ;;
esac

case "$arch" in
  arm64|aarch64) arch_part="aarch64" ;;
  x86_64|amd64) arch_part="x86_64" ;;
  *) die "unsupported architecture: $arch" ;;
esac

target="${arch_part}-${os_part}"

if [[ -z "$INSTALL_DIR" ]]; then
  if [[ -w "/usr/local/bin" ]]; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="$HOME/.wax/bin"
  fi
fi

if [[ -z "$VERSION" ]]; then
  log "Resolving latest release tag from github.com/${REPO}"
  release_tag="$(resolve_latest_tag)"
else
  release_tag="v${VERSION#v}"
fi

version="${release_tag#v}"
archive_name="wax-${version}-${target}.tar.gz"
archive_url="https://github.com/${REPO}/releases/download/${release_tag}/${archive_name}"
sha_url="${archive_url}.sha256"

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

archive_path="$tmp_dir/$archive_name"
sha_path="$archive_path.sha256"

log "Installing wax ${version} for ${target}"
log "Download: ${archive_url}"

run curl -fL "$archive_url" -o "$archive_path"
run curl -fL "$sha_url" -o "$sha_path"

if command -v shasum >/dev/null 2>&1; then
  verify_cmd=(shasum -a 256 -c "$sha_path")
elif command -v sha256sum >/dev/null 2>&1; then
  verify_cmd=(sha256sum -c "$sha_path")
else
  die "need shasum or sha256sum to verify checksum"
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  printf '[dry-run] %s\n' "${verify_cmd[*]}"
else
  (cd "$tmp_dir" && "${verify_cmd[@]}")
fi

run mkdir -p "$INSTALL_DIR"

extract_dir="$tmp_dir/extract"
run mkdir -p "$extract_dir"
run tar -xzf "$archive_path" -C "$extract_dir"

if [[ "$DRY_RUN" -eq 1 ]]; then
  log "[dry-run] would install wax to $INSTALL_DIR/wax"
  log ""
  log "Next steps:"
  log "  wax init --non-interactive --language compose"
  log "  wax language install compose"
  log ""
  log "Verify with: wax --help"
  exit 0
fi

wax_bin="$(find "$extract_dir" -type f -name wax -print -quit)"
[[ -n "$wax_bin" ]] || die "could not find wax binary in archive"

run install -m 0755 "$wax_bin" "$INSTALL_DIR/wax"

log "Installed to $INSTALL_DIR/wax"
log ""
log "Next steps:"
log "  wax init --non-interactive --language compose"
log "  wax language install compose"
log ""
log "Verify with: wax --help"
