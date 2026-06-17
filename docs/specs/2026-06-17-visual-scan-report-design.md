# Visual Scan Report Design

## Context

Wax already has the scan facts needed for reporting, but the current user-facing output is still sparse:

- `wax scan` prints a minimal terminal summary in `engine/crates/wax-cli/src/commands/scan.rs`
- the deferred post-alpha UX plan already reserves space for static HTML reporting in `docs/plans/2026-05-24-post-alpha-ux-plan.md`

The next reporting surface should feel like a real product artifact, not a debug dump. The primary concern is design-system maintainer usability: the first screen must be visually strong, immediately legible, and useful on the very first scan.

During brainstorming, we compared the current Wax direction with Omlet's reporting style and converged on a tighter narrative rather than a many-widget dashboard. We also iterated on a real HTML prototype and promoted it into the canonical skill template at:

`skills/wax-scan/templates/report.html`

That template is the visual anchor for this design.

## Goals

- Produce a static local HTML report template that feels rich, intentional, and visually clear out of the box.
- Optimize the default report for design-system maintainers, not for executives or generic analytics browsing.
- Make the first screen useful on a single scan with no history.
- Lead with adoption, then show trend, then show action priority.
- Leave analytical breakdown and data selection to the Wax scan-report AI skill or report data provider.
- Make the visual system branding-aware now and configurable later.

## Non-Goals

- Do not build a hosted dashboard or multi-page analytics application.
- Do not require multi-language repositories for the default layout to make sense.
- Do not make trend charts depend on history to the point that the report feels broken on first run.
- Do not add speculative ownership/team charts in the first version.
- Do not change scan-facts contracts just to support decorative visuals.
- Do not prescribe Rust derivation scripts or report aggregation internals in this design.

## Primary Audience

The default report is for design-system maintainers.

They need to answer three questions quickly:

1. How much design-system adoption do we have right now?
2. Is adoption improving over time?
3. What should we tackle next to increase adoption?

## Visual Narrative

The default report should tell one story in order:

1. `Current adoption`
2. `Adoption over time`
3. `Where adoption is uneven`
4. `Top non-design-system migration opportunities`

This is intentionally not a dashboard of unrelated charts. It is a narrative page with supporting details below the fold.

## Page Structure

### Hero section

The hero is a bold adoption snapshot with:

- a large adoption percentage headline
- supporting usage counts
- a prominent split snapshot of `design system` vs `non-design system`
- two supporting metric tiles that do not duplicate the hero headline

The current approved tile set is:

- `Adopted components` — for example `84 / 126`
- `Trend` — for example `+8 pts` when a baseline exists

Copy should stay positive and product-like. Avoid copy such as `Gap to close` in the hero.

### Chart 1: Adoption over time

This chart should not be bars.

The approved chart is a smooth `100% split area chart`:

- the chart is normalized to 100% at every time point
- the lower area represents design-system share
- the upper area represents non-design-system share
- a beeswax-yellow boundary line defines the split over time

This chart must show the `vs` relationship directly, not just isolated values.

If historical baselines are missing, the report should render a deliberate first-scan state instead of an empty or broken chart.

### Chart 2: Adoption by project/package

The secondary breakdown chart should not be `by language`, because that only works well for a smaller subset of repositories.

The approved replacement is `Adoption by project/package` using ranked horizontal bars.

This chart should show where adoption is strongest and weakest across the repo in a way that still makes sense for single-language repositories.

### Chart 3: Top non-design-system components to tackle

This is the maintainer action chart.

It should be a ranked horizontal bar chart showing the most repeated non-design-system components or equivalent migration candidates. It should read as the queue of the biggest adoption opportunities, not as a generic component list.

## Visual System

The report should follow Wax branding rather than default analytics colors.

### Approved palette direction

- beeswax yellow is the primary accent and highlight
- adoption charts should lean on soft greens plus beeswax yellow
- warm reds should be reserved for genuine error or severity signals, not used as the default report language
- supporting neutrals should feel soft, warm, and paper-like rather than stark SaaS gray

### Brand configurability

The first version should read colors from a Wax-owned theme/config layer where practical so that:

- Wax can ship a branded default now
- report themes can become user-configurable later

The first version does not need to expose full end-user theme configuration yet, but its implementation should avoid hard-coding the palette in a way that blocks that future.

## Data Model Expectations

This design only defines the template and the data slots it needs.

The Wax scan-report AI skill or another report data provider can decide how to derive the breakdowns. The template should accept presentation-ready values for:

- current adoption percentage and usage counts
- adopted component count
- optional trend delta
- 100% split-area trend points
- project/package adoption rows
- ranked non-design-system component opportunities
- visible limits and diagnostics

The report should assume useful facts already exist from scans. The immediate priority is making the UI template right, not designing the full aggregation pipeline.

## First-Scan Behavior

The report must feel worthwhile even with no prior history.

That means:

- the hero must stand on its own
- the project/package breakdown must stand on its own
- the migration opportunity chart must stand on its own
- the trend area should gracefully degrade to a first-scan state when no baseline/history exists

The first scan should never make the page feel empty.

## Error Handling And Confidence

The report should never look broken because scan coverage is partial.

- if a baseline is missing, render a first-scan state for trend
- if some language or project data is partial, still render available visuals and flag confidence clearly
- if registry coverage is sparse, surface it as a visible limit near the relevant visuals
- diagnostics remain secondary and should not dominate the visual hierarchy

## Layout Principles

- Keep the opening screen to a small number of large visual blocks
- Prefer strong hierarchy over many widgets
- Favor horizontal bars and one strong area chart over a grab-bag of chart types
- Preserve usefulness on laptop and smaller desktop widths
- Keep the report fully static and local with no network dependency

## Implementation Notes

- Produce a canonical static HTML template first
- Keep the report self-contained or asset-local so it opens offline
- Keep chart and section slots explicit so `skills/wax-scan/SKILL.md` can populate them later
- Avoid baking data-derivation assumptions into the visual template

## Testing

Testing for this phase is visual and template-focused.

- hero contains the large adoption headline and split snapshot
- trend section renders the split-area container and first-scan fallback copy/state
- project/package breakdown renders ranked horizontal bars
- migration opportunity chart renders ranked horizontal bars
- diagnostics and limits remain present but secondary
- desktop and narrow viewports preserve hierarchy without text overlap

## Constraints

- Keep the report static and local
- Keep the report useful on the first scan
- Keep the default layout meaningful for single-language repositories
- Keep branding defaulted now and configurable later
- Keep diagnostics secondary to the visual story
- Keep contract changes synchronized with schemas, fixtures, docs, and tests when needed

## Verification

Prototype/design verification for this spec is visual:

- review the HTML template at `skills/wax-scan/templates/report.html`
- confirm the report tells the approved narrative:
  - hero adoption snapshot
  - split-area trend
  - project/package adoption breakdown
  - ranked migration opportunities
