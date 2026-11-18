# Public registry catalogue implementation plan

**Status:** Phase 1 complete; Phase 3a (THE-892) implemented, with verification recorded below. Authorized as a THE-889 extraction
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
