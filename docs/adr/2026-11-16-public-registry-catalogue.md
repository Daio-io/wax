# ADR: Public registry catalogue and Git design-system bundles

**Status:** Accepted (implemented; pilot follow-ups tracked in Linear)
**Date:** 2026-11-16
**Type:** Registry source, lockfile, and catalogue contract
**Related:** [Public registry distribution options](../specs/public-registry-options.md) · [Git registry sources ADR](./2026-08-01-git-registry-sources.md) · [Archived implementation plan](../plans/archive/2026-11-16-public-registry-catalogue-plan.md)

## Context

Public design systems needed a way to publish registries that other
repositories can pin and consume. Root-level Git registry sources could not
group a design system's definition and per-language registries at one commit,
and there was no way to find published systems.

## Decision

1. **Git-subfolder bundles.** Git registry sources accept optional `subdir` and
   `system`. A bundle's `<subdir>/.wax/wax.config.json` definition and every
   declared `.wax/registries/<language>.json` resolve together at one
   immutable commit, with path-safety checks and digest verification before
   any state is saved. Main-repository and npm-package distribution were
   rejected; see the options spec.
2. **Lockfile v3.** Locked registries gain optional `subdir` and `definition`
   metadata. The writer emits schema version 3 and still reads versions 1 and
   2. A shared Git source identity keeps root and nested sources from
   colliding.
3. **Built-in public catalogue.** `find_design_systems` and
   `install_design_system` read a versioned catalogue index from
   `Daio-io/wax-registries`. Installs require an exact revision, and
   `public:<id>@<revision>` selectors work for search, detail, and configless
   scans.
4. **Explicit app adoption.** `wax init --non-interactive --design-system` and
   `wax sync --design-system` adopt one bundle revision across all bound
   languages, with rollback on failure. Committed config and lock never contain
   global paths, and `wax validate` stays repo-local and offline.

## Implementation summary

| Work | What shipped | PR |
|------|--------------|----|
| THE-890 | Catalogue contract, Git bundle plumbing, lockfile v3 | [#370](https://github.com/Daio-io/wax/pull/370) |
| THE-892 | Public catalogue find and exact install | [#372](https://github.com/Daio-io/wax/pull/372) |
| THE-894 | App consumption, scriptable init/sync, shadcn pilot fixture | [#376](https://github.com/Daio-io/wax/pull/376) |

## Open follow-ups

Tracked in Linear under THE-889 / THE-894, not in this repository:

- Add the six remaining pinned pilot registries and consumers once the
  catalogue publishes them. The catalogue currently contains shadcn only.
- Confirm the catalogue repository's documentation, the multi-system public
  smoke, and its maintenance and staging evidence.

## Consequences

- Design systems can publish multi-language bundles from ordinary repository
  subfolders without new hosting.
- Existing root Git sources and schema 1/2 lockfiles keep working.
- Registry matching and parser behavior are unchanged; coverage is limited to
  what each published bundle records.
