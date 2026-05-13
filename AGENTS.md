# AGENTS.md

This repository is still in the planning and foundation stage. Until project-specific implementation guidance exists, use these default working rules.

## Scope

- Treat the approved spec and implementation plan as the current source of truth.
- Prefer small, reviewable changes over broad speculative refactors.
- Keep the repo easy to inspect and easy for agents to reason about.

## Git

- Do not rewrite history unless explicitly asked.
- Do not amend commits unless explicitly asked.
- Keep commits focused and explainable.
- Do not commit unrelated changes together.
- If the working tree is dirty, understand the overlap before editing shared files.
- Prefer draft PRs while plans and scaffolding are still moving.

## TypeScript

- Prefer strict typing over loose `any`-driven code.
- Keep modules small and responsibility-focused.
- Prefer plain data structures and explicit interfaces at package boundaries.
- Use runtime validation for config and persisted artifacts.
- Avoid premature abstractions until at least two real call sites need them.

## Tooling

- Prefer `pnpm` for workspace and package commands.
- Prefer `vitest` for tests.
- Prefer fast local commands that can run package-scoped during iteration.
- Keep the dependency surface modest unless there is a clear payoff.

## Planning And Execution

- Do not start implementation from the spec alone when a reviewed plan exists.
- Update plans and specs when decisions materially change.
- Treat parser spikes, schema contracts, and artifact shapes as review gates, not assumptions.

## Repo Conventions

- Keep generated working state out of git unless intentionally recorded.
- Default repo-local tool state belongs under `.wax/`.
- Prefer JSON for config and persisted artifacts unless a later decision supersedes it.
- Store implementation plans under `docs/plans/`.

## Communication

- State assumptions when they affect architecture or persisted formats.
- Surface blockers early if a change would force a stack or schema decision that has not been reviewed.
- When in doubt, optimize for traceability and deterministic behavior.
