# Phase 2B — explicit resolution evidence and callee origin

> **Archived:** Historical record only. Shipped in #293 (THE-310); see the [resolution evidence ADR](../../adr/2026-08-22-resolution-evidence-and-callee-origin.md). Work is now tracked in the issue tracker.

## Status

- Doc status: `merged`
- Implementation status: `in-progress`

## Scope

Add explicit resolution evidence and callee-origin fields to scan facts and
usage sites across the parser-backed language packs. Preserve observed import
symbols in qualified identities, including aliases and package mismatches.

## Verification

Run the focused language-pack tests and the workspace formatting and test gates
before closing the implementation task.
