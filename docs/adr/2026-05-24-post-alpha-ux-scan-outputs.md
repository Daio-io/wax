# ADR: Post-alpha UX scan outputs and reports

**Status:** Accepted (implemented)
**Date:** 2026-05-24
**Type:** CLI output contract and reporting artifacts
**Related:** [Archived implementation plan](../plans/archive/2026-05-24-post-alpha-ux-plan.md) · [Interactive init ADR](./2026-06-13-interactive-init.md) · [CI scan summaries](../ci-scan-summary.md)

## Context

The alpha `wax scan` printed a minimal stdout summary. Teams needed scan results
in three places: the terminal, CI and pull-request surfaces, and durable
artifacts that charts and later reporting could consume without reparsing the
merged scan.

## Decision

1. **Two output channels.** `--format` selects stdout (`summary` by default,
   `quiet`, `json-summary`, or `markdown`). Repeatable `--output <format=path>`
   flags and config `outputs[]` defaults write `json-summary`, `graph-data`,
   `markdown`, and `html` artifacts. CLI flags append to config defaults,
   repeated pairs are idempotent, and distinct formats sharing one path are
   rejected.
2. **Repo-relative artifacts.** Output paths are repo-relative unless
   `--allow-absolute-output` is passed. Examples write under `.wax/out/`.
3. **Versioned contracts.** `json-summary` and `graph-data` are versioned JSON
   validated by `scan-summary.schema.json` and `scan-graph.schema.json`. Both
   report explicit limits when facts cannot support a rollup.
4. **Static reports, not hosted UI.** The HTML report renders offline from
   summary and graph data. Markdown targets PR comments and supports
   `--baseline` deltas against an earlier summary or merged scan.
5. **Packs stay fact-only.** `wax-cli` and `wax-core` own derived summaries,
   graph data, and reports. The `ScanFacts` contract does not change for output
   formatting.

## Implementation summary

| Work | What shipped | PR |
|------|--------------|----|
| Guided init | Interactive `wax init` wizard (see its own ADR) | #148 |
| THE-635 | `--format`, `json-summary`, config `outputs[]` | #321 |
| THE-636 | `graph-data` artifact and schema | #322 |
| THE-639 | Static HTML scan report | #323 |
| THE-637 | Markdown summary, `--baseline` deltas, CI recipe | #324 |
| THE-638 | Scan output documentation and closeout | #325 |

## Consequences

- CI can run a plain `wax scan` and get stable machine and PR-friendly outputs.
- Graph consumers evolve against a versioned shape instead of `MergedScan`.
- Registry discover/draft, rich validation, multi-page export, and a hosted web
  UI stayed out of scope.
