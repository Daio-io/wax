# Release and Rollout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **PR boundary:** Treat each checked **Task** as one implementation PR. Complete all steps inside a task, run its verification commands, commit the task, tick the task checkbox in this plan in the same PR, and open a PR before starting the next task. Phase checkpoints gate batches of task PRs; do not combine multiple tasks into one PR unless the human explicitly approves it.

**Goal:** Ship an **initial alpha** of `wax` that is feature-complete for the v1 spec CLI surface, downloads language packs from a hosted index over HTTPS, and installs from **GitHub Releases**, **Homebrew tap**, and optionally **npm**—without requiring a local Rust toolchain.

**Architecture:** Builds on the [Rust engine and language packs](./2026-05-16-rust-engine-language-packs-plan.md) foundation: expose `wax scan` and `wax validate` in the CLI, forward repo language config into pack subprocess requests, execute auto-install during scan when allowed, fetch the pack index over HTTPS with a baked-in default URL, then publish prebuilt `wax` and `wax-lang-*` artifacts plus a generated pack index. Install channels distribute **only the engine binary**; language packs remain on-demand downloads into `~/.wax/langs/`.

**Tech Stack:** Rust edition 2024, `wax-cli` / `wax-core`, clap, reqwest (blocking), cargo-dist or GitHub Actions release matrix, GitHub Releases, static JSON pack index, Homebrew tap formula, optional `@wax/cli` npm postinstall wrapper

**Specs (review first):**

- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — CLI surface, distribution, trust model
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — product context (registry validation scope for alpha)

**Previous phase:** [Rust engine and language packs plan](./2026-05-16-rust-engine-language-packs-plan.md) Phases 1–5.

**Roadmap:** `docs/plans/README.md` (order 2; merges in #33). Do not start release **implementation** until order 1 is `complete` in that roadmap and this plan doc is `merged`.

---

## Phase split

This plan is the **release and rollout** phase after the engine foundation. It delivers the v1 user-facing CLI surface, hosted pack index, prebuilt binaries, and install channels.

| Area | This phase delivers |
|------|---------------------|
| Scan CLI | `wax scan` with `--no-auto-install`, `--concurrency`; stdout summary (adoption %, diagnostics) |
| Validate CLI | `wax validate` — repo-only checks; warns on empty registry |
| Pack index | HTTPS fetch + default `WAX_LANG_INDEX` |
| Scan orchestration | Per-language `.waxrc` config on the wire; auto-install when allowed |
| Releases | Tagged prebuilt matrix on GitHub Releases |
| Pack index hosting | Generated `index.json` (`compose` + `basic` for alpha) |
| Install channels | curl script, Homebrew tap, optional npm wrapper |
| Versioning | Aligned semver (`0.1.0-alpha.N` until stable) |

Alpha **does not** include static site export, backend API, web UI, Swift pack, kernel plugins, or Sigstore signing (v1.1). Registry **discover** / **draft** workflows from the component tracker design remain post-alpha.

---

## Prerequisites

Plan document PRs: merge #33 (roadmap) before or with this PR so `docs/plans/README.md` exists on `main`.

**Order 1 gate (verify in repo; do not rely on stale plan checkboxes alone):**

- [ ] [`docs/plans/README.md`](./README.md) on `main` shows order 1 implementation status `complete` (update the roadmap when foundation work is actually done).
- [ ] Foundation **code** is present and tests pass on `main`:

```bash
cd engine
cargo test --workspace
cargo test -p wax-lang-basic
cargo test -p wax-lang-compose
```

Required on `main` before release Task 1: `wax-lang-basic` and `wax-lang-compose` crates (foundation Tasks 12b, 12c), `Engine::scan_repo`, language install path (Tasks 8–11). Foundation **documentation-only** tasks (e.g. Task 16 release sketch) do not block release implementation.

- [ ] This plan reviewed; alpha scope (below) agreed with maintainers.
- [ ] GitHub repo has **Releases** enabled and a decision on org/domain for default index URL (e.g. `https://github.com/<org>/wax/releases/download/...` for alpha, custom domain later).

## Milestones

| Milestone | When | What ships |
|-----------|------|------------|
| **Release tag** | After Phase 3 Tasks 8–11 | Prebuilt `wax` + language-pack archives, published `index.json`, semver tag on GitHub Releases. Maintainer smoke only. |
| **Public alpha** | After Phase 4 Tasks 12–13 (and Phase 5 Task 15 docs) | End users can install via curl or Homebrew without building from source; README getting-started path works. npm (Task 14) is optional. |

Do not announce **public alpha** until install channels (at least curl + Homebrew) and Phase 5 getting-started docs land. A Phase 3 release tag is for validation and index smoke, not end-user onboarding.

## Alpha scope (feature-complete bar)

A new user on macOS or Linux (supported triple) can:

1. Install `wax` via curl or Homebrew tap (npm optional per Task 14).
2. Run `wax init --non-interactive --language compose` without setting `WAX_LANG_INDEX` manually.
3. Populate `design-system/registry.json` with canonical components (init scaffolds empty `components`; discover/draft is post-alpha).
4. Run `wax scan` and receive `.wax/out/scan-merged.json` with real parser-backed facts plus a stdout summary (adoption %, key diagnostics).
5. Run `wax validate` in CI without global install state (warns if registry is still empty).
6. Run `wax scan --no-auto-install` in CI with a committed `wax.lock.json`.
7. Run `wax language doctor` and see effective index URL, lock pins, and install status.

## Execution model

- One task = one branch, one focused commit series, one PR.
- Task PR titles: `Task N: <short description> (release plan)`.
- Each task PR ticks its task heading and completed step checkboxes in this plan.
- Phase checkpoints are review gates; do not start Phase 3 until Phase 1–2 alpha CLI/registry tasks are merged.

## File structure (additions)

```text
engine/
  dist.toml                    # cargo-dist config (Task 9), or release/ scripts
  schemas/
    waxrc.schema.json          # Task 15 — editor validation for .waxrc
  fixtures/registry/
    alpha-index.json           # compose + basic entries (Task 7)
docs/plans/
  2026-05-24-release-and-rollout-plan.md
.github/workflows/
  release.yml                  # tag-triggered release (Task 10)
  alpha_smoke.yml              # optional post-release smoke (Task 16)
scripts/
  install.sh                   # curl installer (Task 12)
  generate-pack-index.sh       # build index.json from release manifest (Task 11)
homebrew/
  Formula/wax.rb               # tap formula (Task 13)
packages/
  cli/                         # @wax/cli npm wrapper (Task 14, optional)
CHANGELOG.md                   # alpha release notes (Task 15)
```

Generated / hosted (not committed):

```text
GitHub Release assets: wax-*, wax-lang-* per triple
Release-attached or gh-pages index.json
```

---

## Phase 1 — Alpha CLI and scan orchestration

**Execution checkpoint:** Phase 1 completes the spec v1 **scan path**. Do not tag alpha releases until Phase 2 (HTTPS index + default URL) is also done—otherwise `wax init`/`wax scan` auto-install cannot reach a real index.

### - [x] Task 1: Forward `.waxrc` language config into scan requests

**Files:**

- Modify: `engine/crates/wax-core/src/lib.rs` (job construction loop and `run_scan_job`)
- Modify: `engine/crates/wax-core/tests/scan_resolve.rs`
- Modify: `engine/crates/wax-core/tests/scan_output.rs`

- [x] **Step 1: Build per-language config map when loading `.waxrc`**

For each enabled `LanguageEntry`, copy `extra` (all keys beyond `id` / `enabled`) into a `BTreeMap<LanguageId, serde_json::Map<...>>` (or extend `ScanJob` with a `config` field).

- [x] **Step 2: Thread config through job construction**

When building each `ScanJob` in `run_scan_jobs`, attach the map entry for that `language_id` so jobs carry config before execution.

- [x] **Step 3: Pass config in `run_scan_job`**

Replace `config: serde_json::Map::new()` with the config from the job (or lookup for `job.language_id`).

- [x] **Step 4: Add integration test asserting config reaches stub pack**

Extend `scan_resolve` (or subprocess protocol test) to assert `design_system_registry` and `roots` appear on the wire when present in `.waxrc`.

Run: `cd engine && cargo test -p wax-core scan_resolve scan_output subprocess_protocol`

Expected: PASS; configured `.waxrc` keys appear in `ScanRequest.config`.

### - [x] Task 2: Auto-install execution during scan

**Files:**

- Modify: `engine/crates/wax-core/src/lib.rs`
- Modify: `engine/crates/wax-core/src/auto_install.rs` (if helper extraction needed)
- Create: `engine/crates/wax-core/tests/scan_auto_install.rs`
- Modify: `engine/crates/wax-core/src/install.rs` (reuse from scan path if needed)

- [x] **Step 1: Extend `ScanOptions` with `allow_auto_install: bool`**

Default `true` for local scans; CLI will set `false` for `--no-auto-install`.

- [x] **Step 2: When policy returns `needs_install` and auto-install is allowed, execute install plans**

Call existing `install_language` (or shared helper used by CLI) for each plan, refresh global state, then continue scan—do not return `AutoInstallRequired` when installs succeed.

- [x] **Step 3: When auto-install is disabled, preserve current fail-fast behavior**

Return typed error instructing user to run `wax language install` or enable auto-install.

- [x] **Step 4: Test happy path and `--no-auto-install` equivalent**

Run: `cd engine && cargo test -p wax-core scan_auto_install auto_install_policy`

Expected: PASS.

### - [x] Task 3: `wax scan` CLI command

**Files:**

- Create: `engine/crates/wax-cli/src/commands/scan.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`
- Create: `engine/crates/wax-cli/tests/scan_command.rs`

- [x] **Step 1: Add `Scan` subcommand with flags**

Support:

- `--repo-root` (default `.`)
- `--no-auto-install`
- `--concurrency=N`

- [x] **Step 2: Wire to `Engine::scan_repo_with_options`**

Map flags to `ScanOptions { scan_concurrency, allow_auto_install }`.

- [x] **Step 3: Print scan success summary on stdout**

Minimum alpha summary (human-readable, not only a JSON path):

- Path to `.wax/out/scan-merged.json`
- Enabled language ids and per-language scan status (`complete` / `partial` / `failed`)
- Per-language adoption coverage when present (`metrics.adoption_coverage_ratio`, formatted as percentage)
- Up to a small capped number of error-severity diagnostics across languages (code + message)

Full `wax export` / HTML reports remain deferred; this stdout block is the minimum interpretability bar for alpha.

- [x] **Step 4: CLI integration test**

Spawn `wax` binary with fixture repo + `file://` index (via env) and assert exit 0 and output file exists.

Run: `cd engine && cargo test -p wax-cli scan_command && cargo test -p wax-cli wax_binary`

Expected: PASS; `wax --help` lists `scan`.

### - [x] Task 4: `wax validate` CLI command

**Files:**

- Create: `engine/crates/wax-core/src/validate.rs`
- Modify: `engine/crates/wax-core/src/lib.rs`
- Create: `engine/crates/wax-core/tests/validate_repo.rs`
- Create: `engine/crates/wax-cli/src/commands/validate.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`
- Create: `engine/crates/wax-cli/tests/validate_command.rs`

- [x] **Step 1: Define repo-only validation rules (alpha minimum)**

Validate without reading `~/.wax/`:

- `.waxrc` loads and `schema_version` is supported
- `wax.lock.json` present when any language is enabled (per spec)
- Each enabled language has `design_system_registry` path that exists and parses as JSON with supported `schema_version`
- Registry file paths are repo-relative and non-empty where required
- No duplicate enabled language ids
- **Warning** (exit 0, printed to stderr): `design_system_registry` parses but `components` is missing or empty — adoption metrics will be empty until the registry is populated manually (registry discover/draft is post-alpha)

Defer registry **usage cross-checks** (dead entries, ambiguous matches) to post-alpha unless trivial to add from existing pack config validators.

- [x] **Step 2: Implement `validate_repo(repo_root) -> Result<ValidateReport, ValidateError>`**

Structured errors with field paths for CI.

- [x] **Step 3: Add `wax validate` CLI**

`--repo-root` (default `.`); exit code 1 on validation failure; print human-readable issues to stderr.

- [x] **Step 4: Tests for valid fixture repo and common failure modes**

Include: valid repo passes; empty `components: []` emits warning and exit 0.

Run: `cd engine && cargo test -p wax-core validate_repo && cargo test -p wax-cli validate_command`

Expected: PASS; `wax --help` lists `validate`.

---

## Phase 2 — Remote pack index and defaults

**Execution checkpoint:** After Phase 2, `wax init` and `wax language install` work against HTTPS index URLs without manual `file://` setup.

### - [x] Task 5: HTTPS pack index fetch

**Files:**

- Modify: `engine/crates/wax-core/src/registry.rs`
- Modify: `engine/crates/wax-core/tests/` (registry tests)
- Modify: `engine/crates/wax-core/Cargo.toml` (if reqwest features needed)

- [x] **Step 1: Extend `fetch_pack_index` to support `https://` and `http://`**

Reuse the same blocking HTTP client pattern as `install.rs`. Keep `file://` for unit tests.

- [x] **Step 2: Add error variants for HTTP status, timeout, and malformed remote JSON**

Preserve typed errors; no bare strings at crate boundary.

- [x] **Step 3: Unit tests**

Keep existing `file://` tests; add test with mock HTTP server or recorded fixture (prefer local mock via `httptest` / `wiremock` if already in workspace—otherwise minimal integration test behind `file://` plus one https test against example.com invalid JSON disabled; use `httpmock` crate sparingly).

Pragmatic alpha approach: use `file://` in unit tests and an integration test in `wax-cli` that serves index via `file://` converted path; add one CI job with real HTTPS against GitHub raw URL of committed `alpha-index.json` after Task 11.

**Phase 2 completion criterion:** `file://` tests pass in CI; real HTTPS against the published index is validated manually after Task 11 (automated in Task 16).

Run: `cd engine && cargo test -p wax-core registry`

Expected: PASS.

### - [x] Task 6: Default `WAX_LANG_INDEX` and doctor visibility

**Files:**

- Create: `engine/crates/wax-core/src/defaults.rs` (or `constants.rs`)
- Modify: `engine/crates/wax-core/src/lib.rs`
- Modify: `engine/crates/wax-cli/src/commands/language.rs`
- Modify: `engine/crates/wax-cli/tests/` (doctor output test)

- [x] **Step 1: Define `DEFAULT_WAX_LANG_INDEX` constant**

Alpha default: branch-backed raw `index.json` URL for this repo: `https://raw.githubusercontent.com/Daio-io/wax/gh-pages/index.json`. The first successful release workflow bootstraps `gh-pages`; before that first tag completes, the default URL may 404 and maintainers should use `--registry` / `WAX_LANG_INDEX` for local dry-runs. Override remains via `WAX_LANG_INDEX` env and `--registry`.

- [x] **Step 2: Update `resolve_registry_url` to fall back to default**

When neither `--registry` nor `WAX_LANG_INDEX` is set, use `DEFAULT_WAX_LANG_INDEX`.

- [x] **Step 3: `wax language doctor` prints effective index URL**

Include default vs override in output.

- [x] **Step 4: Tests for default resolution and override precedence**

Run: `cd engine && cargo test -p wax-cli` (doctor tests)

Expected: PASS.

### - [x] Task 7: First-party alpha pack index fixture

**Files:**

- Create: `engine/fixtures/registry/alpha-index.json`
- Modify: `engine/fixtures/registry/official-manifest.json` (align ids with `compose` and `basic`)
- Modify: `engine/crates/wax-core/tests/install_language.rs` (optional cross-reference)

**Alpha index scope:** List **`compose`** and **`basic`** only. Do **not** publish `react` in the alpha index until `wax-lang-react` has production extraction (stub today). Task 11 generator must match this list; README getting started must not feature `wax init --language react`.

- [x] **Step 1: Author index listing `compose` and `basic`**

Each entry: `id`, `version`, `api_version`, `targets` map with release URLs and sha256 filled by Task 11 generator (placeholders OK in fixture until first tag).

- [x] **Step 2: Document index schema in spec or plan comment block**

Match [language packs spec § Distribution](../specs/2026-05-16-language-packs-and-distribution.md). Note `react` deferred from alpha index.

Plan note: `engine/fixtures/registry/alpha-index.json` follows the v1 Distribution shape from the language packs spec: a top-level array of pack entries, each with `id`, `version`, `api_version`, and a `targets` object keyed by Rust target triple. Each target contains a release asset `url` and artifact `sha256`; the fixture uses placeholder zero digests until Task 11 generates the first release index. Alpha publishes only `compose` and `basic`; `react` is deferred until production extraction is ready.

- [x] **Step 3: Wire one CLI integration test to load `file://` copy of alpha index**

Run: `cd engine && cargo test -p wax-cli`

Expected: PASS.

---

## Phase 3 — Versioning and prebuilt releases

**Execution checkpoint:** Create the first **release tag** (`v0.1.0-alpha.1` or agreed name) only after Tasks 8–11 land and manual smoke on one triple succeeds. This is not **public alpha** (see Milestones)—install channels come in Phase 4.

### - [x] Task 8: Workspace semver alignment

**Files:**

- Modify: `engine/Cargo.toml` (workspace.package.version if using inheritance)
- Modify: `engine/crates/*/Cargo.toml`
- Create: `CHANGELOG.md`

- [x] **Step 1: Set workspace version to `0.1.0-alpha.1` (or agreed alpha semver)**

All publishable crates align: `wax-cli`, `wax-core`, `wax-contract`, `wax-lang-api`, `wax-lang-basic`, `wax-lang-compose`, `wax-lang-react`.

- [x] **Step 2: Ensure `wax.lock.json` example / init writes matching `wax_version`**

- [x] **Step 3: Add CHANGELOG alpha section**

Run: `cd engine && cargo build --workspace`

Expected: PASS.

### - [x] Task 9: Release packaging configuration

**Files:**

- Create: `engine/dist.toml` (cargo-dist) **or** `scripts/build-release.sh` + `release/manifest.template.json`
- Modify: `engine/Cargo.toml` (dist metadata)

- [x] **Step 1: Choose cargo-dist vs hand-rolled matrix**

Used the documented hand-rolled matrix fallback (`scripts/build-release.sh`) because `cargo-dist` install currently fails on a yanked upstream dependency (`color-backtrace = 0.7.3`) in the published `cargo-dist` crate.

- [x] **Step 2: Configure artifacts for v1 triple matrix**

**Required** for alpha (must match Task 7 / 11 index entries): `wax`, `wax-lang-compose`, `wax-lang-basic`.

**Not in alpha index:** `wax-lang-react` may still build in the matrix for contributors, but do **not** list `react` in `index.json` until production extraction is ready.

Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`.

- [x] **Step 3: Local dry-run build for host triple**

Run: documented command (e.g. `cd engine && cargo dist build --artifacts=local` or `./scripts/build-release.sh`)

Expected: tar.gz containing expected binary names.

### - [x] Task 10: GitHub Actions release workflow

**Files:**

- Create: `.github/workflows/release.yml`
- Modify: `.github/workflows/build_engine.yml` (optional: skip duplicate work)

- [x] **Step 1: Trigger on version tags `v*`**

- [x] **Step 2: Build matrix and upload GitHub Release assets**

Include SHA256 checksums file per asset.

- [x] **Step 3: Smoke step: download host artifact and run `wax --version`**

Run: merge workflow; maintainers tag `v0.1.0-alpha.1` to validate.

Expected: Release page shows **12** archives (3 alpha-index binaries × 4 triples) + checksums. Every id listed in `index.json` must have a corresponding uploaded archive.

### - [x] Task 11: Pack index generation and publication

**Files:**

- Create: `scripts/generate-pack-index.sh` (or Rust bin in `engine/tools/`)
- Modify: `.github/workflows/release.yml`

- [x] **Step 1: Script reads release manifest (URLs + sha256 per triple) and emits `index.json`**

Emit entries for **`compose` and `basic` only** in alpha (see Task 7). Exclude `react` until a follow-up release plan task promotes it.

- [x] **Step 2: Attach `index.json` to GitHub Release and/or commit to `gh-pages`**

Default index URL from Task 6 is the `gh-pages` raw URL. The first alpha tag must create or update `gh-pages/index.json` before the post-release verification job runs.

- [x] **Step 3: Post-release job verifies `fetch_pack_index` against published URL**

Run: manual on first alpha tag; automate in Task 16. The release workflow retries this verification to allow raw GitHub content propagation, and it must reject stale `gh-pages` content by checking the fetched index version and artifact URLs against the current release tag.

Expected: `wax language install compose` downloads from release URL on supported host.

---

## Phase 4 — Install channels

**Execution checkpoint:** Required for **public alpha**. May start once Task 10 release workflow is merged; curl/Homebrew (Tasks 12–13) should land before announcing public alpha. Task 11 (published index) can run in parallel with Task 12 once the first release tag exists.

### - [x] Task 12: curl install script

**Files:**

- Create: `scripts/install.sh`
- Modify: `README.md`

- [x] **Step 1: Detect OS/arch and map to Rust triple**

- [x] **Step 2: Download `wax` release archive, verify sha256, install to `/usr/local/bin` or `~/.wax/bin`**

Language packs **not** bundled; print next steps (`wax init`, `wax language install`).

- [x] **Step 3: Document one-liner in README**

Run: `./scripts/install.sh --dry-run` or manual test against alpha release.

Expected: `wax --help` works after install.

### - [ ] Task 13: Homebrew tap formula

**Files:**

- Create: `homebrew/Formula/wax.rb`
- Modify: `README.md`

- [ ] **Step 1: Formula installs `wax` binary only from GitHub Release**

Use versioned URL + sha256 from release.
Blocked until the first published `wax` GitHub Release provides macOS archive checksums.

- [x] **Step 2: Document tap install**

```bash
brew tap <org>/wax
brew install wax
```

Status: README now marks Homebrew as pending until a dedicated tap repository exists (expected: `<org>/homebrew-wax`) and checksums are published.

- [x] **Step 3: Caveats section explains language pack download on first use**

Optional: automate formula bump in release workflow (post-alpha).

Expected: `brew install` succeeds on macOS for at least one arch.

### - [x] Task 14: npm `@wax/cli` wrapper (optional for alpha)

**Files:**

- Create: `packages/cli/package.json`
- Create: `packages/cli/postinstall.js`
- Create: `packages/cli/run.js`

**Decision:** Task 14 is **optional for public alpha**. If deferred to alpha+1, README and Task 15 must lead with curl + Homebrew and state npm timeline explicitly.

- [x] **Step 1: Package downloads host `wax` binary from GitHub Releases on postinstall**

Mirror patterns from esbuild/turbo; verify sha256.

- [x] **Step 2: Expose `wax` bin in npm**

- [x] **Step 3: Document `npm install -g @wax/cli` in README**

Mark optional in alpha if schedule tight; move to post-alpha if needed.

Run: `npm pack` dry run; manual install test.

Expected: `npx @wax/cli --help` works.

---

## Phase 5 — Alpha documentation and verification

### - [x] Task 15: Alpha getting started documentation

Task 15 covers documentation readiness only. Homebrew install availability remains gated by Task 13.

**Files:**

- Modify: `README.md`
- Create: `engine/schemas/waxrc.schema.json` (or `engine/crates/wax-contract/schemas/waxrc.schema.json`)
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md` (mark distribution § implementation status if accurate)
- Modify: `docs/plans/2026-05-16-rust-engine-language-packs-plan.md` (link to this plan)

- [x] **Step 1: Replace cargo-only install section with alpha paths**

Primary: curl now. Homebrew remains pending until Task 13 publishes the tap and release checksums. npm only if Task 14 shipped; otherwise document expected npm timeline (e.g. alpha+1). Keep `cargo install --path` for contributors.

- [x] **Step 2: Document end-to-end flow (Compose-only for alpha)**

`install → wax init --non-interactive --language compose → populate design-system/registry.json → wax validate → wax scan → inspect .wax/out/`

State clearly: **`wax init` scaffolds an empty registry**; users must add canonical components manually for meaningful adoption metrics until registry discover/draft ships (post-alpha). Do not document `wax init --language react` in getting started until the react pack is production-ready.

- [x] **Step 3: Commit JSON Schema for `.waxrc`**

Enable editor validation/autocomplete; reference schema path from README.

- [x] **Step 4: Document monorepo / multi-repo usage**

One `.waxrc` + `wax.lock.json` per repo; shared pack index via default `WAX_LANG_INDEX`; language packs installed once globally under `~/.wax/langs/`.

- [x] **Step 5: Document CI recipe**

Commit `wax.lock.json`; run `wax validate` and `wax scan --no-auto-install`.

- [x] **Step 6: Tick completed tasks in this plan**

Run: `rg -n "cargo install --path" README.md` — ensure contributor path remains.

Expected: New user can follow README without reading plans.

### - [ ] Task 16: Alpha smoke verification

**Files:**

- Create: `.github/workflows/alpha_smoke.yml` (optional scheduled or post-release)
- Create: `engine/crates/wax-cli/tests/alpha_smoke.rs` (ignored test or feature-gated)

- [ ] **Step 1: Script downloads released `wax` for runner triple**

- [ ] **Step 2: Run init (file or https index), validate, scan against small fixture**

- [ ] **Step 3: Assert `scan-merged.json` schema_version and non-scaffold compose counts where applicable**

Run: workflow_dispatch after first alpha tag.

Expected: PASS on ubuntu-latest and macos-latest.

### - [x] Task 17: Cross-plan documentation links

**Files:**

- Modify: `docs/plans/2026-05-16-rust-engine-language-packs-plan.md`

- [x] **Step 1: Add "Next phase" section linking to this plan**

- [x] **Step 2: Point npm meta-installer deferred item at release plan Task 14**

- [x] **Step 3: Extend self-review table with release-plan task references for distribution and scan/validate CLI**

Completed in the PR that introduced this plan. Re-verify links remain accurate after Phase 5 Task 15 README updates.

---

## Follow-on plans

After **public alpha** ships, implement **order 3** in `docs/plans/README.md`: **Post-alpha UX plan** (`docs/plans/2026-05-24-post-alpha-ux-plan.md` — separate plan doc PR #34; merge after this PR so the file exists on `main` before adding markdown links).

---

## Deferred (post-alpha)

Each item includes a **target** so follow-up work can be scheduled without reopening alpha scope.

| Item | Target |
|------|--------|
| Static site export (`wax export`) | Interpretability plan — before full web UI |
| PR diff / markdown scan summaries for CI | Post-alpha UX plan Task 3 (#34) |
| Registry discover / draft CLI workflows | Separate plan after alpha (component tracker design) |
| Rich `wax validate` (dead entries, ambiguous matches) | After discover/draft or using scan facts |
| Interactive `wax init` TTY wizard | Post-alpha UX plan order 3 (#34) |
| Richer `wax scan` output formats | Post-alpha UX plan order 3 (#34) |
| Swift language pack | Dedicated parser spike plan |
| WASM language packs | Future platform plan |
| Kernel **plugins** | Future ADR |
| Backend API and web UI | Component tracker design — post-alpha product surface |
| Sigstore/cosign signing (v1.1) | Security release track |
| Windows prebuilt matrix | Platform expansion after macOS/Linux alpha stable |
| homebrew-core submission | After tap usage justifies core PR |
| In-process language packs / daemon NDJSON mode | Performance plan |
| `react` in public pack index + getting started | When `wax-lang-react` production extraction lands |
| npm `@wax/cli` if skipped in alpha | alpha+1 install channel (Task 14) |
| Align engine default scan timeout with spec (`WAX_SCAN_TIMEOUT_SECS` / 10 minutes) | Small engine fix when CI timeouts bite |

---

## Self-review (alpha spec coverage)

| Spec / product requirement | Plan task |
|----------------------------|-----------|
| `wax scan` | Task 3 |
| `wax scan --no-auto-install` | Tasks 2, 3 |
| `wax scan --concurrency=N` | Task 3 |
| `wax validate` (repo-only) | Task 4 |
| Forward `.waxrc` config to packs | Task 1 |
| Auto-install on scan | Task 2 |
| `wax init` without manual `WAX_LANG_INDEX` | Tasks 6, 7, 11 |
| `wax language install` over HTTPS artifacts | Foundation 8b; Tasks 5, 11 |
| Lockfile digest drift refusal | Foundation 8b; Task 2 (scan path) |
| HTTPS pack index | Task 5 |
| Default `WAX_LANG_INDEX` | Task 6 |
| Hosted pack index | Tasks 7, 11 |
| Prebuilt release matrix | Tasks 9, 10 |
| `wax-lang-basic` in release matrix (required for alpha index) | Tasks 9, 10, 11 |
| GitHub Releases install | Tasks 10, 12 |
| Homebrew | Task 13 |
| npm wrapper | Task 14 (optional) |
| CI: validate + scan --no-auto-install | Tasks 4, 15 |
| `wax language doctor` shows index URL | Task 6 |
| Semver / changelog | Task 8 |
| Alpha smoke | Task 16 |
| Scan stdout summary (adoption %, diagnostics) | Task 3 |
| Empty registry warning on validate | Task 4 |
| `.waxrc` JSON Schema + monorepo docs | Task 15 |
| Alpha index: `compose` + `basic` only | Tasks 7, 11 |
| Public alpha install (curl + Homebrew) | Tasks 12, 13, 15 |

---

## Review checklist for humans

Before starting implementation, confirm:

1. Alpha tag naming: `v0.1.0-alpha.1` vs `v0.1.0`.
2. Default index hosting: GitHub Release asset vs `gh-pages` vs custom domain.
3. **npm for alpha vs alpha+1:** Is Task 14 required before public alpha? If deferred, README leads with curl/Homebrew and states npm timeline.
4. Org/name for Homebrew tap and npm scope (`@wax/cli` availability).
5. Auto-install executes in **engine** (Task 2) vs CLI-only orchestration—Task 2 recommendation keeps `wax scan` behavior consistent for library callers.
6. `wax validate` alpha scope is intentionally minimal; rich usage analysis waits for discover/draft or scan-facts-based validate.
7. **Getting started uses Compose only** until `wax-lang-react` is production-ready; alpha index lists `compose` + `basic` only.
8. **Minimum scan stdout summary** is defined in Task 3 (path, languages, adoption %, capped diagnostics)—not JSON path alone.
9. **Empty registry:** documented in Task 15; Task 4 warns on `components: []` without failing validate.
10. **Post-alpha UX** is order 3 in `docs/plans/README.md`; plan doc is PR #34 (no markdown link to that file in this plan until #34 merges).
11. **`wax-lang-basic` is required** in the release matrix whenever it appears in the alpha index (`compose` + `basic` + `wax`).

---

## Execution handoff

**Plan saved to:**

- `docs/plans/2026-05-24-release-and-rollout-plan.md`

**Two execution options:**

1. **Subagent-driven (recommended)** — one task per subagent, one PR per task, review between task PRs
2. **Inline** — execute one task at a time in-session, still committing and opening one PR per task

Start with **Task 1** (config forwarding), then **Task 2** (auto-install), then **Task 3** (`wax scan` CLI), then **Task 4** (`wax validate`)—Phase 1 establishes the end-to-end scan path before release wiring in Phases 2–3.
