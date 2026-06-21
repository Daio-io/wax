#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="$(mktemp -d)"
trap 'rm -rf "$INSTALL_DIR"' EXIT

output="$(
  "$ROOT_DIR/scripts/install.sh" \
    --dry-run \
    --version 0.1.0-alpha.1 \
    --install-dir "$INSTALL_DIR" \
    2>&1
)"

printf '%s\n' "$output" | grep -F "[dry-run] would install wax to $INSTALL_DIR/wax" >/dev/null
printf '%s\n' "$output" | grep -F "[dry-run] $INSTALL_DIR/wax language update --all" >/dev/null
printf '%s\n' "$output" | grep -F "Verify with: $INSTALL_DIR/wax --help" >/dev/null

