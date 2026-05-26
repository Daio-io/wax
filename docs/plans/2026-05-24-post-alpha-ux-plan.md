# Post-Alpha UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PR boundary:** Treat each checked **Task** as one implementation PR. Complete all steps inside a task, run its verification commands, commit the task, tick the task checkbox in this plan in the same PR, and open a PR before starting the next task.

**Goal:** Improve day-one ergonomics after the initial alpha ships—guided onboarding, richer scan feedback, durable data exports, graph-ready report artifacts, and CI-friendly output—without building registry discover/draft.

**Architecture:** Builds on [Release and rollout](./2026-05-24-release-and-rollout-plan.md) once public alpha is live (`wax scan` summary, `wax validate`, install channels). CLI-first UX and reporting layers; no changes to the `ScanFacts` contract unless a task explicitly requires new optional fields. Language packs continue to emit facts only; `wax-cli` / `wax-core` own derived summaries, graph data, and report artifacts.

**Tech Stack:** Rust `wax-cli`, `wax-core`, clap, terminal prompts (e.g. `dialoguer` or `inquire`—pick one crate in Task 1), JSON schema, markdown/text formatters for CI summaries, lightweight static HTML report generation

**Specs (review first):**

- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — registry lifecycle and CLI product surfaces (discover/draft remain out of scope here)
- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — existing `wax init` / `wax scan` behavior

**Previous phases:**

- [Rust engine and language packs](./2026-05-16-rust-engine-language-packs-plan.md)
- [Release and rollout](./2026-05-24-release-and-rollout-plan.md) — alpha tag + public install path

**Roadmap:** [Plan execution order](./README.md) — this is **order 3**; plan doc and implementation start only after order 2 public alpha ships.

---

## When to start

**Gate:** Do not begin until the release plan has shipped **public alpha** (curl + Homebrew + getting-started docs, at minimum `v0.1.0-alpha.1` or agreed tag).

**Owner / timeline:** Assign when tagging alpha.1 (e.g. “post-alpha UX squad” or single maintainer). Target: first milestone within one sprint after alpha.1 if capacity allows.

---

## Scope

| In scope | Out of scope |
|----------|----------------|
| Interactive `wax init` when TTY available | Registry discover / draft (separate plan) |
| Richer `wax scan` output formats for humans and CI | Backend API / hosted web UI |
| Stable JSON summary and graph-data exports | Changing alpha lockfile or pack index policy |
| Static local HTML report for adoption graphs | Full multi-page `wax export` site |
| PR-friendly markdown scan summaries and deltas | Registry discover / draft (separate plan) |
| Preserve `--non-interactive` for all scripted paths | Historical storage service |

Alpha already delivers a minimum stdout scan summary (release plan Task 3). This plan extends interpretability and onboarding by making scan results useful in three places: the terminal, CI/PR surfaces, and durable artifacts that later graph/reporting work can consume.

---

## Output model

`wax scan` has two output channels:

1. **Stdout format:** selected by `--format`, intended for immediate terminal or pipe usage.
2. **Artifact outputs:** selected with repeatable `--output <format=path>` flags and optional `.waxrc` defaults, intended for files checked by humans, CI, or later tooling.

Stdout formats:

| Format | Purpose | Stdout content |
|--------|---------|----------------|
| `summary` | Default human scan result | Path to merged scan, language statuses, adoption %, capped diagnostics, enabled artifacts |
| `quiet` | Scripts that only need the canonical result path | Path to `.wax/out/scan-merged.json` |
| `json-summary` | One-shot automation | Single JSON summary object, no surrounding prose |
| `markdown` | PR comment text when no file output is desired | Markdown summary section |

Artifact outputs:

```bash
wax scan \
  --output json-summary=.wax/out/scan-summary.json \
  --output graph-data=.wax/out/scan-graph.json \
  --output markdown=.wax/out/scan-summary.md \
  --output html=.wax/out/report/index.html
```

`.waxrc` may define matching defaults so CI can run plain `wax scan`:

```json
{
  "outputs": [
    { "format": "json-summary", "path": ".wax/out/scan-summary.json" },
    { "format": "graph-data", "path": ".wax/out/scan-graph.json" },
    { "format": "markdown", "path": ".wax/out/scan-summary.md" },
    { "format": "html", "path": ".wax/out/report/index.html" }
  ]
}
```

CLI `--output` flags append to `.waxrc` outputs. Repeating the same `format=path` pair is idempotent. Output paths must be repo-relative by default; absolute paths require an explicit `--allow-absolute-output` escape hatch if implementation needs it. Generated outputs remain under `.wax/out/` in examples and must stay out of git unless a fixture intentionally commits them.

### Stable JSON summary

`json-summary` is a stable, versioned report contract for CI and downstream dashboards. It must include at least:

- `schema_version` and `generated_at`
- `repo_root`, `scan_path`, `snapshot_ids`
- `languages[]`: id, version, status, parser, files scanned, adoption coverage ratio, usage counts, resolved/candidate counts
- `adoption`: repository coverage ratio plus rollups available from current facts
- `diagnostics[]`: severity, code, message, language, optional source location
- `artifacts[]`: format, path, byte size when known
- `limits[]`: warnings when rollups are unavailable because facts do not yet include module/category/ownership data

Create `engine/schemas/scan-summary.schema.json` with the first implementation and validate fixture output against it.

### Graph-data JSON

`graph-data` is for charting and later report UIs. It should be normalized enough to render adoption graphs without reparsing `MergedScan`:

- `nodes[]`: design-system components, local components, languages, files, and reporting boundaries when known
- `edges[]`: usage, composition/wrapper, ownership/reporting-boundary, and replacement edges when facts support them
- `metrics[]`: named metric values scoped to repo, language, component, category, or boundary
- `metadata`: schema version, source scan path, generated timestamp, and known data gaps

The first pass may omit node/edge kinds not present in current `ScanFacts`, but it must preserve a versioned shape so graph consumers can evolve safely.

### Markdown and HTML reports

Markdown is optimized for CI/PR comments: adoption headline, per-language table, changed metrics when a baseline is provided, top diagnostics, and artifact links.

HTML is a static local report, not a hosted web UI. It should be self-contained or write assets next to `index.html`, render without network access, and include:

- adoption headline and per-language cards
- at least one visual graph from `graph-data` or summary metrics
- diagnostics table with severity grouping
- links to raw JSON artifacts
- explicit limits when module/category/ownership rollups are unavailable

---

## Execution model

- One task = one branch, one PR.
- Task PR titles: `Task N: <description> (post-alpha UX)`.
- Re-run full engine verification when touching `wax-cli` / `wax-core`:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Phase 1 — Guided init

### - [ ] Task 1: Interactive `wax init` TTY wizard

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Modify: `engine/crates/wax-cli/Cargo.toml` (prompt dependency)
- Create: `engine/crates/wax-cli/tests/init_interactive.rs` (TTY-gated or mocked)

- [ ] **Step 1: Choose prompt library and document non-interactive invariant**

CI and scripts must continue to use `--non-interactive`. Interactive mode only when stdin is a terminal and flag is absent.

- [ ] **Step 2: Prompt for language (Compose-first), roots, optional first scan**

Reuse existing init install/lockfile logic after selections. Default language list matches alpha index (`compose`; offer `basic` for smoke; do not offer `react` until production-ready).

- [ ] **Step 3: Fall back to current behavior when not a TTY**

Clear message: use `--non-interactive` with `--language` and other flags.

- [ ] **Step 4: Manual smoke + unit tests with mocked stdin**

Run: `cd engine && cargo test -p wax-cli init_interactive`

Expected: PASS; interactive path writes same artifacts as non-interactive equivalent.

---

## Phase 2 — Scan output and data artifacts

### - [ ] Task 2: `wax scan` output controls and JSON summary

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-core/src/config.rs` (or current `.waxrc` config module)
- Create: `engine/schemas/scan-summary.schema.json`
- Create: `engine/crates/wax-cli/tests/scan_output_format.rs`
- Create: `engine/crates/wax-cli/tests/scan_output_artifacts.rs`

- [ ] **Step 1: Add stdout `--format` flag**

Supported values: `summary` (default, release plan Task 3 behavior), `quiet` (path only), `json-summary` (single JSON object on stdout for scripts), `markdown` (PR-ready markdown on stdout).

- [ ] **Step 2: Add repeatable artifact outputs**

Support `--output <format=path>` for `json-summary`, `graph-data`, `markdown`, and `html`. Parse `.waxrc.outputs[]` with the same format/path shape. CLI flags append to config defaults.

- [ ] **Step 3: Implement stable `json-summary`**

Read `MergedScan` / per-language facts and emit the output model above. `summary`: human lines (languages, adoption %, capped diagnostics, enabled artifact paths). `json-summary`: stable object matching `engine/schemas/scan-summary.schema.json`.

- [ ] **Step 4: Tests per stdout format and artifact output**

Run: `cd engine && cargo test -p wax-cli scan_output_format`

Expected: PASS; stdout never mixes prose into `json-summary`; requested artifacts are written to configured paths; duplicate `format=path` outputs are idempotent.

### - [ ] Task 3: Graph-data export and static HTML report

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Create: `engine/crates/wax-cli/src/scan_graph.rs` (or `wax-core` report module if shared)
- Create: `engine/crates/wax-cli/src/scan_report_html.rs`
- Create: `engine/schemas/scan-graph.schema.json`
- Create: `engine/crates/wax-cli/tests/scan_report_artifacts.rs`

- [ ] **Step 1: Build graph-data JSON from `MergedScan`**

Emit versioned `nodes[]`, `edges[]`, `metrics[]`, and `metadata` as described above. Include explicit `metadata.limits[]` when current facts cannot provide module/category/ownership rollups.

- [ ] **Step 2: Generate static HTML from summary + graph data**

Write a local, no-network report at `html` output paths. Include adoption headline, per-language cards, at least one graph, diagnostics table, raw artifact links, and visible limits.

- [ ] **Step 3: Validate artifacts**

Run: `cd engine && cargo test -p wax-cli scan_report_artifacts`

Expected: PASS; generated graph JSON validates against schema; HTML contains the headline metrics, a graph container or inline rendered chart, diagnostics, and links to generated JSON artifacts.

### - [ ] Task 4: PR / markdown scan summary and CI deltas

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Create: `engine/crates/wax-cli/src/scan_summary_md.rs` (or inline module)
- Create: `docs/ci-scan-summary.md`
- Create: `engine/crates/wax-cli/tests/scan_markdown_summary.rs`

- [ ] **Step 1: Emit markdown for stdout and artifact paths**

Emit markdown suitable for PR comments: adoption headline, per-language table, top diagnostics, generated artifact links, and explicit limitations.

- [ ] **Step 2: Add baseline comparison inputs**

Support `--baseline <path>` for a previous `json-summary` or `scan-merged.json` file. Markdown and JSON summary outputs should include deltas when a baseline is provided: adoption coverage change, resolved/candidate usage change, new error diagnostics, and removed/resolved diagnostics when computable.

- [ ] **Step 3: Document GitHub Actions usage**

Add copy-paste CI snippets that generate `json-summary`, `graph-data`, `markdown`, and `html` artifacts. Show a PR-comment workflow using markdown and an artifact upload step for HTML/JSON.

- [ ] **Step 4: Optional workflow fixture**

Add an example workflow step in `.github/workflows/` or a fixture repo only if it will not run on every PR by default.

Run: manual generate from alpha fixture; snapshot test optional.

Expected: Copy-paste CI recipe works on `ubuntu-latest`; markdown includes deltas when `--baseline` is supplied and degrades clearly when no baseline exists.

---

## Phase 3 — Documentation handoff

### - [ ] Task 5: Update product docs for UX phase

**Files:**

- Modify: `README.md`
- Modify: `docs/plans/2026-05-24-release-and-rollout-plan.md` (follow-on link only)

- [ ] **Step 1: README section “Interactive init” after Task 1 ships**

- [ ] **Step 2: README / docs for scan `--format`, `--output`, JSON, graph-data, markdown, HTML, and CI after Tasks 2–4**

- [ ] **Step 3: Tick tasks in this plan**

Expected: Users upgrading from alpha.1 see new UX without reading implementation plans.

---

## Deferred (later plans)

| Item | Target plan / phase |
|------|---------------------|
| Registry discover / draft | Separate registry authoring plan |
| Rich `wax validate` (dead entries, ambiguous matches) | After discover/draft or scan-facts validate |
| Full multi-page `wax export` site | Interpretability / reporting plan |
| Web UI | Component tracker product surface |

---

## Self-review

| Outcome | Task |
|---------|------|
| TTY `wax init` for casual users | Task 1 |
| Scriptable init unchanged | Task 1 |
| Scan output formats and JSON summary | Task 2 |
| Graph-data and static HTML report | Task 3 |
| CI markdown summary and deltas | Task 4 |
| Public docs updated | Task 5 |

---

## Review checklist for humans

1. Public alpha tag exists before starting Task 1.
2. Prompt library choice is acceptable for license and binary size.
3. `json-summary` and `graph-data` schema versioning strategy is acceptable for external CI and dashboards.
4. HTML report stays static/local and does not accidentally become a hosted web UI scope increase.
5. React remains excluded from interactive init language list until release plan promotes it in the pack index.

---

## Execution handoff

**Plan saved to:** `docs/plans/2026-05-24-post-alpha-ux-plan.md`

Start with **Task 1** after alpha.1 is tagged and install docs are live.
