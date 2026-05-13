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
- package or symbol match rules
- category
- status
- optional metadata such as deprecation, replacement, ownership, or tags

#### LocalComponent

A project-defined composable that is not itself part of the design system but is built from one or more design system components or other local components.

Examples:
- feature-specific wrappers
- branded variants
- app-level convenience composables
- repeated local patterns that may indicate gaps in the design system

This must be a first-class entity, not a tag on a usage record, because composition tracking depends on traversable component-to-component relationships.

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
- confidence or match status

#### Module

A reporting boundary representing a Gradle module or configured project segment.

#### ScanSnapshot

A versioned record of one scan run used for current-state and historical reporting.

Suggested fields:
- snapshot id
- scan timestamp
- repo revision when available
- plugin versions
- status
- diagnostic summary

### Core Relationships

The kernel should support at least these normalized relationships:
- `uses`
- `composes`
- `wraps`
- `declared_in`
- `depends_on`

Interpretation:
- `uses`: a local component or usage site invokes a DS component
- `composes`: a local component is built from one or more DS or local components
- `wraps`: a local component primarily forwards to a DS component with limited customization
- `declared_in`: an entity belongs to a module, file, or symbol container
- `depends_on`: rollup relationship useful for module-level analysis

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
- DS component references
- local component declarations
- component-to-component composition links
- usage sites
- unresolved or ambiguous matches

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
- identify invocation sites
- match invocations to registry entries
- detect local components built from DS primitives
- emit normalized graph facts

### Compose Scan Flow

1. Load and validate the DS registry.
2. Discover configured Kotlin source files and modules.
3. Parse source files with a Kotlin source parser.
4. Identify composable declarations and call sites.
5. Match call targets against the canonical DS registry.
6. Build local component composition records for non-DS composables that use DS components.
7. Emit entities, edges, and diagnostics to the kernel.
8. Persist the snapshot and compute rollups in the kernel.

### First-Version Limits

- no runtime or preview instrumentation
- no guarantee of full resolution for complex indirection
- best-effort handling for aliases and straightforward wrappers only
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

## AI Skill Ecosystem Boundary

AI should not be embedded into the core CLI or runtime. Instead, the system should expose stable artifacts that external AI skills can read and improve.

### Core Artifacts For Automation

- registry config files
- scan diagnostics
- unresolved symbol reports
- wrapper and drift candidate reports
- composition graph exports
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
- human-readable summaries
- machine-readable exports
- persistence to local or remote storage

The CLI should work well in both local and CI workflows.

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
- local filesystem or Postgres-backed persistence
- stateless API/web layer where possible
- no SaaS-only assumptions

## Reporting Model

The first reporting model should support design system maintainers directly.

### Core Reports

- adoption by DS component
- adoption by module
- local compositions built from DS components
- wrapper candidates
- unresolved or ambiguous usage diagnostics
- low-adoption and unused DS components
- reach or centrality of DS components in the composition graph

### Future-Compatible Reports

The model should leave room for future additions such as:
- trend analysis across snapshots
- cross-repo comparisons
- design token alignment
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

## Testing Strategy

Testing should be layered so the kernel and plugins can evolve independently.

### Kernel Tests

Cover:
- graph construction
- metric calculation
- wrapper and drift heuristics
- snapshot comparison
- report queries

### Plugin Contract Tests

Provide a reusable conformance suite for any plugin implementation.

### Compose Fixture Tests

Use focused source fixtures covering:
- direct DS usage
- nested composition
- wrappers
- aliases
- ambiguous cases
- false positive boundaries
- unsupported patterns

### CLI Integration Tests

Cover:
- registry discovery
- draft generation
- validation workflows
- scan execution
- export behavior

### API And UI Tests

Cover:
- query correctness against known snapshots
- rendering of key report views
- drill-down traceability from report result to underlying facts

## Traceability Principle

The kernel must be deterministic and explainable. If the system reports adoption, wrapper candidates, drift, or composition reach, users must be able to inspect the underlying facts and diagnostics that produced that result.

## Recommended Delivery Sequence

The design is intentionally scoped to support one implementation plan. The recommended order is:

1. define the kernel graph model and persistence contract
2. implement the Compose plugin contract and source extraction path
3. implement CLI registry discovery, draft, validate, and scan workflows
4. implement baseline reports and exports
5. add backend/API and report-oriented web UI
6. document the artifact interface for external AI skills and future ecosystem plugins

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
