# ADR: Registry search and agent workflow

**Status:** Accepted  
**Date:** 2026-10-22  
**Type:** Product contract and agent workflow  
**Related:** [Archived implementation plan](../plans/archive/2026-10-22-registry-search-plan.md)

## Context

Agents and developers need to find design-system components and tokens without
guessing from names or reading registry formats directly. Lookup must work from
configured apps and explicit sources, preserve the exact content evaluated, and
remain safe when publisher-authored documentation contains arbitrary text.

## Decision

1. **Versioned public envelopes.** `RegistrySearchResponse` exposes deterministic
   fuzzy results and snapshot references; `RegistryDetailResponse` returns one
   exact, lossless entry from the selected immutable snapshot.
2. **Snapshot-pinned handoff.** Consumers carry the result id and digest into
   `wax component` or `wax token`. Cached historical content keeps that detail
   stable after a remembered source refresh.
3. **Source-backed selection.** The CLI supports configured app locks,
   remembered systems, paths, HTTP URLs, Git tags or commits, and offline cache
   resolution with typed failures.
4. **Safe agent workflow.** `skills/wax-search` teaches search, comparison,
   exact detail, and repository-native implementation. Registry docs, metadata,
   attachments, examples, and unknown fields remain untrusted data and are not
   executed or treated as instructions.
5. **Separated responsibilities.** `wax-registry-discover` owns explicit
   publisher associations and lossless rediscovery. `wax-scan` owns adoption
   analytics. The search skill does not implement a parser, ranker, package
   upgrade, registry refresh, or scan metric.

## Consequences

- UI implementation can cite exact registry identity and facts without silent
  drift during refresh.
- Missing facts, empty results, ambiguity, and offline misses stay visible
  rather than triggering guessed imports, props, or token values.
- The shipped skill collection gains a third focused workflow while publisher
  maintenance and scan analytics retain their existing contracts.
- Contract verification builds the real CLI and exercises component and token
  detail against local fixtures; existing registry-skill and pack-index tests
  remain part of closeout.
