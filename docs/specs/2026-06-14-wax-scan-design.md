# Wax Scan Analytics Skill Design

## Summary

Wax should provide a project-scoped Agent Skill, `wax-scan`, that orchestrates a fresh repository scan and produces an actionable design-system adoption analytics report. The skill validates configuration, always re-scans, extracts deterministic metrics from `scan-merged.json`, and uses AI-assisted narrative only for gaps—with explicit confidence labels and data-gap callouts.

Default output is a section-by-section terminal report. Optional `--html` writes a self-contained dashboard-style HTML report to `.wax/out/report/index.html`.

AI is an interpretation layer for the skill workflow. It is not a runtime dependency of `wax scan`, `wax validate`, or language-pack execution.

## Goals

- Run `wax validate` then a fresh `wax scan` on every skill invocation.
- Produce adoption analytics that prioritize actionable insights over vanity metrics.
- Compute deterministic metrics from current `ScanFacts` / `MergedScan` contracts.
- Fill unavailable metrics with labeled AI inference and explicit data-gap sections.
- Default to terminal output in the section order defined by the analytics spec.
- Support `--html` for a dashboard-style local report at `.wax/out/report/index.html`.
- Support optional `--baseline` for limited trend deltas when the user supplies a prior scan artifact.
- Stop on `wax validate` failures; do not scan until validation passes.
- Remain compatible with future engine reporting (`json-summary`, `graph-data`, HTML artifacts from post-alpha UX) without duplicating that work in the engine.

## Non-Goals

- Changing `ScanFacts`, `MergedScan`, or language-pack wire contracts in this phase.
- Building hosted dashboards or a backend API.
- Automatic historical scan discovery from git history in v1.
- Making AI part of `wax scan` or `wax validate` runtime behavior.
- Replacing post-alpha UX Task 2 engine reporting; this skill is a bridge until richer artifacts ship.

## Relationship to Existing Work

| Surface | Role |
|---------|------|
| `wax scan` | Deterministic scan orchestration; writes `.wax/out/scan-merged.json` |
| `wax validate` | Repo-local config gate; must pass before scan |
| `skills/wax-registry-discover` | Precedent for CLI orchestration + artifact review + agent judgment |
| Post-alpha UX plan Task 2 | Future engine-owned `json-summary`, `graph-data`, markdown, and HTML artifacts |
| Component tracker design | Long-term reporting semantics (wrappers, drift, reach) the skill will consume when facts support them |

When post-alpha engine summaries ship, the skill should prefer engine artifacts when present and fall back to the skill-local extractor otherwise.

## User Experience

### Default invocation

```text
/wax-scan
```

or natural language such as “scan this repo and report on design system adoption.”

### Parameters

| Parameter | Effect |
|-----------|--------|
| *(none)* | Terminal section-by-section report |
| `--html` | Also write `.wax/out/report/index.html` |
| `--html-only` | Write HTML only; skip terminal report |
| `--baseline <path>` | Compare against a prior `scan-merged.json` for limited trend deltas |
| `--no-auto-install` | Pass through to `wax scan` for CI-style runs with committed lockfiles |

### Workflow

```text
1. Verify Wax config exists (.waxrc or .wax/wax.config.json)
   → missing: stop, guide to `wax init`

2. Run `wax validate`
   → failures: stop, show errors, do not scan

3. Run `wax scan` (always fresh)

4. Read `.wax/out/scan-merged.json`
   (+ optional `--baseline` when supplied)

5. Run deterministic extractor → intermediate insights JSON

6. Produce terminal report (section-by-section, analytics spec order)

7. If --html or --html-only → render `.wax/out/report/index.html`
```

### Terminal report shape

The report walks through all analytics sections in spec order. Sections without supporting scan facts still appear with a standard data-gap block:

```text
Data gap: <metric> requires <missing capability>. Not computed in this scan.
```

Each populated insight answers:

1. What was found?
2. Why does it matter?
3. How severe is it?
4. What should be done next?
5. What benefit is expected?

### HTML report

Triggered only when the user passes `--html` or `--html-only`.

| Item | Value |
|------|-------|
| Path | `.wax/out/report/index.html` |
| Style | Dashboard — black background, beeswax yellow accent, KPI cards, horizontal SVG bar charts, data tables |
| Network | Self-contained; embedded CSS; no CDN |
| Content | Same sections as terminal; KPI grid and charts at top; executive summary near top; data-gap sections visually muted |

Unless `--html-only` is set, the terminal report still prints when HTML is requested.

## Architecture

### Approach

**Skill + deterministic extractor + agent narrative** (recommended and selected).

```text
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────────┐
│  wax-scan skill │────▶│ wax validate     │────▶│ wax scan (fresh)    │
└────────┬────────┘     └──────────────────┘     └──────────┬──────────┘
         │                                                    │
         │                    ┌───────────────────────────────┘
         │                    ▼
         │           ┌────────────────────┐
         │           │ scan-merged.json   │
         │           └─────────┬──────────┘
         │                     ▼
         │           ┌────────────────────┐
         │           │ extract-insights   │  deterministic (jq/script)
         │           └─────────┬──────────┘
         │                     ▼
         │           ┌────────────────────┐
         └──────────▶│ Agent narrative    │  hybrid insights + gaps
                     └─────────┬──────────┘
                               ▼
                     ┌────────────────────┐
                     │ Terminal report    │
                     │ HTML (optional)    │
                     └────────────────────┘
```

### Skill layout

```text
skills/wax-scan/
├── SKILL.md
├── reference.md
├── fixtures/
│   ├── scan-merged.sample.json
│   └── expected-insights.sample.json
├── templates/
│   └── report.html
└── scripts/
    ├── extract-insights.sh
    └── test-extract-insights.sh
```

### Deterministic metrics (Adoption Metrics v2)

| Section | v2 availability | Source |
|---------|-----------------|--------|
| UI invocation adoption | Yes | `repo_summary.metrics.invocation_adoption_ratio` and `counts.adoption` |
| Registry resolution | Yes | `repo_summary.metrics.registry_resolution_ratio` |
| Raw invocation breakdown | Yes | `counts.raw_invocations.{resolved,local,candidate,unresolved}` |
| Local definition inventory | Yes | `counts.definitions` and `symbol_usage_summary[]` local rows |
| Unresolved UI calls | Yes | `counts.raw_invocations.unresolved` and unresolved symbol rollups |
| Parent-scope hotspots | Partial | `symbol_usage_summary[].parent_scopes` and `counts.parent_scopes` |
| Custom component analysis | Partial | `local_components` + local invocation rollups |
| Component health (basic) | Partial | DS symbol rollups and registry breadth |
| Fragmentation (basic) | Partial | Local symbol grouping by naming patterns |
| Feature/route/team coverage | No | `limits[]` data gap |
| Override analysis | No | `limits[]` data gap |
| Deprecated components | No | `limits[]` unless registry metadata expands |
| Version adoption | No | `limits[]` unless pack/version facts expand |
| Wrapper proliferation | No | `limits[]` — no composition edges in facts yet |
| Trends | Optional | `--baseline` with compatible v2 `scan-merged.json`; v1 baselines emit a compatibility gap |

### Hybrid labeling

| Label | Meaning |
|-------|---------|
| **Deterministic** | Directly from extractor JSON |
| **Inferred (medium confidence)** | Pattern heuristics with evidence (e.g. `PrimaryButton` → likely DS `Button` duplicate) |
| **Inferred (low confidence)** | Weak signal; include evidence count |

Executive summary scores (health, maturity, debt) are agent-synthesized composites of available metrics. When data is sparse, the report must explain weighting and uncertainty rather than imply false precision.

### Trend analysis (v2)

- No automatic git-history baseline discovery.
- When `--baseline <path>` is provided, the baseline must be a prior v2 `scan-merged.json`. Compute deltas for UI invocation adoption, registry resolution, raw invocation counters, parent-scope totals, and per-language status when computable.
- v1 baselines emit a compatibility data gap instead of mixing denominators.
- Otherwise emit a single trends data-gap section.

## Analytics Spec

The full analytics specialist prompt and section definitions live verbatim in `skills/wax-scan/SKILL.md`. The skill enforces:

- Actionable insights over raw statistics
- Priority recommendations (P0–P3)
- Executive summary with health score, top wins, top opportunities
- All primary objectives: adoption, coverage, debt, component health, version adoption, fragmentation, migration opportunities, missing capabilities, trends

## Error Handling

The skill stops when:

- Wax config is missing
- `wax validate` fails
- `wax scan` fails
- `scan-merged.json` is missing or unreadable after scan
- Extractor script fails

Warnings (non-blocking when scan succeeds):

- Partial language scan status
- Sparse usage data (zero usage sites)
- Baseline file incompatible with current merge shape

## Testing

| Layer | Coverage |
|-------|----------|
| Extractor script | Fixture-based tests against committed `scan-merged.json` samples |
| Skill docs | Workflow guardrails: validate-before-scan, stop on validate failure, fresh scan, `--html` path |
| HTML template | Manual smoke: open `index.html` offline; verify cards, badges, and at least one SVG chart render |

Engine crate tests are not required for this phase unless the extractor is later promoted into `wax-core`.

## Documentation

- Design spec: this document
- Implementation plan: `docs/plans/2026-06-14-wax-scan-plan.md`
- Skill: `skills/wax-scan/SKILL.md` with `--html` and related parameters in frontmatter and a `## Parameters` section
- `README.md`: short AI skills section entry when implementation lands
- `docs/plans/README.md`: roadmap entry for the wax-scan skill plan

## Future Compatibility

When post-alpha UX ships `json-summary` and `graph-data`:

1. Skill checks for `.wax/out/scan-summary.json` and `.wax/out/scan-graph.json` after scan.
2. Prefer engine artifacts for deterministic sections.
3. Retain skill-local extractor as fallback.
4. Expand HTML template to consume additional graph metrics without changing the default `--html` path.
