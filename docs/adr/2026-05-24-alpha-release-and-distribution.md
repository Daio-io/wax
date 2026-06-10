# ADR: Alpha release and distribution

**Status:** Accepted (implemented)  
**Date:** 2026-05-24  
**Type:** Addendum (release phase after [Rust engine foundation](./2026-05-16-rust-engine-language-packs.md))  
**Related:** [Language packs spec](../specs/2026-05-16-language-packs-and-distribution.md) · [Archived implementation plan](../plans/archive/2026-05-24-release-and-rollout-plan.md)

## Context

After the Rust engine foundation landed, `wax` needed a public alpha: scriptable CLI surface, hosted pack index, prebuilt binaries, and install channels that do not require a local Rust toolchain. Language packs remain on-demand downloads into `~/.wax/langs/`; install channels distribute only the `wax` engine binary.

## Decision

Ship **v1 alpha** with the following distribution and CLI surface:

1. **Scan and validate CLI** — `wax scan` (with `--no-auto-install`, `--concurrency`), `wax validate` (repo-local, CI-friendly), per-language `.waxrc` config forwarded on the wire, auto-install during scan when allowed.
2. **Hosted pack index** — HTTPS fetch with default `WAX_LANG_INDEX`, `wax language doctor` shows effective index URL.
3. **Release pipeline** — tagged GitHub Releases, cargo-dist matrix for macOS/Linux triples, generated `index.json` attached to releases.
4. **Install channels** — curl installer (`scripts/install.sh`), Homebrew tap (`homebrew/Formula/wax.rb`), optional `@waxhq/wax` npm wrapper.
5. **Alpha scope bar** — new users can init, scan with parser-backed facts, validate in CI, and manage language packs without manual index configuration.

Alpha explicitly excludes static site export, backend API, web UI, Swift pack, kernel plugins, and Sigstore signing (v1.1).

## Implementation summary

All 17 tasks in the release plan shipped:

| Task area | What shipped |
|-----------|----------------|
| Scan orchestration | Language config on wire, auto-install execution, stdout scan summary |
| Validate | `wax validate` with repo-only rules and registry warnings |
| Pack index | HTTPS/`http://` fetch, default index constant, alpha fixture with `compose` + `basic` (+ `react` after React plan) |
| Versioning | `0.1.0-alpha.N` semver alignment, CHANGELOG alpha section |
| Releases | `release.yml` tag workflow, pack index generation script, post-release smoke |
| Install | curl script, Homebrew formula, `@waxhq/wax` npm postinstall wrapper |
| Docs | Getting-started README, `.waxrc` JSON Schema, CI recipe, monorepo guidance |
| Verification | Alpha smoke workflow, cross-plan documentation links |

Follow-on npm publishing hardening is recorded in [npm trusted publishing ADR](./2026-06-04-npm-trusted-publishing.md).

## Consequences

### Positive

- End users install prebuilt `wax` without compiling Rust.
- CI can run `wax validate` and `wax scan --no-auto-install` with committed lockfiles.
- Pack index and release assets stay aligned through generated `index.json`.

### Negative / trade-offs

- Alpha targets macOS/Linux triples only; Windows deferred.
- npm wrapper is optional; curl and Homebrew are the primary alpha paths.
- Registry discover/draft workflows deferred to order 4 (now shipped separately).

## References

- [Archived release and rollout plan](../plans/archive/2026-05-24-release-and-rollout-plan.md)
- [Rust engine foundation ADR](./2026-05-16-rust-engine-language-packs.md)
- [npm trusted publishing ADR](./2026-06-04-npm-trusted-publishing.md)
