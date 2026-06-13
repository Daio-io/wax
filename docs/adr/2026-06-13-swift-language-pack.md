# ADR: SwiftUI language pack

**Status:** Accepted (implemented)
**Date:** 2026-06-13
**Related:** [Design](../plans/archive/2026-06-12-swift-language-pack-design.md) · [Implementation plan](../plans/archive/2026-06-13-swift-language-pack-plan.md)

## Context

Wax supports parser-backed language packs for Compose and React. SwiftUI projects need
the same registry-backed scan and per-language discovery workflow without adding
Swift-specific logic to the engine.

## Decision

Add `wax-lang-swift` as a `tree-sitter-swift` backed language pack. Swift v1 detects
`struct Name: View` declarations, `func Name(...) -> some View` declarations, direct
registry-backed calls, and simple member-qualified calls by final member name. It
implements both `scan` and `discover` over the existing stdio wire protocol.

## Consequences

- SwiftUI projects can scan and discover design-system registries through the same
  CLI workflow as Compose and React.
- The scanner remains static and deterministic, but does not perform Swift module
  or type resolution.
- Future SwiftPM/Xcode/SourceKit-aware resolution can build on this pack without
  changing the engine contract.
