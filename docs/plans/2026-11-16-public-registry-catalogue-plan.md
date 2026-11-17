# Public registry catalogue implementation plan

**Status:** Phase 1 implemented and verified; awaiting outer workflow review. Authorized as a THE-889 extraction
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

Phase 2 (publisher-side bundle authoring/publishing workflow) and Phase 3
(catalogue index distribution) are out of scope for this phase and tracked
separately under THE-889.

## Verification record

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
