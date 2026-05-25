# Post-Alpha UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PR boundary:** Treat each checked **Task** as one implementation PR. Complete all steps inside a task, run its verification commands, commit the task, tick the task checkbox in this plan in the same PR, and open a PR before starting the next task.

**Goal:** Improve day-one ergonomics after the initial alpha ships—guided onboarding, richer scan feedback, and CI-friendly output—without expanding alpha scope or building registry discover/draft.

**Architecture:** Builds on [Release and rollout](./2026-05-24-release-and-rollout-plan.md) once public alpha is live (`wax scan` summary, `wax validate`, install channels). CLI-only UX layers; no changes to the `ScanFacts` contract unless a task explicitly requires new optional fields.

**Tech Stack:** Rust `wax-cli`, clap, terminal prompts (e.g. `dialoguer` or `inquire`—pick one crate in Task 1), markdown/text formatters for CI summaries

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
| Richer `wax scan` output formats for humans and CI | Static site export / web UI |
| PR-friendly markdown scan summaries (stretch) | Changing alpha lockfile or pack index policy |
| Preserve `--non-interactive` for all scripted paths | Full `wax export` HTML reports |

Alpha already delivers a minimum stdout scan summary (release plan Task 3). This plan extends interpretability and onboarding—not the first path to a JSON file.

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

## Phase 2 — Scan and CI output

### - [ ] Task 2: `wax scan` output formats

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Create: `engine/crates/wax-cli/tests/scan_output_format.rs`

- [ ] **Step 1: Add `--format` flag**

Suggested values: `summary` (default, release plan Task 3 behavior), `quiet` (path only), `json-summary` (single JSON object on stdout for scripts).

- [ ] **Step 2: Implement formatters reading `MergedScan` / per-language facts**

`summary`: human lines (languages, adoption %, capped diagnostics). `json-summary`: stable schema documented in plan or `engine/schemas/scan-summary.schema.json`.

- [ ] **Step 3: Tests per format**

Run: `cd engine && cargo test -p wax-cli scan_output_format`

Expected: PASS.

### - [ ] Task 3: PR / markdown scan summary for CI (stretch)

**Files:**

- Modify: `engine/crates/wax-cli/src/commands/scan.rs`
- Create: `engine/crates/wax-cli/src/scan_summary_md.rs` (or inline module)

- [ ] **Step 1: Add `--format=markdown` or `wax scan --write-summary=path.md`**

Emit markdown section suitable for PR comments: adoption table per language, top diagnostics, link to artifact path.

- [ ] **Step 2: Document GitHub Actions snippet in README or `docs/ci-scan-summary.md`**

- [ ] **Step 3: Example workflow step in `.github/workflows/` (optional fixture repo)**

Run: manual generate from alpha fixture; snapshot test optional.

Expected: Copy-paste CI recipe works on `ubuntu-latest`.

---

## Phase 3 — Documentation handoff

### - [ ] Task 4: Update product docs for UX phase

**Files:**

- Modify: `README.md`
- Modify: `docs/plans/2026-05-24-release-and-rollout-plan.md` (follow-on link only)

- [ ] **Step 1: README section “Interactive init” after Task 1 ships**

- [ ] **Step 2: README / docs for scan `--format` and CI markdown after Tasks 2–3**

- [ ] **Step 3: Tick tasks in this plan**

Expected: Users upgrading from alpha.1 see new UX without reading implementation plans.

---

## Deferred (later plans)

| Item | Target plan / phase |
|------|---------------------|
| Registry discover / draft | Separate registry authoring plan |
| Rich `wax validate` (dead entries, ambiguous matches) | After discover/draft or scan-facts validate |
| Static site export (`wax export`) | Interpretability / reporting plan |
| Web UI | Component tracker product surface |

---

## Self-review

| Outcome | Task |
|---------|------|
| TTY `wax init` for casual users | Task 1 |
| Scriptable init unchanged | Task 1 |
| Scan output formats | Task 2 |
| CI markdown summary | Task 3 (stretch) |
| Public docs updated | Task 4 |

---

## Review checklist for humans

1. Public alpha tag exists before starting Task 1.
2. Prompt library choice is acceptable for license and binary size.
3. `json-summary` schema versioning strategy if external CI depends on it.
4. Markdown CI format is optional stretch—do not block Tasks 1–2 on Task 3.
5. React remains excluded from interactive init language list until release plan promotes it in the pack index.

---

## Execution handoff

**Plan saved to:** `docs/plans/2026-05-24-post-alpha-ux-plan.md`

Start with **Task 1** after alpha.1 is tagged and install docs are live.
