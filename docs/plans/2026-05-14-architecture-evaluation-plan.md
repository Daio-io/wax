# Architecture Evaluation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the best foundation architecture for `wax` by comparing `TS core + TS plugins`, `Go core + TS plugins`, and `Go core + Go plugins` across correctness, performance, parser viability, plugin ergonomics, plugin distribution, install ergonomics, operational simplicity, and agent friendliness before foundation implementation begins.

**Architecture:** Phase 0 builds thin, comparable spikes rather than the real product. Each option must scan the same Compose fixture corpus, produce the same normalized JSON artifact shape, and report the same benchmark metrics so the ADR is evidence-based instead of speculative.

**Tech Stack:** TypeScript, Go, JSON artifacts, benchmark fixtures, timing/memory instrumentation, markdown ADRs

---

## File Structure

- Create: `docs/specs/2026-05-14-ts-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-go-plugins-option.md`
- Create: `docs/specs/2026-05-14-ts-core-native-parser-helper-option.md`
- Create: `docs/plans/2026-05-14-architecture-evaluation-plan.md`
- Create: `docs/adr/2026-05-14-foundation-architecture-decision.md`
- Create: `prototypes/fixtures/small/`
- Create: `prototypes/fixtures/medium/`
- Create: `prototypes/fixtures/messy/`
- Create: `prototypes/fixtures/large/`
- Create: `prototypes/fixtures/README.md`
- Create: `prototypes/contracts/artifact.schema.json`
- Create: `prototypes/contracts/golden/`
- Create: `prototypes/results/`
- Create: `prototypes/ts-core-ts-plugin/`
- Create: `prototypes/go-core-ts-plugin/`
- Create: `prototypes/go-core-go-plugin/`
- Create: `prototypes/tools/compare-artifacts/`

## Scope

This plan delivers:
- three primary lightweight option specs
- one paper-only fourth option note for `TS core + native parser helper`
- one shared artifact contract
- one shared benchmark corpus
- three thin comparable spikes
- golden artifact correctness checks before timings are accepted
- benchmark results for process startup, cold process scans, warm process incremental scans, parsing, extraction, artifact writing, and peak RSS
- plugin ergonomics notes for a simple alias change and a harder fact-type change
- plugin distribution notes for first-party and third-party plugin delivery
- install ergonomics notes with setup friction, runtime prerequisites, install timing, first-run timing, and binary/dependency footprint
- one ADR with the selected direction

The prototype work in this plan is evidence-gathering. Do not assume every spike artifact must be committed. Commit the plan, option specs, final ADR, and any intentionally retained benchmark fixtures/results; keep throwaway spike work local unless the ADR needs it preserved for review.

This plan intentionally defers:
- production-ready CLI implementation
- final package layout
- full registry authoring flows
- backend/API and web UI
- release packaging

## Spike Time Budget

Each primary spike has a maximum budget of 3 working days. If a spike is incomplete at the budget limit, stop, record what works, record what failed, and include that as evidence in the ADR. The ADR may decide that none of the options are acceptable if every spike misses correctness, install, or performance thresholds.

## Decision Criteria

The ADR must answer these questions explicitly:
- Which option produces the correct normalized artifacts for the shared fixtures?
- Which option meets acceptable startup, cold scan, and warm incremental scan thresholds on the benchmark corpus?
- Where is time spent: file walking, parsing, extraction, or artifact writing?
- Which parser strategy is used in each option, and is parser cost in scope for the benchmark?
- Which option makes plugin iteration easiest for agents and contributors?
- Which option has the most credible first-party and third-party plugin distribution story?
- Which option has the lowest installation and setup friction for first-time users and contributors?
- Which option keeps operational complexity acceptable for v1?
- Which option leaves the cleanest path for future plugin evolution?
- Are all options unacceptable, requiring a smaller follow-up evaluation before Phase 1?

Suggested ADR weighting:
- correctness is a pass/fail gate
- parser viability is a pass/fail gate
- performance and install ergonomics are high weight
- plugin ergonomics and agent friendliness are high weight because plugin iteration is expected to change fastest
- operational simplicity is medium weight for v1 but becomes higher weight before release packaging

## Parser Strategy

Parsing is in scope for this evaluation because parser choice dominates both correctness and scan runtime.

Rules:
- each option spec must name its parser and binding strategy before spike implementation starts
- timed results are valid only if the spike uses a real Kotlin parser strategy, not a hand-rolled regex parser
- the preferred fair comparison is tree-sitter Kotlin across runtimes where bindings are viable
- if an option cannot use an equivalent parser binding, that limitation must be recorded as an architecture finding, not hidden by substituting a weaker parser
- parser startup, parser initialization, and parse time must be measured separately where practical

## Option 2 Boundary Decision

For the `Go core + TS plugins` spike, use a long-lived TypeScript plugin subprocess with newline-delimited JSON messages over stdio.

This boundary is intentionally selected for the spike because:
- it is portable across local development and CI
- it avoids embedding a JavaScript runtime into Go for v1
- it exposes the real dual-runtime install and lifecycle costs
- it keeps third-party TS plugin distribution plausible

The spike must measure:
- Go process startup
- TS plugin process startup
- first request latency
- steady-state scan latency after the TS subprocess is ready
- failure behavior when the plugin process exits with a non-zero status

## Plugin Distribution Criteria

Each option spec must explain how plugins are distributed and loaded for:
- first-party plugins shipped with `wax`
- third-party plugins installed by users
- local development plugins

Examples to evaluate:
- TS packages installed with `npm` or `pnpm`
- Go modules compiled into the main binary
- Go sidecar binaries discovered by path or config
- WASM as a future option, if rejected for v1

Go `.so` plugin loading should not be treated as the default unless the option spec explicitly addresses cross-platform support.

Plugin distribution is evaluated as design evidence in Phase 0, not as a benchmarked implementation. The ADR must label distribution conclusions as design judgement unless a spike actually exercises the distribution path.

## Benchmark Rules

All three spikes must:
- read the same fixture corpus
- emit the same artifact contract
- measure the same metrics
- use the same comparison script
- document install steps and prerequisites from a clean machine perspective
- pass golden artifact equality checks before timing results are accepted
- run on the same benchmark machine with the same command harness
- repeat each timing case at least 5 times and report median plus min/max
- implement the same minimal feature set:
  - composable declaration detection
  - invocation detection
  - slot lambda counting
  - modifier chain capture
  - resolved vs candidate usage classification

Benchmark modes:
- `startup`: process start until ready to accept a scan request
- `cold-process-warm-fs`: new process scanning files that may be in OS cache, representative of CI after checkout
- `warm-process-incremental-hit`: same process rescanning unchanged files with file-hash cache hits
- `warm-process-incremental-change`: same process rescanning after one changed Kotlin file

`cold-process-cold-fs` is intentionally excluded from Phase 0 because reliable cross-platform filesystem cache eviction adds measurement friction and weakens repeatability. Revisit it only if the selected architecture later shows suspicious filesystem-bound behavior.

Memory:
- peak RSS must be measured with a comparable external tool such as `/usr/bin/time -l` on macOS or `/usr/bin/time -v` on Linux
- runtime-internal memory counters may be recorded as secondary diagnostics only

Noise control:
- run benchmarks on one named machine profile
- close unrelated heavy processes where practical
- record OS, CPU, memory, Node version, Go version, and package manager versions
- do not compare results from different machines in the ADR table

Correctness:
- each fixture tier must have a golden artifact or golden summary
- a spike that does not match the golden output is marked incorrect and its timing numbers cannot be used to select the architecture
- tolerated differences must be explicitly listed, such as ordering differences after deterministic sorting
- diagnostic equivalence is not required unless diagnostics are part of the tested normalized facts; different parsers may emit different but acceptable diagnostics for the same source

### Task 1: Write The Three Option Specs

**Files:**
- Create: `docs/specs/2026-05-14-ts-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-ts-plugins-option.md`
- Create: `docs/specs/2026-05-14-go-core-go-plugins-option.md`
- Create: `docs/specs/2026-05-14-ts-core-native-parser-helper-option.md`

- [ ] **Step 1: Write the TS core + TS plugins option spec**

Include:

```md
# TS Core + TS Plugins Option

## Runtime Shape
- TypeScript core
- in-process TypeScript plugins
- shared JSON artifact contract
- parser strategy named before spike implementation
- plugins distributed as npm-compatible packages for third-party use

## Benefits
- fastest plugin iteration
- strongest agent ergonomics
- simplest plugin API shape

## Risks
- scan throughput may degrade on large repos
- performance tuning may arrive earlier

## Distribution
- first-party plugins ship in the workspace/package
- third-party plugins are npm packages
- local plugins can be loaded by package name or path in a future phase
```

- [ ] **Step 2: Write the Go core + TS plugins option spec**

Include:

```md
# Go Core + TS Plugins Option

## Runtime Shape
- Go core
- TypeScript plugin subprocess using newline-delimited JSON over stdio
- shared JSON artifact contract
- parser strategy named before spike implementation

## Benefits
- faster core orchestration and file-heavy paths
- keeps plugin logic more agent-friendly

## Risks
- dual-runtime complexity
- likely loses the in-process plugin model
- release coordination gets harder

## Distribution
- Go core ships as the host
- first-party TS plugins ship with the repo/package
- third-party TS plugins require a package install plus config discovery
```

- [ ] **Step 3: Write the Go core + Go plugins option spec**

Include:

```md
# Go Core + Go Plugins Option

## Runtime Shape
- Go core
- Go plugins
- shared JSON artifact contract
- parser strategy named before spike implementation

## Benefits
- strongest runtime performance and packaging simplicity
- one runtime, one release story

## Risks
- slowest plugin iteration
- weaker ergonomics for rapid plugin API churn

## Distribution
- first-party plugins are compiled into the main binary for v1
- third-party plugins are not supported as Go `.so` plugins in v1
- future third-party distribution would require sidecar binaries, WASM, or a separate extension mechanism
```

- [ ] **Step 4: Write the TS core + native parser helper option note**

Include:

```md
# TS Core + Native Parser Helper Option

## Runtime Shape
- TypeScript core and plugin ergonomics
- native helper used only for parser/file-index hot paths
- shared JSON artifact contract

## Why This Is Paper-Only For Phase 0
- it may dominate if TS ergonomics are best but parser throughput is not
- it adds another implementation axis beyond the three primary options
- evaluate as a fallback if TS+TS loses only on parser/file-walk performance

## Decision Rule
Consider this follow-up only if TS+TS has the best plugin ergonomics but misses performance thresholds for parser-bound reasons.
```

- [ ] **Step 5: Review option specs**

Confirm each option spec names its parser strategy, plugin distribution story, install assumptions, and known risks before spike implementation starts.

### Task 2: Define The Shared Artifact Contract And Fixture Corpus

**Files:**
- Create: `prototypes/contracts/artifact.schema.json`
- Create: `prototypes/contracts/golden/`
- Create: `prototypes/fixtures/README.md`
- Create: `prototypes/fixtures/small/`
- Create: `prototypes/fixtures/medium/`
- Create: `prototypes/fixtures/messy/`
- Create: `prototypes/fixtures/large/`

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
  },
  "registry": {
    "designSystemSymbols": []
  }
}
```

- [ ] **Step 2: Define the benchmark fixture rules**

Write a short README in the fixtures area containing:

```md
- small: 10-20 files, clean Compose usage
- medium: 100-200 files, mixed imports and wrappers
- messy: 100-200 files with aliasing, slots, modifiers, repeated local compositions, and intentionally awkward cases
- large: generated corpora at 5k, 25k, and 50k Kotlin files using the same pattern families as the messy tier
```

Each fixture tier must include a hand-authored registry JSON file that defines canonical DS symbols. `resolved` and `candidate` usage classification is undefined without that registry.

- [ ] **Step 3: Add representative Kotlin fixture files**

Ensure each corpus includes examples for:
- direct DS usage
- local wrappers
- slot lambdas
- modifier chains
- candidate non-DS usages
- deprecated component replacement examples
- token reference and hardcoded styling examples

- [ ] **Step 4: Add golden expected artifacts**

For each fixture tier, write a deterministic expected artifact or expected summary under:

`prototypes/contracts/golden/`

The golden files must include:
- expected design system components
- expected local components
- expected usage site count
- expected resolved usage count
- expected candidate usage count
- expected modifier chain count
- expected slot lambda count

For generated large fixtures, the golden output may be a summary derived from the generator seed rather than a full committed artifact.

- [ ] **Step 5: Add the large corpus generator contract**

The large tier must be generated rather than hand-authored. The generator contract must support:
- `--files 5000`
- `--files 25000`
- `--files 50000`
- deterministic output from a seed
- a mix of resolved and candidate usage
- documented per-pattern proportions

The large generator must scale the messy-tier patterns rather than emit mostly trivial files. Include proportions for at least:
- direct DS usage
- local wrappers
- slot lambdas
- modifier chains
- aliased imports
- candidate non-DS usages
- deprecated replacements
- token references and hardcoded styling values

- [ ] **Step 6: Review fixture corpus**

Confirm the fixture corpus, registry files, and golden summaries are sufficient to compare correctness before any timing results are accepted.

### Task 3: Build The Benchmark Harness And Correctness Gate

**Files:**
- Create: `prototypes/tools/compare-artifacts/`
- Create: `prototypes/results/benchmark-machine.json`

- [ ] **Step 1: Implement artifact comparison**

Requirements:
- load a spike artifact and golden artifact
- sort unordered arrays deterministically before comparison
- fail if required counts or normalized facts differ
- print a concise diff summary

- [ ] **Step 2: Implement benchmark-machine capture**

Record:
- OS and version
- CPU model
- memory
- Node version
- Go version
- package manager versions
- benchmark date

- [ ] **Step 3: Define the timing command contract**

Every spike must expose the same commands:

```bash
run startup
run scan --mode cold-process-warm-fs --fixture prototypes/fixtures/small
run scan --mode warm-process-incremental-hit --fixture prototypes/fixtures/medium
run scan --mode warm-process-incremental-change --fixture prototypes/fixtures/large/5000
run scan --mode cold-process-warm-fs --fixture prototypes/fixtures/large/25000
run scan --mode cold-process-warm-fs --fixture prototypes/fixtures/large/50000
```

- [ ] **Step 4: Review harness output**

Confirm the comparison tool rejects incorrect artifacts and the benchmark machine metadata is captured before running architecture spikes.

### Task 4: Build The TS Core + TS Plugin Spike

**Files:**
- Create: `prototypes/ts-core-ts-plugin/`

- [ ] **Step 1: Implement a thin scanner**

Requirements:
- read all `.kt` files in a corpus directory
- use the parser strategy named in the option spec
- extract declarations, invocations, slot lambdas, and modifier chains
- classify usage as `resolved` or `candidate`
- emit the shared JSON artifact

- [ ] **Step 2: Add instrumentation**

Capture:
- total runtime
- process startup runtime
- parse/extract runtime
- artifact write runtime
- peak RSS using the shared external measurement command
- file count and changed-file count

- [ ] **Step 3: Add correctness gate**

Run the artifact comparison against the golden files before recording timings.

- [ ] **Step 4: Add plugin-change exercises**

Document two plugin rule edits:

```text
Add a new canonical symbol alias and rerun the spike.
Add a new fact type, references_token, that requires extending the artifact, extractor, and reader.
```

Measure:
- code touched
- files touched
- time to make the change

- [ ] **Step 5: Record install ergonomics**

Capture:
- required runtimes and versions
- install commands
- native dependency friction if any
- cold install time
- installed dependency size
- macOS, Linux, and Windows friction notes where observable
- offline or air-gapped install notes
- time to first successful run
- any manual setup or troubleshooting required

Write:

`prototypes/results/ts-core-ts-plugin-install.json`

- [ ] **Step 6: Record results**

Write:

`prototypes/results/ts-core-ts-plugin.json`

- [ ] **Step 7: Record review evidence**

Record the correctness result, timing summary, install findings, and plugin-change notes for the ADR. Keep the spike code local unless preserving it is necessary to explain the decision.

### Task 5: Build The Go Core + TS Plugin Spike

**Files:**
- Create: `prototypes/go-core-ts-plugin/`

- [ ] **Step 1: Implement the Go core probe**

Requirements:
- orchestrate a scan over the same fixture corpus
- delegate extraction to a long-lived TS plugin subprocess using newline-delimited JSON over stdio
- emit the shared JSON artifact
- measure plugin subprocess startup separately from scan time

- [ ] **Step 2: Add instrumentation**

Capture the same metrics as the TS core + TS plugin spike.

- [ ] **Step 3: Add correctness gate**

Run the artifact comparison against the golden files before recording timings.

- [ ] **Step 4: Add the same plugin-change exercises**

Use the same alias-addition and `references_token` changes and record:
- code touched
- files touched
- time to make the change

- [ ] **Step 5: Record install ergonomics**

Capture:
- required runtimes and versions
- install commands
- cross-runtime coordination friction
- cold install time
- installed dependency size
- macOS, Linux, and Windows friction notes where observable
- offline or air-gapped install notes
- time to first successful run
- any manual setup or troubleshooting required

- [ ] **Step 6: Record results**

Write:

`prototypes/results/go-core-ts-plugin.json`

- [ ] **Step 7: Record review evidence**

Record the correctness result, timing summary, install findings, subprocess-boundary notes, and plugin-change notes for the ADR. Keep the spike code local unless preserving it is necessary to explain the decision.

### Task 6: Build The Go Core + Go Plugin Spike

**Files:**
- Create: `prototypes/go-core-go-plugin/`

- [ ] **Step 1: Implement the all-Go probe**

Requirements:
- scan the same fixture corpus
- use the parser strategy named in the option spec
- extract the same facts
- emit the shared JSON artifact

- [ ] **Step 2: Add instrumentation**

Capture the same metrics as the TS core + TS plugin and Go core + TS plugin spikes.

- [ ] **Step 3: Add correctness gate**

Run the artifact comparison against the golden files before recording timings.

- [ ] **Step 4: Add the same plugin-change exercises**

Use the same alias-addition and `references_token` changes and record:
- code touched
- files touched
- time to make the change

- [ ] **Step 5: Record install ergonomics**

Capture:
- required runtimes and versions
- install commands
- toolchain/download friction
- cold install time
- built binary size
- macOS, Linux, and Windows friction notes where observable
- offline or air-gapped install notes
- time to first successful run
- any manual setup or troubleshooting required

- [ ] **Step 6: Record results**

Write:

`prototypes/results/go-core-go-plugin.json`

- [ ] **Step 7: Record review evidence**

Record the correctness result, timing summary, install findings, distribution notes, and plugin-change notes for the ADR. Keep the spike code local unless preserving it is necessary to explain the decision.

### Task 7: Compare Results And Write The ADR

**Files:**
- Create: `docs/adr/2026-05-14-foundation-architecture-decision.md`

- [ ] **Step 1: Build the comparison tables**

Use two tables so the ADR remains readable.

Performance table:

```md
| Option | Correctness | Startup | Cold Process Warm FS | Warm Incremental Hit | Warm Incremental Change | Parse/Extract | Artifact Write | Peak RSS |
```

Ergonomics table:

```md
| Option | Plugin Change Effort | Plugin Distribution | Install Friction | Agent Friendliness | Operational Complexity |
```

- [ ] **Step 2: Write the decision record**

The ADR must contain:
- chosen option
- rejected options
- benchmark summary
- parser strategy summary
- plugin ergonomics summary
- plugin distribution summary
- install ergonomics summary
- operational tradeoff summary
- paper-only `TS core + native parser helper` assessment
- explicit reasons for the decision
- explicit "none acceptable" decision if none of the three spikes clears the bar
- explicit statement on whether snapshot diff and incremental cache risks require a follow-up spike before Phase 1 implementation

- [ ] **Step 3: Write Phase 1 planning follow-up**

Do not edit a missing Phase 1 plan. Instead, create or update the next Phase 1 implementation plan after the ADR is approved.

- [ ] **Step 4: Review ADR evidence**

Confirm the ADR links or summarizes the evidence required to make the decision. Only intentionally retained fixtures, summaries, and final decision docs need to be prepared for review.

## Spec Coverage Check

- speed and filesystem concerns: covered by Tasks 2 through 7
- correctness before timing: covered by Tasks 2 through 7
- parser strategy: covered by Tasks 1 through 7
- plugin ergonomics and agent friendliness: covered by Tasks 4 through 7
- plugin distribution: covered by Tasks 1 through 7
- install ergonomics: covered by Tasks 4 through 7
- TS, Go+TS, and Go+Go options: covered by Tasks 1 through 6
- ADR before execution: covered by Task 7

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
- `registry.designSystemSymbols`
