# Component Tracker Design

## Summary

This document defines the first design for an open source, self-hostable design system component tracker inspired by products like Omlet, but built around a pluggable analysis model. The first supported ecosystem is Jetpack Compose / Compose Multiplatform.

The product is designed to track:
- canonical design system components
- local non-design-system components built from those components
- adoption, composition, wrapper, drift, and reach relationships

The system should be useful without AI, fully runnable in local and CI environments, and extensible to future ecosystems through plugins. AI is treated as an optional ecosystem of external skills that help refine artifacts produced by the core tool, not as a dependency of the runtime.

## Goals

- Build a reusable analysis kernel that is not tied to Compose-specific semantics.
- Support Jetpack Compose / Compose Multiplatform as the first extraction plugin.
- Track both design system components and local composed components built from them.
- Produce reports and dashboards for design system maintainers.
- Support self-hosted OSS usage with CLI-first workflows and optional web reporting.
- Make the design system registry config versioned, inspectable, and repo-local.
- Help users bootstrap and refine the registry through deterministic CLI workflows.
- Provide stable artifacts that external AI skills can use to improve configuration and interpretation.

## Non-Goals

- Full runtime instrumentation in the first release.
- Perfect semantic resolution of all Kotlin patterns.
- Multi-tenant SaaS concerns in the first spec.
- IDE-specific AI workflows in the core product.
- Tracking every possible UI symbol in the codebase from day one.

## Primary Audience

The initial audience is design system maintainers. The system should optimize for:
- visibility into adoption across modules and apps
- visibility into wrappers and compositions built from DS primitives
- insight into drift, gaps, and low-usage components
- traceable, explainable reports rather than opaque heuristics

## Product Scope

The first spec focuses on the scanner core and extension model. It includes:
- a core analysis kernel
- a Compose plugin
- a CLI for discovery, registry authoring, scanning, validation, and export
- storage for scan snapshots and graph data
- a backend/API and simple report-oriented web UI

It does not attempt to fully solve every future plugin shape up front, but it does require clean plugin boundaries so later ecosystems can reuse the same kernel and reporting semantics.

## Architecture

The system is built as a layered analysis platform with strict separation between the core tracking logic and technology-specific extraction plugins.

### Analysis Kernel

The kernel is ecosystem-agnostic. It owns:
- scan orchestration
- plugin execution
- normalized graph ingestion
- persistence of scan snapshots
- metric and report calculation
- query APIs for CLI, API, and web consumers

The kernel must not embed Compose-specific meaning. It should define generic concepts such as:
- canonical components
- local components
- usage sites
- relationships
- snapshots
- diagnostics

The kernel is also where reporting semantics live. Plugins emit facts; the kernel interprets them into higher-level results such as wrappers, adoption rollups, drift signals, and reach analysis.

### Ecosystem Plugins

Plugins are responsible only for technology-specific extraction. A plugin should:
- understand source layout and file discovery for its ecosystem
- load and validate plugin-specific config sections
- parse source artifacts
- emit normalized graph entities and relationships
- emit structured diagnostics

The first plugin is for Jetpack Compose / Compose Multiplatform.

### User-Facing Surfaces

All user-facing interfaces should consume the same kernel behavior:
- CLI for setup, validation, scans, and exports
- API/backend for querying stored snapshots and graph state
- web UI for dashboards and drill-down views

No surface should reimplement business logic independently of the kernel.

## Data Model

The system should store a normalized graph rather than raw ad hoc scan output.

### Core Entities

#### DesignSystemComponent

A canonical tracked component declared in configuration, such as `Button` or `Card`.

Suggested fields:
- stable id
- canonical name
- aliases
- fully-qualified symbol match rule
- explicit alias list
- category
- version
- status
- parameter signatures
- optional metadata such as deprecation or tags

For v1, registry matching should be deterministic and explainable:
- canonical matching is by fully-qualified symbol name
- aliases are explicit additional symbol names
- regex and glob matching are out of scope for v1

Version must be first-class because monorepos often run multiple design system versions concurrently. Migration reporting depends on distinguishing them.

For v1, each versioned DS component is a distinct `DesignSystemComponent` node rather than a single node with a version dimension. That keeps `replaces` edges explicit and makes migration progress queries straightforward.

#### LocalComponent

A project-defined composable that is not itself part of the design system but is built from one or more design system components or other local components.

Examples:
- feature-specific wrappers
- branded variants
- app-level convenience composables
- repeated local patterns that may indicate gaps in the design system

This must be a first-class entity, not a tag on a usage record, because composition tracking depends on traversable component-to-component relationships.

For v1, `LocalComponent` identity is its fully-qualified symbol name. That gives snapshot diffs and trend reports a stable baseline. Rename-resilient identity based on structural hashing is deferred to a later version.

#### ParameterSignature

A normalized description of a DS component parameter surface used to reason about adoption, drift, and wrapper behavior.

Suggested fields:
- component id
- parameter name
- parameter type
- required vs defaulted
- slot parameter flag
- deprecation status

This is a first-class part of the schema because Compose drift is often visible in how a component is called, not only that it is called.

#### UsageSite

A concrete invocation or reference site for a component.

Suggested fields:
- snapshot id
- repository id
- module id
- file path
- symbol scope
- line or source range when available
- resolved target
- parameter bindings
- confidence class
- match status

Each `UsageSite` should carry one of these statuses:
- `resolved`: matched to a canonical DS component
- `candidate`: looks like a UI invocation that may represent non-DS or drift behavior
- `unknown`: scanned but not classified beyond basic detection

Confidence should be explicit rather than vague. For v1, confidence is lowered by:
- import alias indirection
- partially resolved symbol targets
- slot lambdas passed via variables rather than inline
- typealiases that obscure the final symbol
- `CompositionLocal`-based indirection

Low-confidence results must surface in diagnostics and report drill-downs rather than silently counting as high-certainty facts.

#### ParameterBinding

A normalized record of how a `UsageSite` binds a DS component parameter.

Suggested fields:
- parameter name
- binding kind
- literal value when safely serializable
- non-literal marker
- default-used marker
- referenced symbol when resolvable
- modifier chain entries when the bound parameter is `Modifier`

Binding kinds for v1:
- `literal`
- `non_literal`
- `default`
- `slot_content`
- `unknown`

`Modifier` bindings should be captured as an ordered sequence of modifier elements rather than a single opaque blob. This is required for meaningful drift analysis.

#### Repository

A source repository or codebase root that owns one or more reporting boundaries and scan snapshots.

#### Module

A generic reporting boundary in the kernel. In the Compose plugin this will usually map to a Gradle module, but the kernel must not assume Gradle semantics.

#### Team

A first-class ownership entity used for adoption and migration reporting where module boundaries do not align with organizational boundaries.

The ownership mapping source is intentionally pluggable. Team relationships may come from registry config, a separate ownership file, `CODEOWNERS`, or an external organizational export. The kernel only commits to the `owns` relationship, not to a single source of truth for how that mapping is provided.

#### ScanSnapshot

A versioned record of one scan run used for current-state and historical reporting.

Suggested fields:
- snapshot id
- scan timestamp
- repo revision when available
- plugin versions
- cache metadata
- status
- diagnostic summary

Snapshot diffing is a first-class kernel capability in v1, not future work. The kernel must support comparing a baseline snapshot and a head snapshot using stable entity identities.

#### Token

A design token reference used to attribute styling decisions such as color, spacing, typography, shape, or elevation.

This is required in the schema from v1 even if token-focused reports arrive later. Hardcoded styling versus token usage is a primary drift signal.

### Core Relationships

The kernel should support at least these normalized relationships:
- `uses`
- `composes`
- `declared_in`
- `depends_on`
- `replaces`
- `owns`
- `references_token`
- `hardcodes_value`

Interpretation:
- `uses`: a local component or usage site invokes a DS component
- `composes`: a local component is built from one or more DS or local components
- `declared_in`: an entity belongs to a module, file, or symbol container
- `depends_on`: rollup relationship useful for module-level analysis
- `replaces`: one DS component version or symbol supersedes another for migration tracking
- `owns`: a team owns a component, module, or repository boundary
- `references_token`: a usage site or component styling decision references a known design token
- `hardcodes_value`: a usage site or component styling decision hardcodes a value where token alignment might be expected

`wraps` should not be a plugin-emitted edge in v1. Wrapper detection is a kernel-derived annotation computed from composition facts, parameter bindings, and customization heuristics.

### Why A Graph

This graph model enables:
- adoption reporting
- wrapper and drift heuristics
- identification of foundational DS components
- drill-down from canonical DS components to local compositions that depend on them
- future cross-ecosystem reporting using the same semantics

## Plugin Contract

The plugin contract should be narrow and stable. Plugins should emit normalized facts, not interpretive reports.

### Required Plugin Capabilities

- metadata
- registry loader
- target discovery
- extractor
- diagnostics emitter

### Metadata

Metadata should include:
- plugin name
- plugin version
- supported ecosystem
- declared capabilities

### Registry Loader

Loads plugin-specific config and returns a validated registry definition plus any warnings.

### Target Discovery

Identifies:
- source roots
- modules
- candidate files
- excludes and ignored areas

### Extractor

Parses source and emits:
- all composable invocation sites
- DS component references
- local component declarations
- component-to-component composition links
- usage sites
- parameter bindings
- token references or hardcoded styling facts when detectable
- unresolved or ambiguous matches

Each emitted invocation site must carry a classification status so the kernel can compute honest coverage and adoption ratios.

### Diagnostics Emitter

Emits structured warnings and errors such as:
- unsupported source constructs
- parse failures
- ambiguous symbol matches
- unknown repeated symbols
- invalid registry entries

### Contract Rule

Plugins emit facts like:

`LocalComponent FeatureButtonRow composes DesignSystemComponent Button`

They do not emit conclusions like:

`FeatureButtonRow is a wrapper candidate`

Wrapper detection, drift classification, and other higher-level semantics remain kernel responsibilities.

## Compose Plugin Design

The first plugin targets Jetpack Compose / Compose Multiplatform with a source-only scanning approach optimized for predictable CI and local execution.

### Compose Plugin Responsibilities

- read the DS registry config
- discover Kotlin source files
- identify composable declarations
- identify all composable invocation sites
- match invocations to registry entries
- detect local components built from DS primitives
- capture parameter bindings and slot usage
- capture token references and hardcoded styling facts when possible
- emit normalized graph facts

### Compose Scan Flow

1. Load and validate the DS registry.
2. Discover configured Kotlin source files and modules.
3. Parse source files with a Kotlin source parser.
4. Identify composable declarations and call sites.
5. Emit a `UsageSite` record for every composable invocation with `resolved`, `candidate`, or `unknown` status.
6. Match call targets against the canonical DS registry.
7. Capture parameter bindings including slot lambdas and `Modifier` chains.
8. Build local component composition records for non-DS composables that use DS components.
9. Record token references or hardcoded styling values where source analysis can detect them.
10. Emit entities, edges, annotations, and diagnostics to the kernel.
11. Persist the snapshot, compute rollups, and make baseline-vs-head diffs queryable in the kernel.

### Slot Content Handling

Compose slot APIs must be handled explicitly. For v1:
- a DS component invoked inside an inline slot lambda counts as `uses` from the enclosing local component or screen that provides the slot content
- the slot-owning DS component does not inherit that nested usage as its own child usage fact
- slot lambdas passed through variables are still captured, but confidence is reduced if the scanner cannot resolve the final content source cleanly

This keeps adoption attached to the code that actually chose to supply the nested content.

### First-Version Limits

- no runtime or preview instrumentation
- no guarantee of full resolution for complex indirection
- wrapper detection is limited to wrappers that preserve a resolvable call graph and parameter binding surface
- `CompositionLocal`-resolved targets may not resolve to a concrete DS component
- KSP or KAPT-generated composables are only analyzed when generated source exists on disk
- composables invoked via reflection or function references may be missed
- typealiases or import patterns that obscure the final FQN may reduce confidence or become unresolved
- no promise to analyze generated code unless source is available and parsable

These limits must be explicit in user output.

## Registry Authoring Pipeline

The design system registry should not be treated as purely hand-authored. The CLI should help create and refine it through a deterministic workflow.

### Registry Lifecycle

1. `discover`
Scan likely DS modules or packages and propose candidate components using naming, package structure, annotations, and common Compose patterns.

2. `draft`
Write a starter registry config with:
- proposed canonical components
- inferred aliases
- confidence indicators
- ambiguity flags
- suggested categories where possible

3. `validate`
Check registry entries against source usage and report:
- unknown references
- overlapping match rules
- dead entries
- ambiguous matches
- likely structural issues in grouping

4. `refine`
Allow users to edit the registry and rerun validation until stable.

### Registry Design Principles

- the registry is the canonical source of DS truth in v1
- it lives in the repo
- it is versioned and reviewable
- generated drafts are safe starting points, not hidden state
- validation must be deterministic and explainable
- v1 registry matching is FQN-plus-alias only

## AI Skill Ecosystem Boundary

AI should not be embedded into the core CLI or runtime. Instead, the system should expose stable artifacts that external AI skills can read and improve.

### Core Artifacts For Automation

- registry config files
- scan diagnostics
- unresolved symbol reports
- wrapper and drift candidate reports
- composition graph exports
- token alignment exports
- category or taxonomy files if used

### Role Of AI Skills

Optional AI skills for tools such as Claude Code, Codex, Cursor, or similar can help:
- refine registry structure
- collapse or split canonical component definitions
- suggest aliases and categories
- explain diagnostics and ambiguity
- identify repeated local compositions that indicate missing DS components
- recommend design-system improvements based on scan output

### Boundary Rule

- CLI produces reproducible facts, drafts, validations, and exports
- AI produces suggestions, explanations, and refinement proposals
- final registry state remains code/config in the repository

This preserves self-hostability, OSS usability, and deterministic operation in restricted environments.

## Product Surfaces

### CLI

The CLI is the primary first interface. It should support:
- registry discovery
- registry draft generation
- registry validation
- scans
- baseline-vs-head snapshot diff generation
- human-readable summaries
- machine-readable exports
- persistence to local or remote storage

The CLI should work well in both local and CI workflows. In CI, the headline mode is a baseline-vs-head delta artifact suitable for PR comments and status checks.

Baseline resolution for delta artifacts should support three modes in v1:
- an explicit `--baseline <snapshot-id|ref>` input for deterministic CI usage
- the latest snapshot for a configured reference such as `main`
- a repo-local default fallback for interactive local runs

Expected delta outputs:
- JSON artifact for automation
- Markdown summary for PR surfaces
- metric deltas such as adoption change, new wrapper candidates, deprecated component usage changes, and token-drift changes

### Backend/API

The backend should be a thin layer over snapshot storage and graph queries. It exists to support:
- dashboard queries
- drill-down pages
- historical comparisons
- external automation integrations

The backend should not own separate analysis semantics.

### Web UI

The web UI should begin as a report browser, not a broad platform UI.

Initial high-signal views:
- DS component inventory
- adoption by module
- local components composed from DS primitives
- wrapper candidates
- low-adoption or unused DS components
- dependency or reach views showing foundational DS components

### Default Deployment Shape

The default self-hosted architecture should stay modest:
- single-node deployment
- SQLite-backed local default with Postgres as the scale-up option
- stateless API/web layer where possible
- no SaaS-only assumptions

The scan engine should support file-hash-based incremental caching in v1. Full rescans remain available, but large monorepos should not require them for every CI run.

## Reporting Model

The first reporting model should support design system maintainers directly.

### Canonical Adoption Definition

For v1, the canonical adoption metric is coverage ratio:

`resolved DS usage sites / (resolved DS usage sites + candidate non-DS UI usage sites)`

This should be computed at minimum:
- repository level
- module level
- DS component category level

Alternative views such as unique-file counts, screen coverage, or pure usage counts can exist as secondary rollups, but the product should use coverage ratio as the default meaning of "adoption."

### Core Reports

- adoption by DS component
- adoption by module
- trend deltas across snapshots
- local compositions built from DS components
- wrapper candidates
- token alignment and hardcoded styling drift
- unresolved or ambiguous usage diagnostics
- low-adoption and unused DS components
- reach or centrality of DS components in the composition graph
- migration progress across `replaces` edges
- ownership views by team where team mapping is available

### Future-Compatible Reports

The model should leave room for future additions such as:
- cross-repo comparisons
- multi-ecosystem adoption rollups

The first version should not depend on these future reports, but the schema should not block them.

## Error Handling

The system should prefer useful partial results over all-or-nothing execution.

### Error Rules

- invalid registry config fails fast with precise diagnostics
- file-level parse failures are recorded and surfaced
- scans continue unless failure volume crosses a configurable threshold
- ambiguous matches are excluded from canonical counts until resolved
- unknown repeated symbols become review candidates
- low-confidence matches are counted separately from high-confidence matches and surfaced in reports
- plugin capability limits are explicit in summaries and exports

### Snapshot Status

Every scan should produce a snapshot with status:
- `complete`
- `partial`
- `failed`

Each scan should also produce:
- machine-readable results
- human-readable summaries
- structured diagnostics with severity
- optional baseline-vs-head delta artifacts when a comparison snapshot is provided

## Testing Strategy

Testing should be layered so the kernel and plugins can evolve independently.

### Kernel Tests

Cover:
- graph construction
- metric calculation
- wrapper and drift heuristics
- snapshot comparison
- adoption coverage computation
- token-alignment classification
- report queries

### Plugin Contract Tests

Provide a reusable conformance suite for any plugin implementation.

### Compose Fixture Tests

Use focused source fixtures covering:
- direct DS usage
- nested composition
- wrappers
- aliases
- slot-lambda composition
- parameter binding capture
- `Modifier` chain capture
- token references versus hardcoded values
- ambiguous cases
- false positive boundaries
- unsupported patterns

### CLI Integration Tests

Cover:
- registry discovery
- draft generation
- validation workflows
- scan execution
- baseline-vs-head diff generation
- export behavior

### API And UI Tests

Cover:
- query correctness against known snapshots
- rendering of key report views
- drill-down traceability from report result to underlying facts

## Traceability Principle

The kernel must be deterministic and explainable. If the system reports adoption, wrapper candidates, drift, or composition reach, users must be able to inspect the underlying facts and diagnostics that produced that result.

## Privacy And AI Export Boundary

Artifacts intended for optional AI workflows should be explicit about content. By default they may include:
- symbol names
- file paths
- module and repository metadata
- diagnostics
- normalized parameter-binding summaries
- token and hardcoded styling facts

They should not require shipping raw source code unless a user explicitly opts into a richer export mode. This keeps the core product usable in stricter environments while still enabling external skill-based refinement.

## Recommended Delivery Sequence

The design is intentionally scoped to support one implementation plan. The recommended order is:

1. define the kernel graph model and persistence contract
2. implement snapshot diffing, adoption coverage semantics, and token-aware schema support
3. implement the Compose plugin contract and source extraction path
4. implement CLI registry discovery, draft, validate, scan, and diff workflows
5. implement baseline reports and exports
6. add backend/API and report-oriented web UI
7. document the artifact interface for external AI skills and future ecosystem plugins

## Open Source And Self-Hosting Position

The product should be designed to remain:
- open source
- self-hostable
- useful without proprietary services
- configurable through repository-owned files
- extensible by outside contributors

That means:
- no hidden hosted dependency for scanning
- no required AI dependency
- no architecture that assumes one vendor, editor, or source host

## Final Design Decision

The recommended architecture is a plugin-first analysis kernel with a Compose-first extraction plugin, a graph-based tracking model, CLI-led registry authoring, report-oriented web surfaces, and an external AI skill ecosystem that augments but never defines the system of record.
