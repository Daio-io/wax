#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

artifacts_dir="$tmp_dir/artifacts"
mkdir -p \
  "$artifacts_dir/aarch64-apple-darwin" \
  "$artifacts_dir/aarch64-unknown-linux-gnu" \
  "$artifacts_dir/x86_64-apple-darwin" \
  "$artifacts_dir/x86_64-unknown-linux-gnu"

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
    },
    "wax-lang-react": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-react-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz",
      "sha256": "1111111111111111111111111111111111111111111111111111111111111111"
    }
  }
}
JSON

cat > "$artifacts_dir/x86_64-apple-darwin/manifest.json" <<'JSON'
{
  "version": "0.1.0-alpha.1",
  "target": "x86_64-apple-darwin",
  "artifacts": {
    "wax-lang-compose": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-compose-0.1.0-alpha.1-x86_64-apple-darwin.tar.gz",
      "sha256": "2222222222222222222222222222222222222222222222222222222222222222"
    },
    "wax-lang-basic": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-basic-0.1.0-alpha.1-x86_64-apple-darwin.tar.gz",
      "sha256": "3333333333333333333333333333333333333333333333333333333333333333"
    },
    "wax-lang-react": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-react-0.1.0-alpha.1-x86_64-apple-darwin.tar.gz",
      "sha256": "4444444444444444444444444444444444444444444444444444444444444444"
    }
  }
}
JSON

cat > "$artifacts_dir/aarch64-unknown-linux-gnu/manifest.json" <<'JSON'
{
  "version": "0.1.0-alpha.1",
  "target": "aarch64-unknown-linux-gnu",
  "artifacts": {
    "wax-lang-compose": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-compose-0.1.0-alpha.1-aarch64-unknown-linux-gnu.tar.gz",
      "sha256": "5555555555555555555555555555555555555555555555555555555555555555"
    },
    "wax-lang-basic": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-basic-0.1.0-alpha.1-aarch64-unknown-linux-gnu.tar.gz",
      "sha256": "6666666666666666666666666666666666666666666666666666666666666666"
    },
    "wax-lang-react": {
      "url": "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-lang-react-0.1.0-alpha.1-aarch64-unknown-linux-gnu.tar.gz",
      "sha256": "7777777777777777777777777777777777777777777777777777777777777777"
    }
  }
}
JSON

"$repo_root/scripts/generate-pack-index.sh" "$artifacts_dir" "$tmp_dir/index.json"

ruby -rjson -e '
  index = JSON.parse(File.read(ARGV.fetch(0)))
  abort("expected compose/basic/react") unless index.map { |entry| entry.fetch("id") } == %w[compose basic react]
  index.each do |entry|
    abort("wrong version") unless entry.fetch("version") == "0.1.0-alpha.1"
    abort("wrong api version") unless entry.fetch("api_version") == 1
    expected_targets = %w[
      aarch64-apple-darwin
      aarch64-unknown-linux-gnu
      x86_64-apple-darwin
      x86_64-unknown-linux-gnu
    ]
    abort("wrong targets") unless entry.fetch("targets").keys == expected_targets
  end
  compose_linux = index.fetch(0).fetch("targets").fetch("x86_64-unknown-linux-gnu")
  abort("compose linux url missing") unless compose_linux.fetch("url").include?("wax-lang-compose-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz")
  abort("compose linux sha missing") unless compose_linux.fetch("sha256") == "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  react_entry = index.fetch(2).fetch("targets")
  react_expectations = {
    "aarch64-apple-darwin" => {
      "url" => "wax-lang-react-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz",
      "sha256" => "1111111111111111111111111111111111111111111111111111111111111111"
    },
    "aarch64-unknown-linux-gnu" => {
      "url" => "wax-lang-react-0.1.0-alpha.1-aarch64-unknown-linux-gnu.tar.gz",
      "sha256" => "7777777777777777777777777777777777777777777777777777777777777777"
    },
    "x86_64-apple-darwin" => {
      "url" => "wax-lang-react-0.1.0-alpha.1-x86_64-apple-darwin.tar.gz",
      "sha256" => "4444444444444444444444444444444444444444444444444444444444444444"
    },
    "x86_64-unknown-linux-gnu" => {
      "url" => "wax-lang-react-0.1.0-alpha.1-x86_64-unknown-linux-gnu.tar.gz",
      "sha256" => "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    }
  }
  react_expectations.each do |target, expected|
    artifact = react_entry.fetch(target)
    abort("react #{target} url missing") unless artifact.fetch("url").include?(expected.fetch("url"))
    abort("react #{target} sha missing") unless artifact.fetch("sha256") == expected.fetch("sha256")
  end
' "$tmp_dir/index.json"
