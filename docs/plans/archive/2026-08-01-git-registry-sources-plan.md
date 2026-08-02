# Git registry sources implementation plan

**Status:** Complete and archived. See
[ADR](../../adr/2026-08-01-git-registry-sources.md).

**Goal:** Add the config-v2 contract for Git-backed language registries while
keeping fetching, caching, lock pinning, and CLI wiring in follow-on tasks.

## Task 1: Config + schema for git / tag

- [x] Define exhaustive `PathOrUrl` and `Git` runtime registry variants with
  field-specific validation.
- [x] Update current path/URL callers to fail explicitly for Git sources until
  Git resolution is wired.
- [x] Add the Git registry alternative to the JSON Schema and focused tests.
- [x] Add parser coverage for valid values, invalid fields, mixed modes,
  unknown fields, and existing registry forms.
- [x] Update README, config specification, and representative fixtures.
- [x] Run the focused formatting, contract, config, load, and clippy checks.

Follow-on resolver and lock semantics are tracked separately by THE-279 through
THE-281.

## Task 2: System-git fetch helper

- [x] Add typed system-git fetch and commit-pinned read helpers in `wax-core`.
- [x] Add deterministic per-URL bare-repository caching and the dedicated cache path constant.
- [x] Add system-git fixture coverage for refs, tags, locked commits, errors, safe paths, and cache isolation.
- [x] Run formatting, focused registry-git tests, and clippy verification.

## Task 4: Wax sync upgrade and offline validation

- [x] Wire `wax sync --upgrade` through the CLI and core sync options.
- [x] Keep ordinary Git sync pinned and make upgrade output report the tag and
  old/new commits without exposing remote credentials.
- [x] Make Git-only sync independent of global remembered-design-system state;
  preserve lazy state resolution for upstream registries.
- [x] Validate Git registry locks offline using canonical source, identity,
  digest, and full lowercase commit metadata.
- [x] Preserve transactional sync behavior and avoid lockfile rewrites when
  resolved pins are unchanged.
- [x] Run the focused core and CLI sync/validate/help checks.

## Task 5: Docs and README examples

- [x] **Task 5 complete**

- [x] Document the config-v2 Git/tag shape, fixed registry path, and supported
  string, source, upstream, and per-language modes.
- [x] Document locked commit/digest lifecycle, ordinary sync versus
  `wax sync --upgrade`, and offline Git-mode validation.
- [x] Document committed config/lock files, operational ignore paths, CI
  requirements, and system-Git authentication boundaries.
- [x] Update the changelog and cross-reference the existing registry-sync and
  language-pack lock contracts.
- [x] Verify documentation against the shipped CLI and focused engine tests.
