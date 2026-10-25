---
title: wax-scan
sidebar_position: 3
---

# wax-scan

Use `wax-scan` to run a fresh repository scan and turn deterministic facts into
design-system adoption analytics. Terminal output is the default; the workflow
can also render its reviewed, self-contained HTML report.

Safety boundary: Analyze only fresh validated scan evidence, and never write inferred values into registries or metrics.

## Prerequisites

- Repository-local `.wax/wax.config.json` and, for locked language packs,
  `.wax/wax.lock.json`.
- The Wax CLI and any required language packs.
- The skill's extractor and optional HTML template from the repository.

## Happy path

Validate first, optionally refresh an upstream registry when explicitly wanted,
then scan afresh:

```bash
wax sync
wax validate
wax scan
skills/wax-scan/scripts/extract-insights.sh .wax/out/scan-merged.json
```

For CI, pass through strict offline-install behavior:

```bash
wax validate
wax scan --no-auto-install --strict
```

Use `--baseline <path>` only when a previous `scan-merged.json` is available.
Use the skill's `--html` or `--html-only` workflow option to produce
`.wax/out/report/index.html`.

## Output

The deterministic extractor supplies DS-versus-local coverage, raw invocation
breakdowns, registry usage, root groups, parent-scope hotspots, local migration
candidates, token inference, diagnostics, and supported trend deltas. The HTML
report uses the maintained template, local CSS/inline SVG, and escaped
scan-derived strings—no CDN assets.

## Failures and guardrails

- Stop after `wax validate` fails; do not analyze an old artifact as though it
  were the requested fresh scan.
- Prefer extractor or engine artifacts over agent estimation. Label every
  inference and identify unsupported metrics as data gaps.
- Pair hard-coded observation inventory with the assessed count. Unmatched and
  unassessed observations are not automatically debt.
- Do not combine deterministic token classes into a made-up health/debt score.
- Never auto-write registry values. Offer
  [`wax-registry-discover`](./wax-registry-discover.md) only for an evidenced
  publisher gap and only after user acceptance.
- Escape every scan-derived symbol, path, or narrative before HTML insertion.

Read the canonical
[`skills/wax-scan/SKILL.md`](https://github.com/Daio-io/wax/blob/main/skills/wax-scan/SKILL.md),
the [extractor reference](https://github.com/Daio-io/wax/blob/main/skills/wax-scan/reference.md),
and the [report template](https://github.com/Daio-io/wax/blob/main/skills/wax-scan/templates/report.html)
for complete operational detail.
