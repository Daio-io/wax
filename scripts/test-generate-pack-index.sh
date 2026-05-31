#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

artifacts_dir="$tmp_dir/artifacts"
mkdir -p "$artifacts_dir/x86_64-unknown-linux-gnu" "$artifacts_dir/aarch64-apple-darwin"

cat > "$artifacts_dir/x86_64-unknown-linux-gnu/manifest.json" <<'JSON'
{
  "version": "0.1.0-alpha.1",
  "target": "x86_64-unknown-linux-gnu",
  "artifacts": {
    "wax": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    "wax-lang-compose": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-compose-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz",
      "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    },
    "wax-lang-basic": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-basic-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz",
      "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    },
    "wax-lang-react": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-react-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz",
      "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    }
  }
}
JSON

cat > "$artifacts_dir/aarch64-apple-darwin/manifest.json" <<'JSON'
{
  "version": "0.1.0-alpha.1",
  "target": "aarch64-apple-darwin",
  "artifacts": {
    "wax-lang-compose": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-compose-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz",
      "sha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    },
    "wax-lang-basic": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-basic-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz",
      "sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    }
  }
}
JSON

"$repo_root/scripts/generate-pack-index.sh" "$artifacts_dir" "$tmp_dir/index.json"

ruby -rjson -e '
  index = JSON.parse(File.read(ARGV.fetch(0)))
  abort("expected compose/basic only") unless index.map { |entry| entry.fetch("id") } == %w[compose basic]
  index.each do |entry|
    abort("wrong version") unless entry.fetch("version") == "0.1.0-alpha.1"
    abort("wrong api version") unless entry.fetch("api_version") == 1
    abort("wrong targets") unless entry.fetch("targets").keys == %w[aarch64-apple-darwin x86_64-unknown-linux-gnu]
  end
  compose_linux = index.fetch(0).fetch("targets").fetch("x86_64-unknown-linux-gnu")
  abort("compose linux url missing") unless compose_linux.fetch("url").include?("wax-lang-compose-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz")
  abort("compose linux sha missing") unless compose_linux.fetch("sha256") == "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  serialized = JSON.pretty_generate(index)
  abort("react must not be published in alpha index") if serialized.include?("react")
' "$tmp_dir/index.json"
