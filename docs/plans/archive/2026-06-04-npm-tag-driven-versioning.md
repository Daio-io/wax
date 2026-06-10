# npm Tag-Driven Versioning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the npm wrapper publish version derive from the release tag in CI, matching the existing Rust release flow.

**Architecture:** Keep a checked-in placeholder version in `packages/cli/package.json`, then rewrite it in `release.yml` from `WAX_RELEASE_TAG` immediately before `npm publish`. Update workflow invariants and docs so tag-driven publishing is the source of truth for release versioning.

**Tech Stack:** GitHub Actions, npm wrapper tests, Ruby workflow invariant checker

---

### Task 1: Enforce tag-driven npm release versioning

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/check-release-workflow.rb`

- [x] **Step 1: Make workflow invariant checks require a release-time package version rewrite**
- [x] **Step 2: Update the release workflow to write `${WAX_RELEASE_TAG#v}` into `packages/cli/package.json` before `npm publish`**
- [x] **Step 3: Verify the npm wrapper CI runs the updated workflow invariant check**

Run: `ruby scripts/check-release-workflow.rb`
Expected: PASS

### Task 2: Switch checked-in npm metadata and docs to snapshot semantics

**Files:**
- Modify: `packages/cli/package.json`
- Modify: `README.md`
- Modify: `packages/cli/README.md`
- Modify: `docs/plans/archive/2026-05-24-release-and-rollout-plan.md`

- [x] **Step 1: Set the checked-in npm wrapper version to a snapshot placeholder**
- [x] **Step 2: Update docs to say release tags, not checked-in package metadata, control published npm versions**
- [x] **Step 3: Keep alpha install instructions unchanged**

Run: `npm --prefix packages/cli test`
Expected: PASS
