# ADR: Git registry sources

**Status:** Accepted (implemented)
**Date:** 2026-08-01
**Type:** Addendum (config-v2 Git/tag registry sources)
**Related:** [Registry sync and config v2 design](../specs/2026-07-04-registry-sync-config-design.md) · [Archived implementation plan](../plans/archive/2026-08-01-git-registry-sources-plan.md)

## Context

Config v2 already supported path, URL, and upstream registry modes. Design-system
teams often publish language registries from Git at a conventional path, and app
repos needed a way to pin those registries by tag or commit without copying files
into the consumer repo. Fetching, lock pinning, upgrade, and offline validation
had to stay aligned with the existing language-pack lock contract.

## Decision

1. **Git/tag config shape** — A language may use
   `registry: { "git": "<url>", "tag": "<tag-or-sha>" }`. Both fields are
   required. The remote registry path is fixed at
   `.wax/registries/<language-id>.json`; there is no configurable `path`.
2. **System Git only** — Resolve and fetch through the host `git` binary. Cache
   bare clones under `.wax/cache/` and materialize registry JSON for scans.
   Authenticate through system Git helpers (SSH agent, credential helpers); do
   not put credentials in the `git` URL. Config and `.wax/wax.lock.json` persist
   that URL for commit, and Wax only redacts userinfo in error/output labels.
3. **Lock pins** — Record canonical Git identity, full lowercase commit, and
   registry SHA-256 digest in `.wax/wax.lock.json`. Ordinary `wax scan` /
   `wax sync` reuse the locked commit even when a remote tag moves.
4. **Deliberate upgrades** — `wax sync --upgrade` re-resolves configured tags and
   updates pins. Editing `git` or `tag` changes registry identity and forces a
   new resolution.
5. **Offline validate** — Git-mode `wax validate` checks committed config and
   lock metadata only. It does not require network, cache, or remote reachability.
6. **Hard failures** — Git fetch or registry-read failures fail scan/sync.
   Missing Git registry lock entries may be auto-pinned; language-pack pins
   remain required for reproducible CI.

## Implementation summary

| Task | What shipped | PR |
|------|----------------|-----|
| 1. Config + schema | Exhaustive `Git` registry variant, JSON Schema, fixtures | [#262](https://github.com/Daio-io/wax/pull/262) |
| 2. System-git fetch | Bare-repo cache, commit-pinned reads, fixture coverage | [#263](https://github.com/Daio-io/wax/pull/263) |
| 3. Resolve + lock pins | Materialize registries, pin commit/digest, scan/sync reuse | [#264](https://github.com/Daio-io/wax/pull/264) |
| 4. Upgrade + validate | `wax sync --upgrade`, offline Git validate, CLI wiring | [#265](https://github.com/Daio-io/wax/pull/265) |
| 5. Docs closeout | README/spec/changelog updates, ADR, plan archive | [#266](https://github.com/Daio-io/wax/pull/266) |

## Consequences

### Positive

- App repos can consume design-system registries from Git without vendoring
  copies of the JSON.
- Locked commits keep CI reproducible when tags move.
- `wax validate` stays CI-friendly and offline for Git mode.
- Existing path/URL/upstream registry modes remain unchanged.

### Negative / trade-offs

- Requires system Git plus credentials/network when the pinned commit is not
  already cached.
- HTTPS URLs that embed userinfo are accepted today and copied into committed
  lock metadata; operators must keep secrets out of the URL rather than relying
  on Wax to strip them on write.
- No signed-tag or signed-commit verification in this change; authenticity rests
  on commit pins, digests, and host Git authentication.
- Fixed remote registry path only; packs that publish elsewhere need a different
  registry mode.

## References

- [Registry sync and config v2 design](../specs/2026-07-04-registry-sync-config-design.md)
- [Archived implementation plan](../plans/archive/2026-08-01-git-registry-sources-plan.md)
- [Registry sync and config v2 ADR](./2026-07-04-registry-sync-config-v2.md)
- PRs [#262](https://github.com/Daio-io/wax/pull/262), [#263](https://github.com/Daio-io/wax/pull/263), [#264](https://github.com/Daio-io/wax/pull/264), [#265](https://github.com/Daio-io/wax/pull/265), [#266](https://github.com/Daio-io/wax/pull/266)
