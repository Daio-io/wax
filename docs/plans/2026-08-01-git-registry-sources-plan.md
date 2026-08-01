# Git registry sources implementation plan

**Goal:** Add the config-v2 contract for Git-backed language registries while
keeping fetching, caching, lock pinning, and CLI wiring in follow-on tasks.

## Task 1: Config + schema for git / tag

- [x] Define exhaustive `PathOrUrl` and `Git` runtime registry variants with
  field-specific validation.
- [x] Update current path/URL callers to fail explicitly for Git sources until
  Git resolution is wired.
- [x] Add the Git registry alternative to the JSON Schema and focused tests.
- [x] Add parser coverage for valid values, invalid fields, mixed modes,
  unknown fields, and existing registry forms.
- [x] Update README, config specification, and representative fixtures.
- [x] Run the focused formatting, contract, config, load, and clippy checks.

Follow-on resolver and lock semantics are tracked separately by THE-279 through
THE-281.
