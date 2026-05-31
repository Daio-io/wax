#!/usr/bin/env ruby
# frozen_string_literal: true

workflow = File.read(File.expand_path("../.github/workflows/release.yml", __dir__))

def require_includes!(workflow, needle, description)
  return if workflow.include?(needle)

  warn "missing #{description}: #{needle}"
  exit 1
end

require_includes!(
  workflow,
  'if [[ "$version_output" != "wax ${expected_version}" ]]; then',
  "exact wax --version comparison"
)

require_includes!(
  workflow,
  'for binary in wax wax-lang-compose wax-lang-basic; do',
  "expected binary asset matrix validation"
)

require_includes!(
  workflow,
  'for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do',
  "expected target asset matrix validation"
)

require_includes!(
  workflow,
  'verify-release-assets:
    name: Verify release assets
    runs-on: ubuntu-latest',
  "read-only release asset validation job"
)

require_includes!(
  workflow,
  'publish:
    name: Publish GitHub Release',
  "push-only publish job"
)

require_includes!(
  workflow,
  "if: github.event_name == 'push'",
  "push-only publish guard"
)

require_includes!(
  workflow,
  "release/artifacts/${{ matrix.target }}/manifest.json",
  "release manifest artifact upload"
)

require_includes!(
  workflow,
  "./scripts/generate-pack-index.sh release-manifests release-assets/index.json",
  "pack index generation from downloaded release manifests"
)

require_includes!(
  workflow,
  "release-assets/index.json",
  "index.json release asset publication"
)

require_includes!(
  workflow,
  "make_latest: true",
  "published release marked latest for default index URL"
)

require_includes!(
  workflow,
  "cargo test -p wax-core fetches_published_default_pack_index -- --ignored --nocapture",
  "post-release fetch_pack_index default URL verification"
)
