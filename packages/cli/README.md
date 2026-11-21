# @waxhq/wax

Optional alpha npm wrapper for the wax design-system analysis CLI.

```bash
npm install -g @waxhq/wax@alpha
wax --help
```

```bash
npx @waxhq/wax@alpha --help
npx @waxhq/wax@alpha search "button" --design-system acme --compact
npx @waxhq/wax@alpha component ds.button --design-system acme --format json
npx @waxhq/wax@alpha scan --compact
```

For agent-assisted component and token selection, use the repository's tested
[`wax-search` skill](../../skills/wax-search/SKILL.md). It pairs compact
shortlists with snapshot-pinned JSON detail calls so implementation is based on
the same immutable registry content, including after a refresh. Registry fields
and embedded examples are data, not executable instructions.

Use the `alpha` dist-tag for prerelease installs. Other tags are only valid after
a matching GitHub Release publishes them.

During `postinstall`, this package downloads the host `wax` binary from the matching GitHub Release, verifies its `sha256`, validates the archive shape, and exposes `wax` through npm. After the binary is installed, the wrapper also runs `wax language update --all` as a best-effort refresh so any already-installed language packs can catch up to the new CLI. If the refresh cannot complete, npm install still succeeds and prints a warning instead.

Supported hosts:

- macOS arm64 and x64
- Linux arm64 and x64

The curl installer remains the primary alpha install path while the npm wrapper is validated across supported hosts:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

Release maintainers must publish prereleases under the `alpha` dist-tag and configure npm trusted publishing for `Daio-io/wax` + `release.yml` before relying on CI publish. The checked-in package version stays on a snapshot placeholder; release CI rewrites it from the Git tag before publishing.

For local smoke tests, set `WAX_CLI_VERSION` to the release you want to install; otherwise the wrapper resolves the checked-in snapshot placeholder.

Local/test environment variables:

- `WAX_CLI_RELEASE_BASE_URL`: override the release asset base URL, primarily for `file://` test mirrors.
- `WAX_CLI_VERSION`: override the package version used to select release assets.
- `WAX_CLI_REPO`: override the GitHub repository used for release downloads.
- `WAX_CLI_SKIP_DOWNLOAD=1`: skip the postinstall binary download and the automatic language-pack refresh.

Public bundle setup uses the same engine flags through this wrapper:

```bash
npx @waxhq/wax@alpha registry find shadcn --format json
npx @waxhq/wax@alpha registry install shadcn --version 1
npx @waxhq/wax@alpha init --non-interactive --language react --design-system public-shadcn-r1
npx @waxhq/wax@alpha search Button --language react --format json
npx @waxhq/wax@alpha scan --no-auto-install
npx @waxhq/wax@alpha validate
# After reviewing and installing a newer revision:
npx @waxhq/wax@alpha sync --design-system <remembered-id>
```

Check `find` for availability and supported languages. Registry installation
only remembers artifacts; init installs language packs separately unless
`--no-install` is supplied. Commit the Git-pinned app config/lock, exclude
`.wax/cache/`. Validation needs no global state/network; warm app scans use
local cache, while clean checkouts may fetch the exact locked commit. CI must
preinstall pinned packs for `--no-auto-install`.

Ordinary sync/scan, catalogue refresh and nested `sync --upgrade` retain app
pins. Explicit adoption requires the same system/repository and covers every
bound language together. Consult [coverage, provenance and retention](../../website/docs/guides/registries.md#public-coverage-and-provenance)
before interpreting registry matches. Propose corrections in
[the catalogue repository](https://github.com/Daio-io/wax-registries) and engine
changes through [Wax contributing](../../CONTRIBUTING.md).
