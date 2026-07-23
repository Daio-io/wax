# Wax implementation plans

Agents and maintainers use this file as the **source of truth** for which plan to read and execute, and in what order.

**Rules:**

1. **Completed plans** are archived under [`archive/`](./archive/README.md) with ADR records in [`docs/adr/`](../adr/README.md).
2. **Implementation** follows one active plan at a time. Post-alpha UX is explicitly deferred except for extracted tasks listed in this roadmap.
3. Each implementation task remains **one PR per task** inside the active plan, per that plan's execution model.
4. Update the **Doc status** and **Implementation status** columns when a plan doc PR merges or when implementation of a plan finishes.

**Active plan:** None promoted yet. Adoption Metrics v2 (order 11) and Token inference and reporting (order 14) are complete and archived; see their [Adoption Metrics v2 archive](./archive/2026-06-20-adoption-metrics-v2-plan.md), [Adoption Metrics v2 ADR](../adr/2026-06-20-adoption-metrics-v2.md), [token inference archive](./archive/2026-07-19-token-inference-reporting-plan.md), and [token inference ADR](../adr/2026-07-19-token-inference-reporting.md). No new active-plan work begins until a maintainer promotes the next plan.

---

## Roadmap

| Order | Plan | Document | Doc status | Implementation status | ADR |
|------:|------|----------|------------|------------------------|-----|
| 1 | Rust engine and language packs | [archive/2026-05-16-rust-engine-language-packs-plan.md](./archive/2026-05-16-rust-engine-language-packs-plan.md) | `merged` | `complete` | [ADR](../adr/2026-05-16-rust-engine-language-packs.md) |
| 2 | Release and rollout (alpha) | [archive/2026-05-24-release-and-rollout-plan.md](./archive/2026-05-24-release-and-rollout-plan.md) | `merged` | `complete` | [ADR](../adr/2026-05-24-alpha-release-and-distribution.md) |
| 3 | Registry sources and centralized wax layout | [archive/2026-06-02-registry-sources-and-wax-layout.md](./archive/2026-06-02-registry-sources-and-wax-layout.md) | `merged` | `complete` | [ADR](../adr/2026-06-02-registry-sources-and-wax-layout.md) |
| 4 | Registry discovery and skill-assisted review | [archive/2026-06-04-registry-discovery-plan.md](./archive/2026-06-04-registry-discovery-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-04-registry-discovery.md) |
| 5 | Post-alpha UX | [2026-05-24-post-alpha-ux-plan.md](./2026-05-24-post-alpha-ux-plan.md) | `merged` | `deferred` | — |
| 6 | React language pack | [archive/2026-06-07-react-language-pack-plan.md](./archive/2026-06-07-react-language-pack-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-07-react-language-pack.md) |
| 7 | Generic registry discovery protocol | [archive/2026-06-10-generic-registry-discovery-protocol.md](./archive/2026-06-10-generic-registry-discovery-protocol.md) | `merged` | `complete` | [ADR](../adr/2026-06-10-generic-registry-discovery-protocol.md) |
| 8 | SwiftUI language pack | [archive/2026-06-13-swift-language-pack-plan.md](./archive/2026-06-13-swift-language-pack-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-13-swift-language-pack.md) |
| 9 | Interactive init wizard | [archive/2026-06-13-interactive-init.md](./archive/2026-06-13-interactive-init.md) | `merged` | `complete` | [ADR](../adr/2026-06-13-interactive-init.md) |
| 10 | Wax scan analytics skill | [archive/2026-06-14-wax-scan-plan.md](./archive/2026-06-14-wax-scan-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-14-wax-scan-analytics-skill.md) |
| 11 | Adoption Metrics v2 | [archive/2026-06-20-adoption-metrics-v2-plan.md](./archive/2026-06-20-adoption-metrics-v2-plan.md) | `merged` | `complete` | [ADR](../adr/2026-06-20-adoption-metrics-v2.md) |
| 12 | Registry sync and config v2 | [archive/2026-07-04-registry-sync-config-plan.md](./archive/2026-07-04-registry-sync-config-plan.md) | `merged` | `complete` | [ADR](../adr/2026-07-04-registry-sync-config-v2.md) |
| 13 | Token scanning | [archive/2026-07-03-token-scanning-plan.md](./archive/2026-07-03-token-scanning-plan.md) | `merged` | `complete` | [ADR](../adr/2026-07-03-token-scanning.md) |
| 14 | Token inference and reporting | [archive/2026-07-19-token-inference-reporting-plan.md](./archive/2026-07-19-token-inference-reporting-plan.md) | `merged` | `complete` | [ADR](../adr/2026-07-19-token-inference-reporting.md) |
| 15 | Compose parse recovery and UI scoping | [2026-07-22-compose-parse-recovery-plan.md](./2026-07-22-compose-parse-recovery-plan.md) | `merged` | `not-started` | — |

**Doc status:** `pending` -> plan PR open; `merged` -> plan doc on `main`; `planned` -> not yet drafted.

**Implementation status:** `not-started` | `in-progress` | `complete` | `deferred`.

---

## Which plan should I run?

```text
-> No active plan is promoted. Wait for maintainer promotion before starting new roadmap work.
```

Orders 1-4, 6-14 are **complete**. Post-alpha UX (order 5) remains otherwise deferred. Adoption Metrics v2 shipped in PRs #165, #171, and #172 and is archived. Token inference and reporting finished implementation in PRs #231, #233, #235, #236, and #237 plus closeout PR #238 and is archived. Compose parse recovery and UI scoping (order 15) is planned, but no successor plan has been promoted to active yet.

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
| 9 | `dai/interactive-init-plan` | #142 (merged) | Interactive init design and implementation plan |
| 10 | `docs/wax-scan-skill-plan` | #151 (merged) | Wax scan analytics skill design, implementation plan, and Task 1 scaffold |
| 11 | `dai/adoption-metrics-v2-contract` | [#165](https://github.com/Daio-io/wax/pull/165), [#171](https://github.com/Daio-io/wax/pull/171), [#172](https://github.com/Daio-io/wax/pull/172) | Adoption Metrics v2 design, implementation, reporting, and closeout |
| 12 | `dai/registry-sync-config-plan` | #195 (merged) | Registry sync and config v2 design and implementation plan |
| 13 | `dai/token-scanning-plan` | #194 (merged) | Token scanning design and implementation plan |
| 14 | `dai/token-inference-reporting-design` | [#230](https://github.com/Daio-io/wax/pull/230) | Token inference/reporting design and implementation plan |
| 15 | `dai/compose-parse-recovery-design` | [#240](https://github.com/Daio-io/wax/pull/240) | Compose parse recovery/UI-scoping design and implementation plan |

---

## Related specs and ADRs

- [ADR index](../adr/README.md) — what each completed plan shipped
- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — contracts, CLI names, distribution
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — product scope and future surfaces
- [Registry sources design](../specs/2026-06-02-registry-sources-and-wax-layout-design.md) — `.wax/` layout (order 3, complete)
- [Registry discovery design](./archive/2026-06-04-registry-discovery-design.md) — registry authoring (order 4, complete)
- [React language pack design](./archive/2026-06-07-react-language-pack-design.md) — React parser-backed pack (complete)
- [SwiftUI language pack design](./archive/2026-06-12-swift-language-pack-design.md) — SwiftUI parser-backed pack (complete)
- [Interactive init design](../specs/2026-06-13-interactive-init-design.md) — extracted Post-alpha UX Task 1 plan
- [Wax scan analytics skill design](../specs/2026-06-14-wax-scan-design.md) — scan orchestration and adoption reporting skill (order 10)
- [Adoption Metrics v2 design](../specs/2026-06-20-adoption-metrics-v2-design.md) — facts-first invocation adoption contract (order 11, complete; [ADR](../adr/2026-06-20-adoption-metrics-v2.md))
- [Registry sync and config v2 design](../specs/2026-07-04-registry-sync-config-design.md) — clean alpha cutover for remembered design-system registries, no-config local scans, and explicit app sync (order 12, complete)
- [Token scanning design](../specs/2026-07-03-token-scanning-design.md) — additive token references and hard-coded styling candidates (order 13, complete; [ADR](../adr/2026-07-03-token-scanning.md))
- [Token inference and reporting design](../specs/2026-07-19-token-inference-reporting-design.md) — context-aware exact, near, unmatched, and unassessed token findings (order 14, complete; [ADR](../adr/2026-07-19-token-inference-reporting.md))
- [Compose parse recovery and UI scope design](./2026-07-22-compose-parse-recovery-design.md) — fault-tolerant Kotlin parsing and Compose-aware usage scoping (order 15, planned)
