#!/usr/bin/env bash
# Escape scan-derived text before inserting into HTML or SVG text nodes.
# Usage:
#   html_escape "untrusted string"
#   printf '%s' "$value" | html_escape_stdin
set -euo pipefail

html_escape() {
  python3 -c 'import html, sys; print(html.escape(sys.argv[1], quote=False), end="")' "$1"
}

html_escape_stdin() {
  python3 -c 'import html, sys; print(html.escape(sys.stdin.read(), quote=False), end="")'
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  if [[ $# -gt 0 ]]; then
    html_escape "$1"
  else
    html_escape_stdin
  fi
fi
