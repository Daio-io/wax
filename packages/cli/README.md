# @wax/cli

Optional alpha npm wrapper for the wax design-system analysis CLI.

```bash
npm install -g @wax/cli
wax --help
```

```bash
npx @wax/cli --help
```

During `postinstall`, this package downloads the host `wax` binary from the matching GitHub Release, verifies its `sha256`, validates the archive shape, and exposes `wax` through npm.

Supported hosts:

- macOS arm64 and x64
- Linux arm64 and x64

The curl installer remains the primary alpha install path while the npm wrapper is validated across supported hosts:

```bash
curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash
```

Local/test environment variables:

- `WAX_CLI_RELEASE_BASE_URL`: override the release asset base URL, primarily for `file://` test mirrors.
- `WAX_CLI_VERSION`: override the package version used to select release assets.
- `WAX_CLI_REPO`: override the GitHub repository used for release downloads.
- `WAX_CLI_SKIP_DOWNLOAD=1`: skip the postinstall binary download.
