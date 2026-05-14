# Architecture Evaluation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the best foundation architecture for `wax` by comparing `TS core + TS plugins`, `Go core + TS plugins`, and `Go core + Go plugins` across performance, plugin ergonomics, operational simplicity, and agent friendliness before foundation implementation begins.

**Architecture:** Phase 0 builds thin, comparable spikes rather than the real product. Each option must scan the same Compose fixture corpus, produce the same normalized JSON artifact shape, and report the same benchmark metrics so the ADR is evidence-based instead of speculative.

**Tech Stack:** TypeScript, Go, JSON artifacts, benchmark fixtures, timing/memory instrumentation, markdown ADRs

---

## File Structure

- Create: `docs/specs/2026-05-14-ts-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-go-plugins-option.md`
- Create: `docs/plans/2026-05-14-architecture-evaluation-plan.md`
- Create: `docs/adr/2026-05-14-foundation-architecture-decision.md`
- Create: `prototypes/fixtures/small/`
- Create: `prototypes/fixtures/medium/`
- Create: `prototypes/fixtures/messy/`
- Create: `prototypes/contracts/artifact.schema.json`
- Create: `prototypes/results/`
- Create: `prototypes/ts-core-ts-plugin/`
- Create: `prototypes/go-core-ts-plugin/`
- Create: `prototypes/go-core-go-plugin/`

## Scope

This plan delivers:
- three lightweight option specs
- one shared artifact contract
- one shared benchmark corpus
- three thin comparable spikes
- benchmark results for cold scan, warm scan, parsing, extraction, artifact writing, and peak memory
- plugin ergonomics notes for a trivial rule change
- one ADR with the selected direction

This plan intentionally defers:
- production-ready CLI implementation
- final package layout
- full registry authoring flows
- backend/API and web UI
- release packaging

## Decision Criteria

The ADR must answer these questions explicitly:
- Which option meets acceptable cold and warm scan thresholds on the benchmark corpus?
- Where is time spent: file walking, parsing, extraction, or artifact writing?
- Which option makes plugin iteration easiest for agents and contributors?
- Which option keeps operational complexity acceptable for v1?
- Which option leaves the cleanest path for future plugin evolution?

## Benchmark Rules

All three spikes must:
- read the same fixture corpus
- emit the same artifact contract
- measure the same metrics
- use the same comparison script
- implement the same minimal feature set:
  - composable declaration detection
  - invocation detection
  - slot lambda counting
  - modifier chain capture
  - resolved vs candidate usage classification

### Task 1: Write The Three Option Specs

**Files:**
- Create: `docs/specs/2026-05-14-ts-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-go-plugins-option.md`

- [ ] **Step 1: Write the TS core + TS plugins option spec**

Include:

```md
# TS Core + TS Plugins Option

## Runtime Shape
- TypeScript core
- in-process TypeScript plugins
- shared JSON artifact contract

## Benefits
- fastest plugin iteration
- strongest agent ergonomics
- simplest plugin API shape

## Risks
- scan throughput may degrade on large repos
- performance tuning may arrive earlier
```

- [ ] **Step 2: Write the Go core + TS plugins option spec**

Include:

```md
# Go Core + TS Plugins Option

## Runtime Shape
- Go core
- TypeScript plugin boundary
- shared JSON artifact contract

## Benefits
- faster core orchestration and file-heavy paths
- keeps plugin logic more agent-friendly

## Risks
- dual-runtime complexity
- likely loses the in-process plugin model
- release coordination gets harder
```

- [ ] **Step 3: Write the Go core + Go plugins option spec**

Include:

```md
# Go Core + Go Plugins Option

## Runtime Shape
- Go core
- Go plugins
- shared JSON artifact contract

## Benefits
- strongest runtime performance and packaging simplicity
- one runtime, one release story

## Risks
- slowest plugin iteration
- weaker ergonomics for rapid plugin API churn
```

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-05-14-ts-core-ts-plugins-option.md docs/specs/2026-05-14-go-core-ts-plugins-option.md docs/specs/2026-05-14-go-core-go-plugins-option.md
git commit -m "docs: add foundation architecture option specs"
```

### Task 2: Define The Shared Artifact Contract And Fixture Corpus

**Files:**
- Create: `prototypes/contracts/artifact.schema.json`
- Create: `prototypes/fixtures/small/`
- Create: `prototypes/fixtures/medium/`
- Create: `prototypes/fixtures/messy/`

- [ ] **Step 1: Write the artifact contract**

Use this shape:

```json
{
  "schemaVersion": 1,
  "snapshotId": "string",
  "status": "complete",
  "designSystemComponents": [],
  "localComponents": [],
  "usageSites": [],
  "diagnostics": [],
  "metrics": {
    "adoptionCoverageRatio": 0
  }
}
```

- [ ] **Step 2: Define the benchmark fixture rules**

Write a short README in the fixtures area containing:

```md
- small: 10-20 files, clean Compose usage
- medium: 100-200 files, mixed imports and wrappers
- messy: aliasing, slots, modifiers, repeated local compositions
```

- [ ] **Step 3: Add representative Kotlin fixture files**

Ensure each corpus includes examples for:
- direct DS usage
- local wrappers
- slot lambdas
- modifier chains
- candidate non-DS usages

- [ ] **Step 4: Commit**

```bash
git add prototypes/contracts prototypes/fixtures
git commit -m "docs: add shared benchmark contract and fixture corpus"
```

### Task 3: Build The TS Core + TS Plugin Spike

**Files:**
- Create: `prototypes/ts-core-ts-plugin/`

- [ ] **Step 1: Implement a thin scanner**

Requirements:
- read all `.kt` files in a corpus directory
- extract declarations, invocations, slot lambdas, and modifier chains
- classify usage as `resolved` or `candidate`
- emit the shared JSON artifact

- [ ] **Step 2: Add instrumentation**

Capture:
- total runtime
- parse/extract runtime
- artifact write runtime
- peak memory if the runtime exposes it easily

- [ ] **Step 3: Add a trivial plugin-change exercise**

Document one plugin rule edit:

```text
Add a new canonical symbol alias and rerun the spike.
```

Measure:
- code touched
- files touched
- time to make the change

- [ ] **Step 4: Record results**

Write:

`prototypes/results/ts-core-ts-plugin.json`

- [ ] **Step 5: Commit**

```bash
git add prototypes/ts-core-ts-plugin prototypes/results/ts-core-ts-plugin.json
git commit -m "feat: add ts core and plugin architecture spike"
```

### Task 4: Build The Go Core + TS Plugin Spike

**Files:**
- Create: `prototypes/go-core-ts-plugin/`

- [ ] **Step 1: Implement the Go core probe**

Requirements:
- orchestrate a scan over the same fixture corpus
- delegate extraction to a TS plugin boundary
- emit the shared JSON artifact

- [ ] **Step 2: Add instrumentation**

Capture the same metrics as Task 3.

- [ ] **Step 3: Add the same trivial plugin-change exercise**

Use the same alias-addition change and record:
- code touched
- files touched
- time to make the change

- [ ] **Step 4: Record results**

Write:

`prototypes/results/go-core-ts-plugin.json`

- [ ] **Step 5: Commit**

```bash
git add prototypes/go-core-ts-plugin prototypes/results/go-core-ts-plugin.json
git commit -m "feat: add go core ts plugin architecture spike"
```

### Task 5: Build The Go Core + Go Plugin Spike

**Files:**
- Create: `prototypes/go-core-go-plugin/`

- [ ] **Step 1: Implement the all-Go probe**

Requirements:
- scan the same fixture corpus
- extract the same facts
- emit the shared JSON artifact

- [ ] **Step 2: Add instrumentation**

Capture the same metrics as Tasks 3 and 4.

- [ ] **Step 3: Add the same trivial plugin-change exercise**

Use the same alias-addition change and record:
- code touched
- files touched
- time to make the change

- [ ] **Step 4: Record results**

Write:

`prototypes/results/go-core-go-plugin.json`

- [ ] **Step 5: Commit**

```bash
git add prototypes/go-core-go-plugin prototypes/results/go-core-go-plugin.json
git commit -m "feat: add go core go plugin architecture spike"
```

### Task 6: Compare Results And Write The ADR

**Files:**
- Create: `docs/adr/2026-05-14-foundation-architecture-decision.md`

- [ ] **Step 1: Build the comparison table**

Include these columns:

```md
| Option | Cold Scan | Warm Scan | Parse/Extract | Artifact Write | Peak Memory | Plugin Change Effort | Operational Complexity |
```

- [ ] **Step 2: Write the decision record**

The ADR must contain:
- chosen option
- rejected options
- benchmark summary
- plugin ergonomics summary
- operational tradeoff summary
- explicit reasons for the decision

- [ ] **Step 3: Update the parked Phase 1 plan**

Edit:

`docs/plans/2026-05-13-foundation-cli-compose-plan.md`

Replace any stale stack assumptions with the chosen architecture.

- [ ] **Step 4: Commit**

```bash
git add docs/adr/2026-05-14-foundation-architecture-decision.md docs/plans/2026-05-13-foundation-cli-compose-plan.md prototypes/results
git commit -m "docs: record foundation architecture decision"
```

## Spec Coverage Check

- speed and filesystem concerns: covered by Tasks 2 through 6
- plugin ergonomics and agent friendliness: covered by Tasks 3 through 6
- TS, Go+TS, and Go+Go options: covered by Tasks 1 through 5
- ADR before execution: covered by Task 6

## Placeholder Scan

This plan intentionally avoids prescribing full production code. The spikes are comparative probes, not the product implementation.

## Type Consistency Check

Use the same artifact contract and metric names across all three spikes:
- `schemaVersion`
- `snapshotId`
- `status`
- `designSystemComponents`
- `localComponents`
- `usageSites`
- `diagnostics`
- `adoptionCoverageRatio`
