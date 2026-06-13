# Wax implementation plans

Agents and maintainers use this file as the **source of truth** for which plan to read and execute, and in what order.

**Rules:**

1. **Completed plans** are archived under [`archive/`](./archive/README.md) with ADR records in [`docs/adr/`](../adr/README.md).
2. **Implementation** follows one active plan at a time. Post-alpha UX is explicitly deferred.
3. Each implementation task remains **one PR per task** inside the active plan, per that plan's execution model.
4. Update the **Doc status** and **Implementation status** columns when a plan doc PR merges or when implementation of a plan finishes.

**Active plan:** SwiftUI language pack (order 8) is `complete`; post-alpha UX (order 5) remains deferred until the roadmap reopens.

---

## Roadmap

| Order | Plan | Document | Doc status | Implementation status | ADR |
|------:|------|----------|------------|------------------------|-----|
| 1 | Rust engine and language packs | [archive/2026-05-16-rust-engine-language-packs-plan.md](./archive/2026-05-16-rust-engine-language-packs-plan.md) | `merged` | `complete` | [ADR](../adr/2026-05-16-rust-engine-language-packs.md) |
| 2 | Release and rollout (alpha) | [archive/2026-05-24-release-and-rollout-plan.md](./archive/2026-05-24-release-and-rollout-plan.md) | `merged` | `complete` | [ADR](../adr/2026-05-24-alpha-release-and-distribution.md) |
| 3 | Registry sources and centralized wax layout | [archive/2026-06-02-registry-sources-and-wax-layout.md](./archive/2026-06-02-registry-sources-and-wax-layout.md) | `merged` | `complete` | [ADR](../adr/2026-06-02-registry-sources-and-wax-layout.md) |
| 4 | Registry discovery and skill-assisted sync | [archive/2026-06-04-registry-discovery-plan.md](./archive/2026-06-04-registry-discovery-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-04-registry-discovery.md) |
| 5 | Post-alpha UX | [2026-05-24-post-alpha-ux-plan.md](./2026-05-24-post-alpha-ux-plan.md) | `merged` | `deferred` | — |
| 6 | React language pack | [archive/2026-06-07-react-language-pack-plan.md](./archive/2026-06-07-react-language-pack-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-07-react-language-pack.md) |
| 7 | Generic registry discovery protocol | [archive/2026-06-10-generic-registry-discovery-protocol.md](./archive/2026-06-10-generic-registry-discovery-protocol.md) | `merged` | `complete` | [ADR](../adr/2026-06-10-generic-registry-discovery-protocol.md) |
| 8 | SwiftUI language pack | [2026-06-13-swift-language-pack-plan.md](./2026-06-13-swift-language-pack-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-13-swift-language-pack.md) |

**Doc status:** `pending` → plan PR open; `merged` → plan doc on `main`; `planned` → not yet drafted.

**Implementation status:** `not-started` | `in-progress` | `complete` | `deferred`.

---

## Which plan should I run?

```text
IF order 8 doc status is not merged
  → do not implement SwiftUI language pack tasks; review 2026-06-12-swift-language-pack-design.md and 2026-06-13-swift-language-pack-plan.md only

ELSE IF order 8 implementation is not complete
  → execute the next unchecked task in 2026-06-13-swift-language-pack-plan.md

ELSE
  → no active implementation plan; post-alpha UX (order 5) is deferred
```

Orders 1–4, 6, 7, and 8 are **complete**. Post-alpha UX (order 5) remains deferred. See [`archive/README.md`](./archive/README.md) and [`docs/adr/`](../adr/README.md) for prior implementation records.

---

## Plan document PRs (docs only)

| PR sequence | Branch | PR | Contents |
|-------------|--------|-----|----------|
| 1 | `docs/plans-roadmap` | #33 (merged) | This `README.md` + spec roadmap section |
| 2 | `docs/release-and-rollout-plan` | #32 (merged) | Release plan on `main` |
| 3 | `dai/registry-sources-plans` | #66 (merged) | Registry sources and centralized wax layout design + implementation plan |
| 4 | `docs/registry-discovery-plan` | #87 (merged) | Registry discovery design + implementation plan |
| 5 | `docs/post-alpha-ux-plan` | #34 (merged) | Post-alpha UX plan + links |
| 6 | `codex/react-language-pack-plan` | #95 (merged) | React language pack design, implementation plan, and capability roadmap |
| 7 | `docs/generic-registry-discovery-plan` | #115 (merged) | Generic registry discovery protocol implementation plan |
| 8 | `dai/swift-language-pack-plan` | merged | SwiftUI language pack design and implementation plan |

---

## Related specs and ADRs

- [ADR index](../adr/README.md) — what each completed plan shipped
- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — contracts, CLI names, distribution
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — product scope and future surfaces
- [Registry sources design](../specs/2026-06-02-registry-sources-and-wax-layout-design.md) — `.wax/` layout (order 3, complete)
- [Registry discovery design](./archive/2026-06-04-registry-discovery-design.md) — registry authoring (order 4, complete)
- [React language pack design](./archive/2026-06-07-react-language-pack-design.md) — React parser-backed pack (complete)
- [SwiftUI language pack design](./2026-06-12-swift-language-pack-design.md) — in-progress SwiftUI parser-backed pack
