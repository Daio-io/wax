#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="$repo_root/scripts/verify-compose-kotlin-fixtures.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

assert_eq() {
  local actual="$1" expected="$2" label="$3"
  if [[ "$actual" != "$expected" ]]; then
    printf '%s: expected %q, got %q\n' "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_contains() {
  printf '%s\n' "$1" | grep -F "$2" >/dev/null || {
    printf 'missing %q in:\n%s\n' "$2" "$1" >&2
    exit 1
  }
}

fake_compiler="$tmp/kotlinc with spaces"
cat > "$fake_compiler" <<'INNER'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\0' "$@" >> "${FAKE_LOG:?}"
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  if [[ "${args[$i]}" == "-d" && $((i + 1)) -lt ${#args[@]} ]]; then
    printf '%s\n' "$(dirname "${args[$((i + 1))]}")" > "${FAKE_TEMP_DIR_LOG:?}"
  fi
done
if [[ "${FAKE_FAIL:-0}" == "1" ]]; then
  exit 7
fi
exit 0
INNER
chmod +x "$fake_compiler"

set +e
out="$("$script" --bogus 2>&1)"
status=$?
set -e
assert_eq "$status" "2" "unknown arg exit"
assert_contains "$out" "unknown argument"

set +e
out="$("$script" --version 2.4.0 --compiler relative/kotlinc 2>&1)"
status=$?
set -e
assert_eq "$status" "2" "relative compiler exit"
assert_contains "$out" "must be absolute"

set +e
out="$("$script" --version 2.4.0 --compiler "$tmp/missing" 2>&1)"
status=$?
set -e
assert_eq "$status" "2" "missing compiler exit"

set +e
out="$("$script" --version 9.9.9 --compiler "$fake_compiler" 2>&1)"
status=$?
set -e
assert_eq "$status" "2" "no matrix rows exit"
assert_contains "$out" "no matrix rows"

export FAKE_LOG="$tmp/args.log"
export FAKE_TEMP_DIR_LOG="$tmp/temp-dir.log"
: > "$FAKE_LOG"
: > "$FAKE_TEMP_DIR_LOG"
"$script" --version 2.1.0 --compiler "$fake_compiler"
grep -Fz "WhenGuard.kt" "$FAKE_LOG" >/dev/null
grep -Fz -- "-Xwhen-guards" "$FAKE_LOG" >/dev/null
grep -Fz "WhenGuard.jar" "$FAKE_LOG" >/dev/null
validator_temp="$(cat "$FAKE_TEMP_DIR_LOG")"
[[ -n "$validator_temp" ]] || { printf 'validator temp dir was not recorded\n' >&2; exit 1; }
[[ ! -e "$validator_temp" ]] || { printf 'validator temp dir was not cleaned: %s\n' "$validator_temp" >&2; exit 1; }

: > "$FAKE_LOG"
"$script" --version 2.4.0 --compiler "$fake_compiler"
d_count="$(tr '\0' '\n' < "$FAKE_LOG" | grep -c '^-d$' || true)"
assert_eq "$d_count" "10" "2.4.0 fixture count"
if tr '\0' '\n' < "$FAKE_LOG" | grep -F -- "-Xwhen-guards" >/dev/null; then
  printf 'unexpected flag for 2.4.0\n' >&2
  exit 1
fi

export FAKE_FAIL=1
set +e
"$script" --version 2.1.0 --compiler "$fake_compiler"
status=$?
set -e
assert_eq "$status" "7" "compiler failure exit"

printf 'ok: test-verify-compose-kotlin-fixtures\n'
