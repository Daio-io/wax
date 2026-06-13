#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./scripts/generate-pack-index.sh [release-artifacts-dir] [output-index-json]

Reads release manifest files produced by scripts/build-release.sh and emits the
alpha language-pack index. The alpha index publishes compose, basic, react, and swift.

Defaults:
  release-artifacts-dir: release/artifacts
  output-index-json:     release/artifacts/index.json
USAGE
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifacts_dir="${1:-$repo_root/release/artifacts}"
output_path="${2:-$artifacts_dir/index.json}"

ruby -rjson -rfileutils -e '
  artifacts_dir = File.expand_path(ARGV.fetch(0))
  output_path = File.expand_path(ARGV.fetch(1))
  pack_binaries = {
    "compose" => "wax-lang-compose",
    "basic" => "wax-lang-basic",
    "react" => "wax-lang-react",
    "swift" => "wax-lang-swift"
  }

  manifest_paths = Dir.glob(File.join(artifacts_dir, "*", "manifest.json")).sort
  abort("no release manifest files found under #{artifacts_dir}") if manifest_paths.empty?

  version = nil
  entries = pack_binaries.transform_values { {} }

  manifest_paths.each do |path|
    manifest = JSON.parse(File.read(path))
    manifest_version = manifest.fetch("version")
    target = manifest.fetch("target")
    artifacts = manifest.fetch("artifacts")

    if version && version != manifest_version
      abort("mixed release versions in manifests: #{version} and #{manifest_version}")
    end
    version ||= manifest_version

    pack_binaries.each do |pack_id, binary|
      artifact = artifacts.fetch(binary) do
        abort("manifest #{path} missing required artifact #{binary}")
      end
      entries.fetch(pack_id)[target] = {
        "url" => artifact.fetch("url"),
        "sha256" => artifact.fetch("sha256")
      }
    end
  end

  index = pack_binaries.keys.map do |pack_id|
    targets = entries.fetch(pack_id)
    abort("no targets found for #{pack_id}") if targets.empty?

    {
      "id" => pack_id,
      "version" => version,
      "api_version" => 1,
      "targets" => targets.sort.to_h
    }
  end

  FileUtils.mkdir_p(File.dirname(output_path))
  File.write(output_path, "#{JSON.pretty_generate(index)}\n")
  puts "wrote #{output_path}"
' "$artifacts_dir" "$output_path"
