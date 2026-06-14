# ADR: Interactive init wizard

**Status:** Accepted (implemented)
**Date:** 2026-06-13
**Type:** Addendum (CLI onboarding UX)
**Related:** [Design spec](../specs/2026-06-13-interactive-init-design.md) · [Archived implementation plan](../plans/archive/2026-06-13-interactive-init.md)

## Context

Wax shipped deterministic repository setup through `wax init --non-interactive --language <id>`, which suits CI and scripts but is terse for first-time local users. Post-alpha UX Task 1 called for a TTY wizard that guides language selection, scan roots, and registry setup without turning init into scan or discovery.

## Decision

Add an interactive `wax init` path that:

1. **Prompts only on TTY** — When stdin is not a terminal and `--non-interactive` is absent, fail with a scriptable message pointing at `--non-interactive --language <id>`.
2. **Collects `InitSelections`** — Language pack ids from the pack index, scan roots per language (persisted in `.wax/wax.config.json`), and whether registry source lives in-repo.
3. **Keeps init setup-only** — Does not run `wax scan` or `wax registry discover`. Registry source roots are used only to print follow-up discover commands; they are not persisted in config.
4. **Converges on existing init writes** — Interactive and non-interactive paths share `run_init` for config, lockfile, registry scaffolds, language install, and `.gitignore` updates.
5. **Uses `dialoguer` in `wax-cli`** — Prompt dependency stays local to the CLI crate; `InitPrompts` trait enables unit tests with mocked answers.

## Implementation summary

All 4 tasks shipped:

| Task | What shipped |
|------|----------------|
| Selection model | `InitSelections`, `RegistrySetup`, scan-root injection into config generation |
| Prompt adapter | `dialoguer` prompts, `write_next_steps` guidance, `collect_interactive_selections` |
| CLI wiring | `run_init_cli` with TTY detection, `init_interactive.rs` integration tests |
| Documentation | README wizard section, plan checkboxes, roadmap and spec updates |

## Consequences

### Positive

- Local users get guided setup for language packs, scan roots, and registry next steps.
- CI and scripts keep the existing non-interactive flag contract unchanged.
- Prompt logic is testable without a real terminal through the `InitPrompts` abstraction.

### Negative / trade-offs

- Interactive init requires a TTY; headless environments must use `--non-interactive`.
- Registry source roots are ephemeral (printed guidance only), so users must re-enter them or use CLI flags for discover.

## References

- [Interactive init design spec](../specs/2026-06-13-interactive-init-design.md)
- [Archived implementation plan](../plans/archive/2026-06-13-interactive-init.md)
- [Post-alpha UX plan, Task 1](../plans/2026-05-24-post-alpha-ux-plan.md#phase-1--guided-init)
- [Alpha release ADR](./2026-05-24-alpha-release-and-distribution.md)
