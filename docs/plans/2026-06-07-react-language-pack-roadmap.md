# React Language Pack Capability Roadmap

## Summary

This roadmap records the longer React ambition separately from the first production implementation plan. The immediate plan should stay shippable. The roadmap should keep the architecture pointed at best-in-class design-system analysis instead of a shallow JSX tag counter.

This document is a draft and does not schedule implementation until it is added to `docs/plans/README.md`.

## Version Roadmap

| Version | Theme | Capabilities |
|---------|-------|--------------|
| React v1 | Registry usage and locals | SWC parser, configured roots, registry loading, local component declarations, import-aware JSX usage resolution, aliases, configured package entrypoints, deterministic `ScanFacts`, diagnostics for gaps. |
| React v1.1 | Resolver depth | Multi-hop re-export chains, barrel files, package `exports`, monorepo workspace packages, `tsconfig` auto-detection, Vite/Webpack alias hints, clearer unresolved-import reporting. |
| React v1.2 | Wrapper and composition analysis | Local component dependency graph, components that wrap design-system components, wrapper adoption candidates, static composition traces. |
| React v1.3 | Prop and variant usage | Track selected design-system props such as `variant`, `size`, and `color`; summarize values by registry component; detect deprecated values when registry metadata supports them. |
| React v2 | Explainable React intelligence | Resolution traces, confidence levels, stable graph output, custom rules, richer export hooks, and cross-language analysis surfaces that combine React with Compose and future packs. |

## Principles

- Keep the Wax registry as the source of truth for design-system identity.
- Prefer resolved facts over broad guesses.
- Emit diagnostics when accuracy depends on missing config.
- Keep outputs deterministic and repo-local.
- Reuse the normalized language-pack contract unless a roadmap phase proves the shared contract is insufficient.
