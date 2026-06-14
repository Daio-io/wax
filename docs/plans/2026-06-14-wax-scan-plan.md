# Wax Scan Analytics Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a project-scoped `wax-scan` Agent Skill that validates config, runs a fresh scan, extracts deterministic adoption metrics, and produces actionable terminal and optional HTML analytics reports.

**Architecture:** `wax-cli` and `wax-core` remain unchanged in this plan. The skill orchestrates existing `wax validate` and `wax scan` commands, reads `.wax/out/scan-merged.json`, runs a skill-local `extract-insights.sh` script for deterministic metrics, and uses agent narrative for hybrid gap-filling with confidence labels. HTML output is a self-contained dashboard template written to `.wax/out/report/index.html` when `--html` or `--html-only` is requested.

**Tech Stack:** Agent Skill `SKILL.md`, shell + `jq` extractor, static HTML/CSS template with inline SVG charts, existing `wax scan` / `wax validate` CLI.

---

## Reference Spec

- Design spec: [docs/specs/2026-06-14-wax-scan-design.md](../specs/2026-06-14-wax-scan-design.md)
- Roadmap source: [docs/plans/README.md](./README.md)
- Scan facts contract: [docs/specs/2026-05-16-language-packs-and-distribution.md](../specs/2026-05-16-language-packs-and-distribution.md)
- Component tracker reporting semantics: [docs/specs/2026-05-13-component-tracker-design.md](../specs/2026-05-13-component-tracker-design.md)
- Skill precedent: [skills/wax-registry-discover/SKILL.md](../../skills/wax-registry-discover/SKILL.md)

## File Structure

- Create `docs/specs/2026-06-14-wax-scan-design.md`
- Create `docs/plans/2026-06-14-wax-scan-plan.md`
- Create `skills/wax-scan/SKILL.md`
- Create `skills/wax-scan/reference.md`
- Create `skills/wax-scan/scripts/extract-insights.sh`
- Create `skills/wax-scan/templates/report.html`
- Create `skills/wax-scan/fixtures/scan-merged.sample.json`
- Create `skills/wax-scan/fixtures/expected-insights.sample.json`
- Create `skills/wax-scan/scripts/test-extract-insights.sh`
- Modify `README.md` — add `wax-scan` to AI skills section
- Modify `docs/plans/README.md` — add roadmap entry

## Execution model

- One task = one branch, one PR.
- Task PR titles: `Task N: <description> (wax-scan skill)`.
- No `engine/` changes required unless a later task promotes the extractor into `wax-core`.
- Verification for skill tasks is script/fixture based plus manual HTML smoke.

---

## Phase 1 — Skill scaffold and workflow

### - [x] Task 1: Add `wax-scan` skill scaffold

**Files:**
- Create: `skills/wax-scan/SKILL.md`
- Create: `skills/wax-scan/reference.md`
- Create: `skills/wax-scan/scripts/extract-insights.sh` (placeholder until Task 2)
- Create: `skills/wax-scan/templates/report.html` (placeholder until Task 3)

- [x] **Step 1: Create SKILL.md with parameters in frontmatter**

YAML frontmatter must include `--html`, `--html-only`, `--baseline`, and `--no-auto-install` in the `description` field. Add a `## Parameters` section immediately after the title.

```markdown
---
name: wax-scan
description: >-
  Use when running Wax scans and producing design system adoption analytics reports.
  Validates config, runs a fresh scan, outputs a section-by-section terminal report by default.
  Supports --html for dashboard HTML at .wax/out/report/index.html, --baseline for trend deltas,
  --no-auto-install for CI, and --html-only to skip terminal output.
---
```

- [x] **Step 2: Document workflow guardrails**

Include ordered steps:
1. Verify Wax config exists; stop with `wax init` guidance if missing.
2. Run `wax validate`; stop on failure.
3. Run fresh `wax scan` (pass `--no-auto-install` when requested).
4. Read `.wax/out/scan-merged.json`.
5. Run `skills/wax-scan/scripts/extract-insights.sh`.
6. Produce terminal report unless `--html-only`.
7. Write HTML when `--html` or `--html-only`.

- [x] **Step 3: Embed analytics spec verbatim**

Copy the full Design System Analytics and Adoption Specialist prompt from the design conversation into `SKILL.md` without paraphrasing. Preserve section order and output principles.

- [x] **Step 4: Create reference.md**

Document:
- Extractor output field definitions
- `limits[]` catalog for unavailable metrics
- Confidence labeling rules (deterministic / inferred medium / inferred low)
- Data-gap block template
- Baseline delta fields when `--baseline` is supplied

- [x] **Step 5: Commit Task 1**

---

## Phase 2 — Deterministic extractor

### - [x] Task 2: Implement `extract-insights.sh`

**Files:**
- Create: `skills/wax-scan/scripts/extract-insights.sh`
- Create: `skills/wax-scan/fixtures/scan-merged.sample.json`
- Create: `skills/wax-scan/fixtures/expected-insights.sample.json`
- Create: `skills/wax-scan/scripts/test-extract-insights.sh`

- [x] **Step 1: Define insights JSON shape**

Emit a versioned object including at minimum:

```json
{
  "schema_version": 1,
  "generated_at": "<rfc3339>",
  "source_scan": ".wax/out/scan-merged.json",
  "repo_summary": {
    "languages": [],
    "total_usage_sites": 0,
    "resolved_count": 0,
    "candidate_count": 0,
    "unresolved_count": 0,
    "adoption_coverage_ratio": null
  },
  "per_language": [],
  "symbol_rollups": {
    "design_system": [],
    "local": [],
    "unresolved": []
  },
  "fragmentation_candidates": [],
  "limits": [],
  "baseline_deltas": null
}
```

- [x] **Step 2: Implement extraction logic**

From `scan-merged.json`, compute:
- Per-language status, adoption %, counts
- Repository-level resolved/candidate/unresolved totals
- DS symbol usage frequency
- Local component symbol frequency
- Basic fragmentation groups (symbol prefix/suffix families like `*Button`, `*Modal`)
- `limits[]` entries for metrics not supported by current facts

- [x] **Step 3: Add optional baseline comparison**

When second argument `--baseline <path>` is supplied, compute deltas for:
- Adoption coverage ratio
- Resolved/candidate/unresolved counts
- Per-language adoption when comparable

Emit `baseline_deltas` or a single limit entry when baseline is incompatible.

- [x] **Step 4: Add fixture test script**

`skills/wax-scan/scripts/test-extract-insights.sh` runs the extractor against `skills/wax-scan/fixtures/scan-merged.sample.json` and diffs key fields against `skills/wax-scan/fixtures/expected-insights.sample.json`.

Run:

```bash
skills/wax-scan/scripts/test-extract-insights.sh
```

Expected: PASS.

- [x] **Step 5: Commit Task 2**

---

## Phase 3 — HTML dashboard template

### - [ ] Task 3: Add dashboard HTML report template

**Files:**
- Create: `skills/wax-scan/templates/report.html`

- [ ] **Step 1: Build self-contained dashboard shell**

Requirements:
- Embedded CSS only; no external assets or CDN
- Executive summary card pinned at top
- Section cards matching analytics spec order
- Severity badges: critical / high / medium / low
- Inline SVG bar charts for coverage %, debt score proxy, and top fragmentation counts when data exists
- Muted styling for data-gap sections
- Generated timestamp and source scan path in footer

- [ ] **Step 2: Document placeholder contract in reference.md**

Define placeholders the agent must fill from extractor JSON + narrative, for example:
- `{{health_score}}`, `{{coverage_percent}}`, `{{sections[]}}`, `{{recommendations[]}}`, `{{limits[]}}`

- [ ] **Step 3: Manual smoke checklist**

After a fixture-driven render:
- Open `.wax/out/report/index.html` in a browser with network disabled
- Verify cards, badges, and at least one SVG chart render
- Verify data-gap sections are visually distinct

- [ ] **Step 4: Commit Task 3**

---

## Phase 4 — Documentation and roadmap

### - [ ] Task 4: Wire docs and README

**Files:**
- Modify: `README.md`
- Modify: `docs/plans/README.md`

- [ ] **Step 1: Add README AI skills entry**

Document invocation, parameters (`--html`, `--html-only`, `--baseline`, `--no-auto-install`), and output paths.

- [ ] **Step 2: Add roadmap row**

Add order 10 (or next available) to `docs/plans/README.md`:

| Plan | Document | Doc status | Implementation status |
|------|----------|------------|------------------------|
| Wax scan analytics skill | `2026-06-14-wax-scan-plan.md` | `merged` | `in-progress` |

- [ ] **Step 3: Commit Task 4**

---

## Phase 5 — End-to-end skill validation

### - [ ] Task 5: Skill integration smoke

**Files:**
- Modify: `skills/wax-scan/SKILL.md` (only if smoke reveals doc gaps)

- [ ] **Step 1: Run against a fixture or sample repo**

In a repo with Wax config and registries:
1. Invoke skill workflow manually
2. Confirm validate → scan → extract → terminal report
3. Confirm `--html` writes `.wax/out/report/index.html`

- [ ] **Step 2: Verify guardrails**

Confirm skill docs require:
- Stop when config missing
- Stop when validate fails
- Always fresh scan
- Data-gap sections for unsupported metrics
- Trends skipped unless `--baseline` provided

- [ ] **Step 3: Commit Task 5**

---

## Verification summary

| Task | Command / check |
|------|-----------------|
| 2 | `skills/wax-scan/scripts/test-extract-insights.sh` |
| 3 | Manual offline HTML open smoke |
| 5 | Manual skill workflow on sample repo |

No `engine/` verification required for this plan unless later promoted.

## Future work (out of scope)

- Promote extractor into `wax-core` as `json-summary` when post-alpha UX Task 2 ships
- Consume `graph-data` for richer HTML charts
- Add composition/wrapper/override metrics when `ScanFacts` expands
- Automatic git-history baseline discovery
