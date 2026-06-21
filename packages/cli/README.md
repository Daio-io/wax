# @waxhq/wax

Optional alpha npm wrapper for the wax design-system analysis CLI.

```bash
npm install -g @waxhq/wax@alpha
wax --help
```

```bash
npx @waxhq/wax@alpha --help
```

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
