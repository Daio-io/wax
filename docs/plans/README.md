# Wax implementation plans

Agents and maintainers use this file as the **source of truth** for which plan to read and execute, and in what order.

**Rules:**

1. **Plan document PRs** land in the order below (roadmap → release plan → registry sources/layout plan → registry discovery plan → post-alpha UX plan). Do not open implementation PRs from a plan until that plan’s doc row is `merged`.
2. **Implementation** follows one plan at a time: complete (or explicitly defer) all tasks in the active plan before starting the next plan’s Task 1.
3. Each implementation task remains **one PR per task** inside the active plan, per that plan’s execution model.
4. Update the **Doc status** and **Implementation status** columns when a plan doc PR merges or when implementation of a plan finishes.

---

## Roadmap

| Order | Plan | Document | Doc status | Implementation status | Gate (start implementation) |
|------:|------|----------|------------|------------------------|-----------------------------|
| 1 | Rust engine and language packs | [2026-05-16-rust-engine-language-packs-plan.md](./2026-05-16-rust-engine-language-packs-plan.md) | `merged` | `complete` | — |
| 2 | Release and rollout (alpha) | [2026-05-24-release-and-rollout-plan.md](./2026-05-24-release-and-rollout-plan.md) | `merged` | `complete` | Order 1 implementation `complete` |
| 3 | Registry sources and centralized wax layout | [2026-06-02-registry-sources-and-wax-layout.md](./2026-06-02-registry-sources-and-wax-layout.md) | `merged` | `complete` | Order 2 implementation `complete`; may run before registry discover/draft |
| 4 | Registry discovery and skill-assisted sync | [2026-06-04-registry-discovery-plan.md](./2026-06-04-registry-discovery-plan.md) | `pending` | `in-progress` | Order 3 implementation `complete`; implementation starts after this plan doc lands |
| 5 | Post-alpha UX | [2026-05-24-post-alpha-ux-plan.md](./2026-05-24-post-alpha-ux-plan.md) | `merged` | `not-started` | Order 4 implementation `complete`; order 2 public alpha shipped (`v0.1.0-alpha.1` or agreed tag) |

**Doc status:** `pending` → plan PR open; `merged` → plan doc on `main`; `planned` → not yet drafted.

**Implementation status:** `not-started` | `in-progress` | `complete`.

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

ELSE IF order 5 implementation is not complete
  → execute 2026-05-24-post-alpha-ux-plan.md from Task 1

ELSE
  → pick next `planned` product plan (export, richer registry metadata, etc.)
```

---

## Plan document PR order (docs only)

Merge these **documentation PRs** in sequence (separate from implementation task PRs):

| PR sequence | Branch | PR | Contents |
|-------------|--------|-----|----------|
| 1 | `docs/plans-roadmap` | #33 (merged) | This `README.md` + spec roadmap section |
| 2 | `docs/release-and-rollout-plan` | #32 (merged) | Release plan on `main` |
| 3 | `dai/registry-sources-plans` | #66 (merged) | Registry sources and centralized wax layout design + implementation plan |
| 4 | `docs/registry-discovery-plan` | pending | Registry discovery design + implementation plan |
| 5 | `docs/post-alpha-ux-plan` | #34 (merged) | Post-alpha UX plan + links |

Plan document PRs #32, #33, #34, and #66 are merged on `main`.

---

## Related specs

- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — contracts, CLI names, distribution
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — product scope and future surfaces
- [Registry discovery and skill-assisted sync](./2026-06-04-registry-discovery-design.md) — next registry authoring phase
