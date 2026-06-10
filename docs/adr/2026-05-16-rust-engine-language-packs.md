# ADR: Rust engine with downloadable language packs

**Status:** Accepted (implemented)  
**Date:** 2026-05-16  
**Type:** Addendum (foundation direction; does not supersede [component tracker design](../specs/2026-05-13-component-tracker-design.md))  
**Related:** [Language packs and distribution spec](../specs/2026-05-16-language-packs-and-distribution.md) · [Archived implementation plan](../plans/archive/2026-05-16-rust-engine-language-packs-plan.md)

## Context

`wax` is a design-system analysis engine that scans repositories, normalizes usage facts, and produces adoption and drift reports. The product must support multiple UI ecosystems (Jetpack Compose, React, Swift later) without coupling parser evolution to the reporting kernel.

Phase 0 evaluated alternative engine implementations using source fixtures, golden outputs, and benchmark-oriented spikes. Two directions were compared:

| Direction | Phase 0 finding |
|-----------|-----------------|
| **TypeScript core + TS extractors** | Lowest install friction in spikes; single-language ergonomics were strong. |
| **Go core + per-language tooling** | Viable performance profile; ecosystem boundaries were clearer than a monolithic TS core but still mixed orchestration and extraction concerns. |

The provisional Phase 0 conclusion favored **TS + TS** for friction. That path risked blurring the long-term boundary: every new ecosystem could pull parser and runtime concerns into the same package, making the shared `ScanFacts` contract harder to keep stable as languages diverge.

The approved product direction (documented in the language packs spec) is a **small Rust kernel** plus **optional, downloadable native language packs** that communicate over a versioned stdio JSON protocol and return normalized `ScanFacts`.

Production code lives under [`engine/`](../../engine/).

## Decision

We adopt a **Rust analysis engine** (`wax` binary, `wax-core` crate) with **downloadable language packs** (`wax-lang-<id>` binaries) as the v1 foundation.

1. **Kernel responsibilities:** orchestrate `scan`, load `.waxrc`, resolve global pack installs, spawn pack subprocesses, merge `ScanFacts`, compute adoption metrics, and write repo-local scan artifacts. The kernel owns reporting semantics; packs emit facts only.
2. **Pack responsibilities:** discover source for one stack, parse, resolve design-system registry symbols, and return `ScanFacts` over the v1 wire protocol (one JSON request on stdin, one JSON response on stdout).
3. **Distribution:** users install a prebuilt `wax` binary and download packs from a pack index (`WAX_LANG_INDEX`) into `~/.wax/langs/<id>/<version>/`. Repositories pin packs with `wax.lock.json` when using language packs in CI.
4. **Contract boundary:** `wax-contract` defines `ScanFacts`, `LanguageId`, `MergedScan`, and schema versioning; `wax-lang-api` defines in-process and wire protocol types shared by engine and packs.

This ADR records the foundation choice. Operational details (registry format, signing, release matrix, threat model) remain in the language packs spec and follow-on plan tasks.

### Spec decisions incorporated here

The following decisions from spec review are part of this foundation and were implemented in the [archived implementation plan](../plans/archive/2026-05-16-rust-engine-language-packs-plan.md):

| Decision | Choice |
|----------|--------|
| Repository config | JSON-only `.waxrc` for v1 |
| Reproducible CI | `wax.lock.json` required when using language packs |
| Swift | Deferred to a later phase |
| Large scan responses | No fixed size cap; engine must handle large payloads safely |
| Pack artifact trust (v1) | HTTPS + sha256 digest verification + lockfile pins — see spec [§ Pack distribution trust model](../specs/2026-05-16-language-packs-and-distribution.md#pack-distribution-trust-model-v1) |
| Pack signing (v1.1) | Sigstore/cosign planned; not required for v1 — see spec [§ Planned v1.1 signing](../specs/2026-05-16-language-packs-and-distribution.md#planned-v11-signing-sigstore--cosign) |

### Deferred: kernel plugins

**Plugins** (kernel hooks for export formatters, custom rules, fact transforms) are **explicitly out of scope** for this ADR and for v1 implementation. Language extraction is **not** implemented as a plugin; it uses **language packs** only.

A separate ADR will be written when kernel plugin loading, trust boundaries, and API stability requirements are defined. Until then, the spec reserves the term **plugin** for future kernel extensions and uses **language pack** for ecosystem extractors.

## Consequences

### Positive

- Clear separation between orchestration (Rust kernel) and ecosystem-specific parsing (native packs).
- Typed `ScanFacts` contract with schema versioning supports CI reproducibility and cross-language merged reports.
- Prebuilt binaries preserve a low-friction install path comparable to Phase 0 TS spikes without monolithing parsers into the engine.
- New ecosystems ship as new `wax-lang-<id>` artifacts without recompiling the kernel for every parser change.

### Negative / trade-offs

- Users download multiple artifacts (`wax` plus each enabled pack) instead of a single npm-style bundle.
- v1 targets macOS/Linux triples only; Windows packs are deferred.
- Pack trust in v1 relies on transport and digest checks, not code signing (planned for v1.1).
- Rust toolchain is required for engine/pack development; end users consume prebuilt releases.

## Implementation summary

All 18 tasks in the foundation plan shipped on `main`:

| Area | What shipped |
|------|----------------|
| Contracts | `wax-contract` (`ScanFacts`, schema versioning), `wax-lang-api` wire protocol v1 |
| Config | `.waxrc` parser, `wax.lock.json` with language pins |
| Language packs | `wax-lang-compose` (tree-sitter Kotlin), `wax-lang-basic` (line scanner), `wax-lang-react` stdio skeleton |
| Engine | `Engine::scan_repo`, bounded concurrency, atomic `.wax/out/` writes |
| Global install | `~/.wax/langs/`, pack index client, sha256-verified install, auto-install policy |
| CLI | `wax init`, `wax scan` orchestration hooks, `wax language {list,install,uninstall,update,doctor}` |
| Docs | ADR addendum, component-tracker terminology cleanup, release sketch, pack trust model in spec |
| Cleanup | `rust-prototype/` removed |

Follow-on work moved to later ADRs: [alpha release](./2026-05-24-alpha-release-and-distribution.md), [registry layout](./2026-06-02-registry-sources-and-wax-layout.md), [registry discovery](./2026-06-04-registry-discovery.md), [React pack](./2026-06-07-react-language-pack.md).

## References

- [Language packs, configuration, and distribution](../specs/2026-05-16-language-packs-and-distribution.md) — authoritative v1 spec
- [Archived Rust engine implementation plan](../plans/archive/2026-05-16-rust-engine-language-packs-plan.md) — phased delivery record
- Phase 0 evaluation summary: spec § Background: architecture evaluation (spike artifacts not committed to this repository)
