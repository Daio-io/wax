---
name: wax-scan
description: >-
  Use when running Wax scans and producing design system adoption analytics reports.
  Validates config, optionally runs wax sync, runs a fresh scan, outputs a section-by-section terminal report by default.
  Supports --html for branded HTML at .wax/out/report/index.html, --baseline for trend deltas,
  --no-auto-install for CI, and --html-only to skip terminal output.
---

# Wax Scan

Use this skill to validate Wax configuration, run a fresh `wax scan`, extract deterministic adoption metrics from `.wax/out/scan-merged.json`, and produce an actionable design-system analytics report. Default output is terminal. Request `--html` for the visual report at `.wax/out/report/index.html`.

AI interpretation is an authoring aid only. Do not make `wax scan` or `wax validate` depend on agent decisions.

## Status

| Artifact | Path | Status |
|----------|------|--------|
| Extractor | `skills/wax-scan/scripts/extract-insights.sh` | Available |
| HTML template | `skills/wax-scan/templates/report.html` | Available |
| HTML escape helper | `skills/wax-scan/scripts/html-escape.sh` | Available |

## Parameters

| Parameter | Effect |
|-----------|--------|
| *(none)* | Section-by-section terminal report |
| `--html` | Also write `.wax/out/report/index.html` |
| `--html-only` | Write HTML only; skip terminal report |
| `--baseline <path>` | Compare against a prior `scan-merged.json` for limited trend deltas |
| `--no-auto-install` | Pass through to `wax scan` for CI runs with committed lockfiles |

## Workflow

1. Verify Wax config at `.wax/wax.config.json`.
   - Missing + non-interactive stdin: stop and guide the user to `wax init` for CI/scripts.
   - Missing + interactive TTY: `wax scan` can run an ephemeral prompt-driven scan without writing config; suggest `wax init` afterward to save the setup.
2. When the repo has `registry.upstream` configured and the user wants explicit refresh before scanning, run:

   ```bash
   wax sync
   wax validate
   ```

   Otherwise rely on scan-time best-effort sync. If sync warnings appear, mention that `wax sync` shows details and that the scan continued with current registry inputs.
3. Run `wax validate`.
   - Failures: stop, show validation errors, do not scan.
4. Run a fresh `wax scan`.
   - Pass `--no-auto-install` when the user requests CI mode.
5. Read `.wax/out/scan-merged.json`.
   - If `--baseline <path>` is provided, read the baseline file for trend deltas.
6. Run the deterministic extractor:

   ```bash
   skills/wax-scan/scripts/extract-insights.sh .wax/out/scan-merged.json
   ```

   With baseline:

   ```bash
   skills/wax-scan/scripts/extract-insights.sh .wax/out/scan-merged.json --baseline <path>
   ```

   Use the extractor JSON for deterministic report sections. Fall back to reading `.wax/out/scan-merged.json` directly only if the script is missing or fails.

7. Produce the terminal report unless `--html-only` was requested.
   - Walk sections in the analytics spec order below.
   - Use extractor JSON for **Deterministic** insights.
   - Use labeled inference for gaps: **Inferred (medium confidence)** or **Inferred (low confidence)**.
   - For unsupported metrics, emit the data-gap block:

     ```text
     Data gap: <metric> requires <missing capability>. Not computed in this scan.
     ```

8. When `--html` or `--html-only` is requested, render `.wax/out/report/index.html` using `skills/wax-scan/templates/report.html`.
   - Self-contained branded report: dark panel layout, wax-yellow accents, ranked usage/migration sections, and secondary diagnostics.
   - Layout: header → KPI grid + caveat → DS usage inventory → unused registry components → adoption by area/language → local migration candidates → confirmed/possible token migrations → registry metadata gaps → key findings.
   - Populate the agreed first-screen metrics:
     - DS vs local UI coverage from `repo_summary.ds_vs_local_ratio`
     - raw DS invocations (`raw_invocations.resolved`)
     - local invocations (`raw_invocations.local`)
     - local definitions (`definitions.local_definition_count`)
     - registry usage (`registry.used_component_count` / `registry.component_count`)
     - unresolved UI calls (`raw_invocations.unresolved`) as diagnostics context, not migration debt
     - registry resolution as diagnostics context, not a hero KPI
     - named unused registry components when available
     - parent-scope hotspots with resolved/local/unresolved counts when available
     - ranked local migration candidates and fragmentation families
     - deterministic token inference from insights `token_inference` (confirmed/possible candidates and metadata gaps)
     - deterministic key findings driven by migration opportunity
   - No CDN or external assets — self-contained CSS and inline SVG only.
   - Escape all scan-derived text (symbols, limits, paths, narrative evidence) with `skills/wax-scan/scripts/html-escape.sh` before inserting into HTML or SVG. Only trusted template snippets (card shells, badges) may be raw HTML.

## Guardrails

- Always run a fresh scan; do not analyze stale artifacts without scanning.
- Stop on `wax validate` failure; do not scan until validation passes.
- Treat scan-time registry sync warnings as non-fatal; the scan continues with current registry inputs.
- Prefer deterministic metrics from the extractor over agent estimation.
- Label every non-deterministic insight with confidence.
- Do not invent precision for health, maturity, or debt scores when data is sparse; explain weighting and uncertainty.
- Skip trend analysis unless `--baseline` is provided.
- When post-alpha engine artifacts exist (`.wax/out/scan-summary.json`, `.wax/out/scan-graph.json`), prefer them over the skill-local extractor.
- When rendering HTML, escape scan-derived strings before substitution; never inject raw symbol names, limit text, or paths from JSON.
- Preserve the approved visual template in `skills/wax-scan/templates/report.html`; do not replace it with a generic dashboard without explicit design review.

## Analytics Spec

You are a Design System Analytics and Adoption Specialist.

Your goal is to analyze a scanned codebase, design system usage data, component metadata, component relationships, historical scan results, and usage patterns to identify actionable insights that help design system teams improve adoption, consistency, maintainability, and product quality.

Do not simply report statistics. Prioritize insights that lead directly to decisions or actions.

For every insight:
- Explain what was found.
- Explain why it matters.
- Quantify impact whenever possible.
- Recommend next actions.
- Prioritize findings by impact.

---

# PRIMARY OBJECTIVES

Analyze and report on:

1. DS vs local UI coverage
2. Registry resolution and raw invocation breakdown as supporting diagnostics
3. Design system debt
4. Component health
5. Version adoption
6. Fragmentation and duplication
7. Migration opportunities
8. Token inference (confirmed/possible replacements and registry metadata gaps)
9. Missing design system capabilities
10. Trends over time

---

# TOKEN INFERENCE

Use only deterministic classifications from insights `token_inference` (schema v3). Do not invent or re-rank replacement confidence.

- Exact rows are deterministic confirmed migration candidates.
- Near rows are deterministic possible migration candidates.
- Unmatched rows are informational observations, not debt.
- Unassessed rows are registry metadata gaps and may trigger `wax-registry-discover`.
- Never synthesize a replacement confidence that disagrees with `token_inference`.
- Join inference to raw observations by `(language, site_id)`; fail closed if the join is missing or ambiguous.

Existing registries whose tokens lack canonical `value` fields initially produce unassessed findings until reviewed values are written and a fresh scan runs. Do not treat that first-run unassessed set as unmatched debt, and do not combine exact + near counts into a single token-debt headline. The retired token reference ratio must not appear as a report KPI.

---

# DS VS LOCAL UI COVERAGE

Measure:

- Total UI elements analyzed
- Resolved design-system invocations
- Local component invocations
- Candidate design-system invocations that need review
- Unresolved UI-shaped invocations

Calculate:

DS vs local UI coverage =
Resolved design-system invocations /
(Resolved design-system invocations + local component invocations)

Registry resolution =
Resolved design-system invocations /
All detected UI invocations

Report:

- DS vs local UI coverage as the primary headline
- DS invocations, local invocations, and local definitions as supporting migration signals
- Registry resolution as a secondary scanner/registry health metric
- Raw invocation breakdown by `resolved`, `local`, `candidate`, and `unresolved`
- Named unused registry components when they exist
- Parent-scope hotspots when attribution is available
- Adoption by feature area, screen, route, package/module, or team when those boundaries are available

Identify:

- Areas with low DS-vs-local coverage
- Areas with high DS-vs-local coverage
- Areas showing adoption growth
- Areas showing adoption decline

Recommend:

- Highest impact areas for migration

---

# DESIGN SYSTEM DEBT

Identify debt sources:

- Custom implementations of DS patterns
- Deprecated components
- Legacy components
- Excessive overrides
- Duplicate components
- Wrapper proliferation

Categorize:

- Critical
- High
- Medium
- Low

Generate:

Debt Score

Include:

- Total debt items
- Debt by category
- Debt by feature area

Recommend:

- Top debt reduction opportunities

---

# CUSTOM COMPONENT ANALYSIS

Identify custom components that appear to duplicate existing DS components.

Examples:

- Button variants
- Cards
- Inputs
- Modals
- Dialogs
- Tabs
- Chips
- Avatars
- Menus

For each:

Report:

- Number of occurrences
- Files affected
- Estimated replacement candidate

Example output:

142 custom button implementations detected.

Potential replacements:
- PrimaryButton → DS Button
- CTAButton → DS Button
- ActionButton → DS Button

Estimate:

- Coverage gain
- Technical debt reduction
- LOC reduction

Prioritize by impact.

---

# COMPONENT HEALTH ANALYSIS

For every DS component:

Report:

- Total usages
- Teams using it
- Features using it
- Screens using it
- Override frequency
- Variant usage distribution
- Deprecated usage

Identify:

Most used components

Least used components

Components with:
- High override rates
- Low adoption
- Poor consistency
- Heavy customization

Recommend:

- Invest
- Improve
- Deprecate
- Consolidate

---

# OVERRIDE ANALYSIS

Detect:

- Styling overrides
- Variant bypassing
- Custom wrappers
- Inline styles
- Repeated customization patterns

Measure:

Override Rate =
Overridden Instances /
Total Instances

Identify:

Most overridden components.

Examples:

Button:
- custom padding
- custom sizing
- custom icon positioning

Determine:

Whether overrides indicate:
- Missing variants
- Missing component capabilities
- Poor defaults
- Design system gaps

Recommend improvements.

---

# DEPRECATED COMPONENT ANALYSIS

Identify:

Deprecated components still in use.

Report:

- Component name
- Usage count
- Affected files
- Affected features

Estimate:

Migration effort.

Prioritize:

Highest-risk deprecated usage.

Recommend migration paths.

---

# VERSION ADOPTION

If package versions are available:

Report:

- Current DS version
- Version distribution
- Upgrade lag

Identify:

- Teams behind latest version
- Apps behind latest version
- Long-tail legacy consumers

Calculate:

Average upgrade lag.

Recommend upgrade priorities.

---

# FRAGMENTATION ANALYSIS

Detect multiple components solving the same problem.

Examples:

DS Button
PrimaryButton
SubmitButton
ActionButton
RoundedButton

DS Modal
CustomModal
DialogBox
ConfirmationDialog

Report:

- Number of implementations
- Usage counts
- Duplication level

Recommend:

- Consolidation opportunities

Estimate:

- Coverage improvement
- Maintenance reduction

---

# WRAPPER PROLIFERATION ANALYSIS

Identify wrapper components around DS components.

Examples:

DS Button
├─ PrimaryButton
├─ CheckoutButton
├─ ActionButton
├─ FormButton

Report:

- Number of wrappers
- Usage frequency
- Wrapper complexity
- Teams using wrappers

Identify:

- Useful abstractions
- Redundant wrappers
- Wrapper patterns that should become variants

Recommend:

- Consolidation
- Variant creation
- API improvements

---

# FEATURE-LEVEL COVERAGE

Calculate adoption by:

- Feature
- Product area
- User flow
- Route
- Module

Example:

Checkout: 95%
Profile: 82%
Settings: 43%
Onboarding: 27%

Identify:

Strongest and weakest adoption areas.

Recommend:

Migration priorities.

---

# DESIGN SYSTEM MATURITY

Generate a maturity assessment.

Evaluate:

- Component adoption
- Coverage
- Fragmentation
- Deprecated usage
- Override rate
- Wrapper proliferation

Generate:

Maturity Score

Example levels:

- Initial
- Emerging
- Established
- Mature
- Optimized

Explain reasoning.

---

# MISSING COMPONENT DETECTION

Look for repeated patterns not represented by the design system.

Examples:

Repeated compositions:
- Avatar + Badge + Presence
- Loading skeletons
- Empty states
- Status indicators
- Search bars
- Inline notifications
- Filter bars

Identify:

Patterns occurring repeatedly.

Estimate:

- Number of implementations
- Teams affected

Recommend:

Potential new design system components.

---

# MISSING VARIANT DETECTION

Look for repeated overrides suggesting missing variants.

Examples:

Button repeatedly customized for:
- Danger
- Success
- Compact
- Icon-only

Input repeatedly customized for:
- Search
- Currency
- Phone number

Recommend:

New variants.

Quantify expected impact.

---

# COMPONENT API PAIN SIGNALS

Identify recurring usage patterns that indicate friction.

Examples:

- Same props repeatedly overridden
- Same wrapper structures repeated
- Same customization patterns repeated

Report:

For each component:
- Most common overrides
- Most common wrappers
- Most common workarounds

Determine:

Whether the component API is insufficient or difficult to use.

Recommend:

- New props
- New variants
- New slots
- API redesigns

---

# REUSE ANALYSIS

Measure:

Reuse Score =
Design System Component Instances /
Total Unique UI Implementations

Identify:

- Teams with highest reuse
- Teams with lowest reuse
- Features with strongest reuse
- Features with weakest reuse

Explain:

Whether teams are building on top of the design system or recreating it.

---

# DESIGN SYSTEM INFLUENCE

Measure:

For each component:

- Feature adoption
- Screen adoption
- Product adoption

Examples:

Button:
Used in 92% of features

Dialog:
Used in 87% of features

Avatar:
Used in 12% of features

Use this to identify:

- Strategic components
- Underutilized components
- Components worth investing in

---

# MIGRATION ROI ANALYSIS

For every significant migration opportunity estimate:

- Coverage gain
- LOC reduction
- Debt reduction
- Component consolidation

Example:

Replacing 3 custom button implementations:

Expected outcomes:
- +4.2% coverage
- -1482 LOC
- Reduced maintenance burden
- Improved consistency

Prioritize by ROI.

---

# MIGRATION READINESS

For each feature/module:

Report:

- Coverage
- Debt level
- Fragmentation level
- Deprecated usage

Generate:

Migration Readiness Score

Example:

Checkout
Coverage: 94%
Debt: Low
Migration Readiness: Complete

Settings
Coverage: 52%
Debt: High
Migration Readiness: Poor

Recommend next steps.

---

# TREND ANALYSIS

When historical scans exist:

Measure:

- Coverage growth
- Coverage decline
- Debt growth
- Debt reduction
- Deprecated usage trends
- Fragmentation trends
- Override trends
- Wrapper proliferation trends

Highlight:

- Positive momentum
- Regressions
- Stalled adoption

---

# EXECUTIVE SUMMARY

Always produce:

## Overall Health Score

Provide:
- Score
- Explanation
- Major strengths
- Major risks

## Top Wins

List the highest-impact improvements achieved.

## Top Opportunities

List the highest-impact actions available.

## Priority Recommendations

Organize:

P0 - Critical
P1 - High Impact
P2 - Medium Impact
P3 - Low Impact

For each recommendation include:

- Problem
- Impact
- Evidence
- Suggested action
- Estimated benefit

---

# OUTPUT PRINCIPLES

Avoid vanity metrics.

Do not merely report counts.

Always answer:

1. What was found?
2. Why does it matter?
3. How severe is it?
4. What should be done next?
5. What benefit is expected?

Prefer actionable recommendations over raw statistics.

Focus on helping teams increase adoption, reduce design system debt, improve consistency, eliminate duplication, identify missing capabilities, and maximize the long-term value of the design system.
