---
title: Scan
sidebar_position: 3
---

# Scan a repository

Validate committed repository-local inputs before scanning:

```bash
wax validate
wax scan
```

Optionally run `wax sync` between validate and scan when you want to refresh
app registry snapshots from remembered global sources.

For a one-shot configless scan in a repository without `.wax/wax.config.json`,
select one language, repeat `--root` for each repository-relative source root,
and provide a remembered design-system id or an explicit local, HTTP, or `git+`
registry source:

```bash
wax scan --language react --root src \
  --design-system https://design.example.com/wax/react.registry.json

wax scan --language react --root src \
  --design-system acme --offline
```

`--tag` refines an explicit Git selector. `--offline` forbids registry source
I/O and requires the selected immutable snapshot to already be cached. These
configless scans write operational cache and `.wax/out/` data only; they do not
create `.wax/wax.config.json`, `.wax/wax.lock.json`, or `.wax/registries/`.
Use `wax init` and committed config/lockfiles for reproducible team and CI
scans.

Wax writes the merged facts to `.wax/out/scan-merged.json`. The default scan is
best effort: it can produce an artifact and exit successfully when a language is
`partial` or `failed`. Use strict mode when those statuses must fail the run:

```bash
wax scan --strict
```

Useful focused runs include:

```bash
wax scan --no-auto-install --strict
wax scan --compact
wax scan --root-group mobile
wax scan --format json-summary
wax scan --format markdown --baseline .wax/out/previous-summary.json
wax scan --output json-summary=.wax/out/summary.json \
  --output graph-data=.wax/out/graph.json \
  --output html=.wax/out/report.html
```

`--output FORMAT=PATH` is repeatable and combines with configured `outputs`.
Paths are repository-relative unless `--allow-absolute-output` is explicit.
Stdout formats are `summary`, `quiet`, `json-summary`, and `markdown`; artifact
formats are `json-summary`, `graph-data`, `markdown`, and `html`.
`--compact` is an agent-oriented text projection: it emits one stdout line with
the merged artifact path, language statuses, headline adoption and token counts,
failure count, artifact count, and ephemeral marker. Read
`.wax/out/scan-merged.json` for full analytics. It cannot be combined with an
explicit `--format`; default rich output and JSON contracts remain unchanged.

The [scan-output reference](../reference/scan-output.md) describes each contract.
Exact CLI behavior lives in
[`commands/scan.rs`](https://github.com/Daio-io/wax/blob/main/engine/crates/wax-cli/src/commands/scan.rs).
