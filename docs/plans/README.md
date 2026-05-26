# Wax implementation plans

Agents and maintainers use this file as the **source of truth** for which plan to read and execute, and in what order.

**Rules:**

1. **Plan document PRs** land in the order below (roadmap → release plan → post-alpha UX plan). Do not open implementation PRs from a plan until that plan’s doc row is `merged`.
2. **Implementation** follows one plan at a time: complete (or explicitly defer) all tasks in the active plan before starting the next plan’s Task 1.
3. Each implementation task remains **one PR per task** inside the active plan, per that plan’s execution model.
4. Update the **Doc status** and **Implementation status** columns when a plan doc PR merges or when implementation of a plan finishes.

---

## Roadmap

| Order | Plan | Document | Doc status | Implementation status | Gate (start implementation) |
|------:|------|----------|------------|------------------------|-----------------------------|
| 1 | Rust engine and language packs | [2026-05-16-rust-engine-language-packs-plan.md](./2026-05-16-rust-engine-language-packs-plan.md) | `merged` | `complete` | — |
| 2 | Release and rollout (alpha) | [2026-05-24-release-and-rollout-plan.md](./2026-05-24-release-and-rollout-plan.md) | `merged` | `not-started` | Order 1 implementation `complete` |
| 3 | Post-alpha UX | [2026-05-24-post-alpha-ux-plan.md](./2026-05-24-post-alpha-ux-plan.md) | `merged` | `not-started` | Order 2 public alpha shipped (`v0.1.0-alpha.1` or agreed tag) |
| — | Registry discover / draft | *not written* | `planned` | `not-started` | Post-alpha UX or alpha stable; see [component tracker design](../specs/2026-05-13-component-tracker-design.md) |

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
  → do not implement post-alpha UX tasks

ELSE IF order 3 gate not satisfied (public alpha not shipped)
  → wait; optional: help close remaining release plan tasks

ELSE IF order 3 implementation is not complete
  → execute 2026-05-24-post-alpha-ux-plan.md from Task 1

ELSE
  → pick next `planned` product plan (registry, export, etc.)
```

---

## Plan document PR order (docs only)

Merge these **documentation PRs** in sequence (separate from implementation task PRs):

| PR sequence | Branch | PR | Contents |
|-------------|--------|-----|----------|
| 1 | `docs/plans-roadmap` | #33 | This `README.md` + spec roadmap section |
| 2 | `docs/release-and-rollout-plan` | #32 (merged) | Release plan on `main` |
| 3 | `docs/post-alpha-ux-plan` | #34 | Post-alpha UX plan + links |

#32 merged the order 2 plan doc. After this PR (#33) merges, begin release **implementation** Task 1 when order 1 implementation is `complete`. Set order 3 doc status to `merged` when #34 lands.

---

## Related specs

- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — contracts, CLI names, distribution
- [Component tracker design](../specs/2026-05-13-component-tracker-design.md) — product scope and future surfaces
