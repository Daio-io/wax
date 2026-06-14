#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=html-escape.sh
source "$SCRIPT_DIR/html-escape.sh"

payload='<script>alert(1)</script><img onerror=evil src=x>'
escaped="$(printf '%s' "$payload" | html_escape_stdin)"

if [[ "$escaped" == *"<script>"* || "$escaped" == *"<img"* ]]; then
  echo "FAIL: html_escape left raw HTML tags in: $escaped" >&2
  exit 1
fi

if [[ "$escaped" != *"&lt;script&gt;"* ]]; then
  echo "FAIL: expected escaped script tags in: $escaped" >&2
  exit 1
fi

echo "PASS: html_escape neutralizes HTML/script injection in scan-derived text"
