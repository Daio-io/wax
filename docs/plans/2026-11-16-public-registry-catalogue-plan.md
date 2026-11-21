# Public registry catalogue implementation plan

**Status:** Phase 1 and Phase 3a complete. Phase 4 consumption implemented; remaining pilot and Phase 3b evidence gates are open, as recorded below. Authorized as a THE-889 extraction
that runs alongside the active Phase 2B plan without displacing it.

**Goal:** Let ordinary Git design-system bundles live in repository
subfolders. A bundle's publisher definition and every declared language
registry resolve together at one immutable Git commit, while root Git
sources, existing config/lock state, and schema 1/2 lockfiles keep working
unchanged.

## Task 1: [x] Catalogue contract, Git bundle plumbing, and lock v3

- [x] Add the `wax-contract` catalogue contract (`Catalog`, `CatalogEntry`,
  `ArtifactDigest`, `RegistryDefinitionPin`, `UpstreamPin`,
  `RegistryBundlePin`) with bounded, sorted, typed validation and no
  self-referential catalogue commit.
- [x] Add `schemas/registry-catalog.schema.json` and round-trip/rejection
  contract fixtures, including the two-language bundle fixture.
- [x] Factor Git commit resolution and regular-blob reads in `registry_git.rs`
  behind a `fetch_git_files` seam that resolves a ref once and then reads
  every requested path as a typed, path-safety-checked blob at that commit;
  keep the existing root registry wrappers working unchanged.
- [x] Extend `LanguageRegistrySource::Git` / `RegistryRaw::Git` and the
  `waxrc.schema.json` Git variant with optional `subdir` and `system` fields;
  `system` requires `subdir`, and the schema pattern rejects empty, absolute,
  traversal, backslash, and option-like path segments.
- [x] Add `RegistrySourceSpec::GitSystem` (serde type `git_system`) that reads
  `<subdir>/.wax/wax.config.json` through the shared `load_waxrc` parser,
  selects exactly `design_systems[system_id]`, and validates every declared
  language registry as the conventional relative
  `.wax/registries/<language>.json` path.
- [x] Resolve every bundle blob (definition, registries, optional provenance)
  before promoting any selected-source record; reuse `RegistryDocument`
  validation and cache artifacts by digest.
- [x] Add `RegistryBundlePin` tracking repository, full commit, subdir, system
  id, definition/provenance digests, and per-language snapshot refs; keep
  global remembered-registry state backward-readable with serde defaults.
- [x] Extend `LockedRegistry` with optional `subdir` and `definition` fields;
  bump the lockfile writer to schema version 3 while continuing to read
  schema 1 and 2 unchanged, and reject malformed partial bundle metadata
  offline.
- [x] Share one Git source identity helper (`git_source_identity`) across
  cache, lock, lookup, and offline validation so legacy root identity
  (`git:<url>#<ref>`) and nested identity
  (`git-system:<sha256(canonical JSON)>`) never collide across systems or
  subdirs.
- [x] Wire `registry_source.rs`, `registry_memory.rs`, `registry_lookup.rs`,
  `sync.rs`, and `validate.rs` to honor the full commit plus definition and
  registry hashes; keep `wax validate` repo-local with no global state or
  network access, and fail bundle resolution before persistence when
  definitions across languages disagree on commit or hash.
- [x] Add fixture and public-seam coverage: `wax-core/tests/registry_bundles.rs`
  (two subdirs, two languages, moving tag, historical reads, malformed
  definition, missing language/blob, traversal/symlink rejection, hash
  mismatch, warm/cold offline, failed state save preserving prior selection),
  plus `config_v2.rs`, `lockfile_load.rs`, and `validate_repo.rs` coverage for
  schema 1/2 compatibility, v3 round-trips, and rejected partial/mixed bundle
  pins.
- [x] Run the focused catalogue, registry, lockfile, config, and validate
  checks plus the full workspace `cargo fmt` / `cargo test` / `cargo clippy`
  gates.

## Task 2: [x] Docs and roadmap

- [x] Record the rejected main-repository and npm-package distribution options
  and the selected Git-subfolder bundle contract in
  [`docs/specs/public-registry-options.md`](../specs/public-registry-options.md).
- [x] Link this plan as an authorized THE-889 extraction in
  [`docs/plans/README.md`](./README.md) while Phase 2B remains the
  sequential active plan.
- [x] Update Git source examples, compatibility specs, changelog, and website
  guidance; verify Markdown links and the website build/tests. CLI help and
  installation-only npm documentation need no changes because no CLI flags or
  install behavior change. Agent search/scan workflows retain their existing
  selectors and pinned snapshot contracts.
- [x] Prepare `.loopy/commit-msg`; the outer workflow owns commits and PRs.

Phase 2's independent catalogue repository is complete (THE-891), as recorded
by the THE-892 kickoff gate. Phase 3a is authorized by THE-892's Todo state;
Phase 4 adoption/scan integration remains separate.

## Phase 3a: [x] Discover and install the built-in public catalogue (THE-892)

- [x] Add public `find_design_systems` / `install_design_system` interfaces with
  injectable Git source, state, and cache roots. Refresh the built-in `main`
  index on each online find; validate schema, minimum advertised engine version,
  and artifact paths, and cache immutable index data separately from selections.
- [x] Add the versioned find response, schema, and fixtures. Union public,
  configured publisher/app, and remembered rows without dropping origin
  collisions; keep deterministic matching, language filtering, and sorting.
- [x] Report structured partial public failures and warm/cold offline status;
  keep invalid local config/state fatal and ordinary commands free of public
  discovery traffic.
- [x] Require a positive exact revision and install every bundle language,
  definition, and provenance at one selected commit. Verify all hashes before
  state save; lock state read/modify/write; retain original commits on exact
  repeats and reject source/artifact/local slug collisions without overwrite.
- [x] Resolve explicit `public:<id>@<revision>` through shared lookup for search,
  component/token detail, and explicit configless scan selection, caching history
  without remembering the system. Preserve exact detail after refresh and reject
  wrong system/language/digests. Phase 4 owns full scan integration verification.
- [x] Display public bundle revision/commit/languages in list/show and make
  refresh refill/verify the original bundle without selecting newer revisions.
- [x] Cover public interfaces with local Git fixtures and isolated CLI runs:
  union, malformed metadata, compatibility, moved branches, all-language installs,
  independent revisions, concurrency/collisions, digest/missing Git/ref/blob
  failures, failed state writes, app/pack preservation, offline discovery/install,
  and snapshot-pinned search/detail. No live upstream requests in these tests.
- [x] Update CLI help, README quickstart, registry/search guides, and CLI reference.
  Installation-only npm docs need no change; existing agent workflows already
  accept explicit selectors and snapshot-pinned detail.
- [x] Run workspace formatting, tests, Clippy, and website installation/build/tests.
- [x] Prepare `.loopy/commit-msg`; the outer workflow owns commits and PRs.

## Phase 1 verification record

- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `cargo test --workspace`: passed (1,376 tests passed, 3 ignored).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `npm --prefix website ci` and `npm --prefix website test`: passed
  (production build and all 5 website tests).
- Markdown relative link targets and `git diff --check`: passed.

Public seams cover catalogue/schema roundtrips, `fetch_git_files`,
`RegistryCache::resolve/refresh`, ordinary/remembered resolution, sync and
lookup, `load_waxrc`, `load_lockfile`, and offline `validate_repo`. Regression
slices additionally reject missing bundle metadata, inconsistent lock/snapshot
identities, and incomplete catalogue replay while retaining legacy root locks.


## Phase 3a verification record

- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `cargo test --workspace`: passed (1,403 passed, 3 ignored).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `WAX_BUILD_VERSION=1.2.3 cargo test -p wax-core --test registry_catalog minimum_version_accepts`:
  passed, verifying the advertised release-version compatibility check.
- `npm --prefix website ci` and `npm --prefix website test`: passed
  (production build and all 5 website tests).
- CLI find/install help, Markdown relative links, and `git diff --check`: passed.

Public seams: catalogue find/install and snapshot lookup with injected local
Git repositories; shared lookup through the CLI search/detail commands; real
binary argument, JSON-schema, stdout/stderr, and exit contracts with isolated
`WAX_HOME`. Existing app/remembered/root lookup regressions passed unchanged.


## Phase 4: App consumption (THE-894)

- [x] Add scriptable `init --design-system` through the existing remembered
  selection, reject simultaneous interactive/explicit selection, validate
  requested languages before app files, and retain independent pack install /
  `--no-install` behavior.
- [x] Warm the app-local exact bundle cache during init and sync; keep committed
  registry config/lock free of global paths. Verify clean-checkout replay from
  locked Git commits and metadata-only validation with empty global state.
- [x] Add `sync --design-system` to explicitly adopt a same-system/repository
  bundle across all bound languages, using existing prepare/persist rollback.
  Verify missing-language, wrong-system/source, corrupt definition/registry,
  unavailable exact commit, and config/lock write failures preserve app files.
  Preserve exact original config bytes on rollback.
- [x] Show before/after revision directories and commits; keep ordinary sync,
  best-effort scan sync, nested `sync --upgrade`, global installs and refresh
  from changing app selection. Preserve historical snapshot detail.
- [x] Verify installed-id and exact `public:<id>@<revision>` configless scans,
  warmed network-denied runs, missing preinstalled packs, and no app config/lock
  writes through isolated binary flows with a local two-revision Git catalogue.
- [x] Verify the existing interactive public `run_init` handoff with bundle
  selections and keep existing local/remembered/root Git regressions.
- [x] Copy the available shadcn registry, definition, provenance and source
  SHA/digests to `engine/fixtures/public-catalogue/`, retain its upstream MIT
  license, and verify the actual React scanner's exact alias, wrong-package,
  candidate and absent-token behavior. No parser matching changes.
- [ ] Copy the other six pinned pilot registries and licensed consumers from
  the Phase 3b merge; verify Radix primitives/Themes, Astryx React/Swift, and
  independent Material M2/M3 via actual parser public seams.
- [x] Update README, changelog, CLI/website/npm/search+scan skill guidance,
  delivered-contract spec and cross-repository contribution links. Document
  trust, coverage gaps, offline/CI and correction/retention policy.
- [ ] Confirm the secondary repository's matching documentation, six-system
  public smoke, and Phase 3b production workflow/staging/PR-check evidence.
- [ ] Record accepted extraction ADR and archive only once implementation
  merges and all external gates above are satisfied. Phase 2B remains active.
- [x] Run workspace formatting, tests and Clippy; website install/build/tests;
  search-skill contract checks; CLI help, rendered HTML and relative-link checks.
- [x] Prepare `.loopy/commit-msg`; the outer workflow owns commits and PRs.

### External evidence gap

Read-only inspection and a clean public clone on 2026-11-21 found
`Daio-io/wax-registries` at `761243349f9a74a9a70dc20a8877b430241bcae7`
(the shadcn publication merge). Its index has **one system / one registry**,
not the planned six systems / seven registries. There is no Phase 3b merge,
weekly/manual maintenance workflow, six source manifests, maintenance runbook
or trusted-staging evidence at that commit. The only workflow is
`.github/workflows/validate.yml`. The local THE-894 snapshot marks THE-893
Done, but the required recorded Phase 3b merge SHA is absent.

Do not fabricate pilot artifacts, claim the missing smoke/evidence passed,
mark Phase 3b complete, or archive this extraction on that basis. Complete the
open checks against the actual Phase 3b merge when it becomes available.

### Manual public smoke

- Catalogue source: clean clone of `https://github.com/Daio-io/wax-registries.git`
  at `761243349f9a74a9a70dc20a8877b430241bcae7`.
- Engine base SHA: `a643f3cb1b48fec081b6cdf35b6fb3295408d4b4` plus the uncommitted
  THE-894 worktree changes. Built with `WAX_BUILD_VERSION=0.10.2-alpha.1`
  for the catalogue's advertised compatibility floor; this is a test build,
  not a released engine. Binary SHA-256:
  `6c464932cb00bff56c84ad12e8d04a2b823e75d30c9956ac7840e746f64b29a4`.
- Local test-built React pack archive SHA-256:
  `64183f0114d597e6c035c3c31346ac68e9a5b8b003f8f0837a528eeb6de72500`.
  Init used its local pack index and installed the pack normally.
- Passed find → exact shadcn r1 install → scriptable init → app search →
  snapshot-pinned offline detail → strict scan → validate. Actual parser scan:
  complete, one resolved `Button`, zero candidates, zero token references.
- Removed remembered registry state and global registry cache, denied remote
  Git access, then passed strict warmed scan and offline app search. Metadata
  validation also passed with a fresh empty `WAX_HOME`.
- Ten smoke commands passed. No other system is published at this commit;
  the requested six-system smoke and scheduled maintenance evidence remain
  open. The test build uses the engine changes before the outer workflow's
  commit; the final merge SHA must be added by closeout.

### Phase 4 verification record

- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `cargo test --workspace`: passed (1,418 passed, 3 ignored).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `npm --prefix website ci` and `npm --prefix website test`: passed
  (production build and all 5 website tests).
- `scripts/test-wax-search-skill-contract.sh`: passed.
- CLI init/sync help and rendered guide/reference HTML checked, including
  adoption/offline/coverage anchors and the pending pilot matrix. Markdown
  relative file links, pinned fixture SHA-256 digests and `git diff --check`:
  passed.

Public seams: the real binary with isolated state/local Git catalogue/test
packs; public `run_init` with interactive selections; actual
`ReactLanguage::scan` against the pinned shadcn registry. Existing parser-pack
package/candidate/legacy parity and root Git/local/remembered regressions pass
in the workspace suite. Missing catalogue pilots and external production
maintenance evidence are open acceptance gates, not verified outcomes.
