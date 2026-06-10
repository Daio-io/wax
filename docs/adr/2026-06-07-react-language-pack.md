# ADR: React language pack

**Status:** Accepted (implemented)  
**Date:** 2026-06-07  
**Type:** Addendum (ecosystem extractor)  
**Related:** [Design spec](../plans/archive/2026-06-07-react-language-pack-design.md) · [Capability roadmap](../plans/archive/2026-06-07-react-language-pack-roadmap.md) · [Archived implementation plan](../plans/archive/2026-06-07-react-language-pack-plan.md)

## Context

`wax-lang-react` began as a stdio skeleton in the foundation plan. Public alpha needed a production React extractor that emits registry components, local components, and resolved design-system JSX usage through the existing `ScanFacts` contract—without React-specific logic in `wax-core` or `wax-cli`.

## Decision

Promote `wax-lang-react` to a **SWC parser-backed language pack** with:

1. **React scan config** — typed config for source roots, skip patterns, module resolution aliases, and package entrypoints.
2. **Registry integration** — load schema v1 registry JSON, build canonical and alias maps, exclude non-React targets.
3. **Module graph** — index imports/exports, resolve relative imports, aliases, and configured package entrypoints.
4. **Extraction** — discover local JSX-returning components; resolve JSX usage to registry symbols with scoped unresolved diagnostics.
5. **Facts emission** — validated `ScanFacts` with golden fixture coverage and stable wire error codes.
6. **Public distribution** — React in release artifacts, generated pack index, init exposure when required, release dry-run verification.

v1 accuracy is intentionally conservative: simple exported components, one-hop re-exports, configured aliases—advanced patterns remain on the capability roadmap.

## Implementation summary

All 13 tasks shipped:

| Phase | What shipped |
|-------|----------------|
| Config and registry | `ReactScanConfig`, registry loader, file collection with skip patterns |
| Parsing | SWC wrapper for JS/TS/JSX/TSX with diagnostic mapping |
| Resolution | Module graph, local component discovery, JSX-to-registry matching |
| Integration | Subprocess protocol conformance, workspace test updates |
| Docs | React v1 behavior documented, stale deferral notes removed |
| Release | React in pack index and release workflow, public install/onboarding docs |
| Verification | Full React/workspace checks, local release packaging, pack-index validation, release dry-run |

## Consequences

### Positive

- React repos can scan alongside Compose with the same engine merge and output paths.
- Parser-backed facts replace the scaffold empty response for configured scans.
- React is a first-class public language pack in the alpha index.

### Negative / trade-offs

- v1 module resolution is bounded; complex barrel files and dynamic imports need roadmap follow-ups.
- SWC dependency increases pack binary size and build complexity vs the line-scanner `basic` pack.

## References

- [Archived design spec](../plans/archive/2026-06-07-react-language-pack-design.md)
- [Archived capability roadmap](../plans/archive/2026-06-07-react-language-pack-roadmap.md)
- [Archived implementation plan](../plans/archive/2026-06-07-react-language-pack-plan.md)
- [Rust engine foundation ADR](./2026-05-16-rust-engine-language-packs.md)
