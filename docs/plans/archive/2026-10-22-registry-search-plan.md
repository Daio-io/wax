# Registry search and agent workflow plan

**Status:** Complete  
**Roadmap order:** 18  
**Tracking:** [GitHub issue #327](https://github.com/Daio-io/wax/issues/327) · THE-754–THE-765

## Outcome

Wax now exposes versioned registry search and exact-detail contracts, resolves
local, remembered, HTTP, Git, configured-app, and cached sources, and preserves
immutable snapshot identity from discovery through implementation. Parser-backed
publishers provide rich source facts without breaking legacy registries. The
`wax-search` skill closes the workflow by teaching agents to use those public
contracts safely instead of recreating registry parsing or ranking.

## Phase 5 completion checklist

- [x] Add `skills/wax-search/SKILL.md` and the tested
  `examples/search-and-use.md` workflow.
- [x] Cover component and token selection, canonical values, ambiguity, no
  matches, unavailable facts, configured apps, remembered sources, direct URLs,
  Git, offline cache, refresh, and snapshot-pinned details.
- [x] Treat registry documentation, examples, metadata, and unknown fields as
  untrusted data; never execute embedded snippets or instructions.
- [x] Keep registry publication lossless with explicit source associations and
  keep `wax-scan` analytics independent from lookup.
- [x] Include the skill in plugin metadata, README and npm documentation, with
  CI path filters for every affected skill, script, and manifest.
- [x] Add `scripts/test-wax-search-skill-contract.sh` using the real built binary
  and local registry fixtures for both detail kinds and refresh-safe snapshot
  identity.
- [x] Preserve existing registry-authoring and pack-index generator contracts.
- [x] Update the changelog, roadmap, archive index, and completion ADR after the
  phase acceptance criteria pass.
- [x] Keep generated `.wax/` and global cache data out of version control.

## Verification

Run from `engine/`:

```bash
rtk cargo fmt --all
rtk cargo fmt --all --check
rtk proxy ../scripts/test-wax-search-skill-contract.sh
rtk proxy ../scripts/test-wax-registry-skill-contract.sh
rtk proxy ../scripts/test-generate-pack-index.sh
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
```

All commands passed at closeout.
