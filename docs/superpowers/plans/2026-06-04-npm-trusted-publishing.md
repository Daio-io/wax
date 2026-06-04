# npm Trusted Publishing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the npm wrapper to `@waxhq/wax` and wire the existing tag-based release workflow to publish it via npm trusted publishing.

**Architecture:** Reuse the current `release.yml` pipeline as the single source of truth for release versioning. Keep the wrapper package in `packages/cli`, rename user-facing npm references, and add a post-release npm publish job with an explicit version guard so npm publication stays aligned with GitHub release assets.

**Tech Stack:** GitHub Actions, npm trusted publishing (OIDC), Node.js, existing npm wrapper tests

---

### Task 1: Rename the npm package references

**Files:**
- Modify: `packages/cli/package.json`
- Modify: `packages/cli/README.md`
- Modify: `packages/cli/postinstall.js`
- Modify: `packages/cli/run.js`
- Modify: `README.md`

- [x] **Step 1: Update package metadata and docs to `@waxhq/wax`**
- [x] **Step 2: Update install/reinstall messaging to `@waxhq/wax`**
- [x] **Step 3: Run npm wrapper tests**

Run: `npm --prefix packages/cli test`
Expected: PASS

### Task 2: Publish from the existing release workflow

**Files:**
- Modify: `.github/workflows/release.yml`

- [x] **Step 1: Add a version guard comparing the release tag to `packages/cli/package.json`**
- [x] **Step 2: Add a trusted-publishing npm job after GitHub Release publish**
- [x] **Step 3: Use GitHub-hosted Ubuntu, `id-token: write`, and a modern Node version for npm trusted publishing**

Run: `python - <<'PY'
import yaml
yaml.safe_load(open('.github/workflows/release.yml'))
print('ok')
PY`
Expected: `ok`

### Task 3: Verify publish surface

**Files:**
- Modify: `packages/cli/package.json` (if package contents need adjustment)

- [x] **Step 1: Check packed contents and metadata**
- [x] **Step 2: Verify package/version alignment assumptions**

Run: `npm --prefix packages/cli pack --dry-run --json`
Expected: JSON output listing the expected package files
