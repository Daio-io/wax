# Wax implementation plans

Agents and maintainers use this file as the **source of truth** for which plan to read and execute, and in what order.

**Rules:**

1. **Plan document PRs** land in the order below (roadmap → release plan → registry sources/layout plan → registry discovery plan → post-alpha UX plan → React language pack draft plan). Do not open implementation PRs from a plan until that plan’s doc row is `merged`.
2. **Implementation** follows one plan at a time: complete (or explicitly defer) all tasks in the active plan before starting the next plan’s Task 1. Post-alpha UX is explicitly deferred; React language pack is the current active plan.
3. Each implementation task remains **one PR per task** inside the active plan, per that plan’s execution model.
4. Update the **Doc status** and **Implementation status** columns when a plan doc PR merges or when implementation of a plan finishes.

---

## Roadmap

| Order | Plan | Document | Doc status | Implementation status | Gate (start implementation) |
|------:|------|----------|------------|------------------------|-----------------------------|
| 1 | Rust engine and language packs | [2026-05-16-rust-engine-language-packs-plan.md](./2026-05-16-rust-engine-language-packs-plan.md) | `merged` | `complete` | — |
| 2 | Release and rollout (alpha) | [2026-05-24-release-and-rollout-plan.md](./2026-05-24-release-and-rollout-plan.md) | `merged` | `complete` | Order 1 implementation `complete` |
| 3 | Registry sources and centralized wax layout | [2026-06-02-registry-sources-and-wax-layout.md](./2026-06-02-registry-sources-and-wax-layout.md) | `merged` | `complete` | Order 2 implementation `complete`; may run before registry discover/draft |
| 4 | Registry discovery and skill-assisted sync | [2026-06-04-registry-discovery-plan.md](./2026-06-04-registry-discovery-plan.md) | `merged` | `complete` | Order 3 implementation `complete`; plan doc merged |
| 5 | Post-alpha UX | [2026-05-24-post-alpha-ux-plan.md](./2026-05-24-post-alpha-ux-plan.md) | `merged` | `deferred` | Order 4 implementation `complete`; public alpha shipped; maintainers explicitly deferred this plan while React language pack is active |
| 6 | React language pack | [2026-06-07-react-language-pack-plan.md](./2026-06-07-react-language-pack-plan.md) | `merged` | `in-progress` | Current active plan: Order 4 implementation `complete`; public alpha shipped; Post-alpha UX explicitly deferred; React plan doc merged |

**Doc status:** `pending` → plan PR open; `merged` → plan doc on `main`; `planned` → not yet drafted.

**Implementation status:** `not-started` | `in-progress` | `complete` | `deferred`.

Set order 1 to `complete` only after `cd engine && cargo test --workspace` passes on `main` (including `wax-lang-basic` and `wax-lang-compose`). Do not infer completion from foundation plan doc checkboxes alone.

---

## Which plan should I run?

```text
IF order 1 implementation is not complete
  → finish remaining tasks in rust-engine-language-packs plan (unlikely on main)

ELSE IF order 2 doc status is not merged
  → do not implement release tasks; wait for plan doc PR or merge plan doc first

ELSE IF order 2 implementation is not complete
  → execute 2026-05-24-release-and-rollout-plan.md from Task 1

ELSE IF order 3 doc status is not merged
  → do not implement registry source/layout tasks; wait for plan doc PR or merge plan doc first

ELSE IF order 3 implementation is not complete
  → execute 2026-06-02-registry-sources-and-wax-layout.md from Task 1

ELSE IF order 4 doc status is not merged
  → do not implement registry discovery; wait for plan doc PR or merge plan doc first

ELSE IF order 4 implementation is not complete
  → execute 2026-06-04-registry-discovery-plan.md from Task 1

ELSE IF order 5 doc status is not merged
  → do not implement post-alpha UX tasks

ELSE IF order 5 gate not satisfied (public alpha not shipped)
  → wait; optional: help close remaining release plan tasks

ELSE IF order 5 implementation is not complete and not explicitly deferred
  → execute 2026-05-24-post-alpha-ux-plan.md from Task 1

ELSE IF order 6 gate not satisfied (React plan doc is not merged, public alpha has not shipped, or Post-alpha UX is neither complete nor explicitly deferred)
  → do not implement React language pack tasks; review 2026-06-07-react-language-pack-design.md and roadmap only

ELSE IF order 6 implementation is not complete
  → execute the next unchecked task in 2026-06-07-react-language-pack-plan.md

ELSE
  → pick next `planned` product plan (export, richer registry metadata, etc.)
```

---

## Plan document PRs (docs only)

This table records the plan-document PR associated with each execution-order row. Earlier docs may have merged before later roadmap reshuffles; the roadmap table above is the source of truth for execution order.

| PR sequence | Branch | PR | Contents |
|-------------|--------|-----|----------|
| 1 | `docs/plans-roadmap` | #33 (merged) | This `README.md` + spec roadmap section |
| 2 | `docs/release-and-rollout-plan` | #32 (merged) | Release plan on `main` |
| 3 | `dai/registry-sources-plans` | #66 (merged) | Registry sources and centralized wax layout design + implementation plan |
| 4 | `docs/registry-discovery-plan` | #87 (merged) | Registry discovery design + implementation plan |
| 5 | `docs/post-alpha-ux-plan` | #34 (merged) | Post-alpha UX plan + links |
| 6 | `codex/react-language-pack-plan` | #95 (merged) | React language pack design, implementation plan, and capability roadmap |

Plan document PRs #32, #33, #34, #66, #87, and #95 are merged on `main`.

---

## Related specs

- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — contracts, CLI names, distribution
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — product scope and future surfaces
- [Registry discovery and skill-assisted sync](./2026-06-04-registry-discovery-design.md) — registry authoring phase (order 4, complete)
- [React language pack design](./2026-06-07-react-language-pack-design.md) — active React parser-backed language pack scope
