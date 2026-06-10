# ADR: npm trusted publishing and tag-driven versioning

**Status:** Accepted (implemented)  
**Date:** 2026-06-04  
**Type:** Addendum (release channel hardening for `@waxhq/wax`)  
**Related:** [Alpha release ADR](./2026-05-24-alpha-release-and-distribution.md) · [Archived plans](../plans/archive/2026-06-04-npm-trusted-publishing.md) · [Tag-driven versioning plan](../plans/archive/2026-06-04-npm-tag-driven-versioning.md)

## Context

The alpha release plan introduced an optional `@waxhq/wax` npm wrapper that downloads the engine binary on postinstall. Two follow-on gaps remained:

1. Published npm versions must match GitHub release tags, not hand-edited `package.json` metadata.
2. npm publish should use trusted publishing (OIDC) from the existing `release.yml` pipeline rather than long-lived tokens.

## Decision

1. **Package identity** — rename user-facing npm references to `@waxhq/wax` across `packages/cli`, README, and install messaging.
2. **Tag-driven versioning** — checked-in `packages/cli/package.json` uses a snapshot placeholder; `release.yml` rewrites the version from `WAX_RELEASE_TAG` immediately before `npm publish`.
3. **Trusted publishing** — add a post-release npm job to `release.yml` with `id-token: write`, version guard comparing tag to package metadata, and GitHub-hosted Ubuntu + modern Node.
4. **Workflow invariants** — `scripts/check-release-workflow.rb` enforces release-time version rewrite and publish job requirements.

## Implementation summary

| Plan | Tasks | What shipped |
|------|-------|----------------|
| Tag-driven versioning | 2 | Release workflow version rewrite, snapshot placeholder in package.json, docs updated, invariant checker |
| Trusted publishing | 3 | `@waxhq/wax` rename, trusted-publishing npm job, packed-contents verification |

## Consequences

### Positive

- npm and GitHub Release versions stay aligned by construction.
- OIDC trusted publishing removes long-lived npm tokens from CI secrets.
- Release workflow invariants catch regressions in CI.

### Negative / trade-offs

- Local `packages/cli/package.json` version is not the published version until release runs.
- npm remains an optional install channel; curl and Homebrew are primary.

## References

- [Archived trusted publishing plan](../plans/archive/2026-06-04-npm-trusted-publishing.md)
- [Archived tag-driven versioning plan](../plans/archive/2026-06-04-npm-tag-driven-versioning.md)
- [Alpha release ADR](./2026-05-24-alpha-release-and-distribution.md)
