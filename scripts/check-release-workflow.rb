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
  "tags:\n      - \"v*\"\n    branches-ignore:\n      - \"**\"",
  "tag-only release trigger"
)

require_includes!(workflow, "workflow_dispatch:", "manual release dry-run trigger")

require_includes!(
  workflow,
  'if [[ "$version_output" != "wax ${expected_version}" ]]; then',
  "exact wax --version comparison"
)

require_includes!(
  workflow,
  'for binary in wax wax-lang-compose wax-lang-basic wax-lang-react wax-lang-swift; do',
  "expected binary asset matrix validation"
)

require_includes!(
  workflow,
  'if [[ "$archive_count" != "20" || "$checksum_count" != "20" ]]; then',
  "expected 20 archive and checksum count validation"
)

require_includes!(
  workflow,
  'echo "expected 20 archives and 20 checksums; found ${archive_count} archives and ${checksum_count} checksums" >&2',
  "expected 20 archive and checksum count failure message"
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
  'publish-npm:
    name: Publish npm package',
  "trusted-publishing npm job"
)

require_includes!(
  workflow,
  "if: github.event_name == 'push'",
  "push-only publish guard"
)

require_includes!(
  workflow,
  "id-token: write",
  "OIDC permission for npm trusted publishing"
)

require_includes!(
  workflow,
  "tmp_package_json=\"$(mktemp)\"",
  "temporary package.json rewrite file"
)

require_includes!(
  workflow,
  "package_file=\"packages/cli/package.json\"",
  "npm package file path for release-time rewrite"
)

require_includes!(
  workflow,
  "pkg.version = process.env.WAX_RELEASE_TAG.replace(/^v/, \"\")",
  "npm package version derived from release tag"
)

require_includes!(
  workflow,
  "mv \"$tmp_package_json\" \"$package_file\"",
  "atomic package.json rewrite before npm publish"
)

require_includes!(
  workflow,
  "stamped_version=\"$(node -p 'require(\"./packages/cli/package.json\").version')\"",
  "read-back stamped package version"
)

require_includes!(
  workflow,
  "plugin_file=\".claude-plugin/plugin.json\"",
  "Claude skills plugin manifest file path for release-time rewrite"
)

require_includes!(
  workflow,
  "plugin.version = process.env.WAX_RELEASE_TAG.replace(/^v/, \"\")",
  "Claude skills plugin version derived from release tag"
)

require_includes!(
  workflow,
  "stamped_plugin_version=\"$(node -p 'require(\"./.claude-plugin/plugin.json\").version')\"",
  "read-back stamped Claude skills plugin version"
)

require_includes!(
  workflow,
  'echo ".claude-plugin/plugin.json version ${stamped_plugin_version} does not match stamped release tag ${expected_version}" >&2',
  "explicit stamped Claude skills plugin version mismatch failure"
)

require_includes!(
  workflow,
  'echo "packages/cli/package.json version ${stamped_version} does not match stamped release tag ${expected_version}" >&2',
  "explicit stamped npm version mismatch failure"
)

require_includes!(
  workflow,
  "unset NODE_AUTH_TOKEN NPM_TOKEN npm_config__authToken",
  "legacy npm auth token clearing before trusted publish"
)

require_includes!(
  workflow,
  "sed -i '/:_authToken/d' \"$npmrc\"",
  "setup-node .npmrc auth clearing before trusted publish"
)

require_includes!(
  workflow,
  'npm publish --provenance --access public --tag "$npm_tag"',
  "npm provenance publish command"
)

require_includes!(
  workflow,
  "release/artifacts/${{ matrix.target }}/manifest.json",
  "release manifest artifact upload"
)

require_includes!(
  workflow,
  "./scripts/test-generate-pack-index.sh",
  "pack index generator regression test"
)

require_includes!(
  workflow,
  "./scripts/generate-pack-index.sh release-manifests release-assets/index.json",
  "pack index generation from downloaded release manifests"
)

require_includes!(
  workflow,
  "WAX_PACK_INDEX_URL: file://${{ github.workspace }}/release-assets/index.json",
  "generated pack index validation URL"
)

require_includes!(
  workflow,
  "cargo test -p wax-core --locked validates_pack_index_from_env -- --ignored --nocapture",
  "pre-publish generated pack index validation"
)

require_includes!(
  workflow,
  "release-assets/index.json",
  "index.json release asset publication"
)

require_includes!(
  workflow,
  "git fetch origin refs/heads/gh-pages:refs/remotes/origin/gh-pages || true",
  "gh-pages remote-tracking ref fetch"
)

require_includes!(
  workflow,
  "git -C gh-pages-worktree push origin HEAD:gh-pages",
  "gh-pages pack index publication"
)

if workflow.include?("make_latest:")
  warn "release workflow must not set make_latest for prerelease alpha tags"
  exit 1
end

if workflow.include?("/releases/latest/download/index.json")
  warn "release workflow must not rely on GitHub Releases latest for alpha index"
  exit 1
end

require_includes!(
  workflow,
  "cp release-assets/index.json gh-pages-worktree/index.json",
  "gh-pages index copy"
)

require_includes!(
  workflow,
  "cargo test -p wax-core --locked fetches_published_default_pack_index -- --ignored --nocapture",
  "post-release fetch_pack_index default URL verification"
)

require_includes!(
  workflow,
  "WAX_EXPECTED_RELEASE_TAG: ${{ env.WAX_RELEASE_TAG }}",
  "current release tag passed to pack index verification"
)

require_includes!(
  workflow,
  "for attempt in {1..12}; do",
  "published pack index verification retry loop"
)
