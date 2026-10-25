---
title: Continuous integration
sidebar_position: 4
---

# Run Wax in CI

Commit `.wax/wax.config.json`, `.wax/wax.lock.json`, and reviewed registry files.
Do not rely on a developer's global `~/.wax/` cache.

Install the engine, install every language pack at the version pinned by the
lockfile, and then use the CI-safe path:

```bash
wax language install <id>@<version>
wax validate
wax scan --no-auto-install --strict \
  --output json-summary=.wax/out/summary.json \
  --output markdown=.wax/out/summary.md
```

Repeat `wax language install <id>@<version>` for each lock-pinned pack.
`--no-auto-install` prevents network-driven pack installation during the scan.
`--strict` returns nonzero after writing artifacts if any language is partial or
failed. Upload `.wax/out/scan-merged.json` and requested outputs as CI artifacts.
To compare runs, download a previous JSON summary or merged scan and pass it via
`--baseline PATH`.

Language packs install globally under `~/.wax/langs/`, but validation resolves
the committed repository lock and does not depend on pre-existing global state.
The lockfile-driven install loop and complete workflow are in the
[CI scan-summary recipe](https://github.com/Daio-io/wax/blob/main/docs/ci-scan-summary.md).
