# Visual Scan Report Template Plan

> **For agentic workers:** This plan is about finalizing the UI template only. Do not implement scan aggregation, Rust report derivation, or scripting workflows from this plan.

**Goal:** Finalize the static HTML report template so the generated Wax report has the right visual hierarchy, chart language, branding, and responsive behavior before any data pipeline work begins.

**Architecture:** Treat the report as a presentation template with explicit data slots. The existing `wax-scan` skill or a future report data provider can populate those slots later; this plan protects the approved user experience.

**Tech Stack:** Static HTML, CSS, inline SVG charts, visual review in browser, responsive viewport checks

## Global Constraints

- Keep the work focused on the UI template.
- Do not design or implement data derivation scripts here.
- Keep the default report meaningful for single-language repositories.
- Use Wax branding: soft green, beeswax yellow, warm paper neutrals.
- Reserve red for true error or severity states.
- Keep diagnostics secondary to the main visual story.
- Keep the template self-contained and local.

---

## Template Shape

The HTML template is stored at:

`skills/wax-scan/templates/report.html`

The canonical template should include these sections in this order:

- `Current adoption` hero with large adoption percentage, usage count, split bar, `Adopted components`, and optional `Trend`
- `Adoption over time` as a smooth 100% split-area chart with green adopted area and beeswax-yellow boundary line
- `Adoption by project/package` as ranked horizontal bars
- `Top non-DS components to tackle` as ranked horizontal bars
- `Visible limits` and `Diagnostics` as secondary footer panels

## Data Slots

The template should expose clear replacement points for:

- repository name
- scan date
- adoption percentage
- design-system usage count
- total tracked usage count
- adopted component count
- total registry component count
- optional trend delta
- trend points
- project/package rows
- non-design-system component rows
- visible limits
- diagnostics

## Task 1: Promote The Prototype Into A Reviewable Template

**Files:**

- Modify: `skills/wax-scan/templates/report.html`
- Reference: `.superpowers/brainstorm/report-prototype.html`
- Modify: `docs/specs/2026-06-17-visual-scan-report-design.md`

- [x] Copy the approved prototype into `skills/wax-scan/templates/report.html`.
- [x] Rename any one-off prototype labels to generic template labels.
- [x] Keep the approved sections and chart language unchanged.
- [x] Add a short note in the design spec that this file is the canonical visual reference.
- [ ] Open the HTML file in a browser and confirm it renders without a dev server.

## Task 2: Template Responsiveness Pass

**Files:**

- Modify: `skills/wax-scan/templates/report.html`

- [ ] Check desktop width around `1280px`.
- [ ] Check laptop width around `1024px`.
- [ ] Check narrow width around `390px`.
- [ ] Adjust CSS so the hero, cards, bars, and labels do not overlap.
- [ ] Keep the first viewport visually strong at desktop and laptop sizes.

## Task 3: Visual Polish Pass

**Files:**

- Modify: `skills/wax-scan/templates/report.html`

- [x] Confirm the `Adopted components` tile does not duplicate the hero adoption percentage.
- [x] Confirm `Adoption by project/package` replaces the earlier `By language` direction.
- [x] Confirm the trend is a smooth 100% split-area chart, not bars.
- [x] Confirm the palette uses soft green and beeswax yellow as the primary report identity.
- [x] Confirm reds appear only in diagnostics or true severity states.

## Task 4: Final Template Review

**Files:**

- Modify: `docs/specs/2026-06-17-visual-scan-report-design.md`
- Modify: `docs/plans/2026-06-17-visual-scan-report.md`

- [ ] Update the spec with any final visual decisions from review.
- [ ] Keep this plan focused on template finalization.
- [ ] Do not add Rust implementation tasks unless a later request explicitly asks for product integration.
- [ ] Ask for final visual approval before implementation work begins.

## Task 5: Update The Wax Scan AI Skill

**Files:**

- Modify: `skills/wax-scan/SKILL.md`
- Modify: `skills/wax-scan/reference.md`
- Reference: `skills/wax-scan/templates/report.html`
- Reference: `docs/specs/2026-06-17-visual-scan-report-design.md`

- [x] Document that the skill owns analytical breakdown and data selection.
- [x] Document that the template owns visual layout and chart language.
- [x] Instruct the skill to prefer `Adoption by project/package` over `By language` unless the user explicitly asks for multi-language comparison.
- [x] Instruct the skill to preserve the approved visual narrative: hero, split-area trend, project/package bars, non-DS action bars, secondary diagnostics.
- [x] Instruct the skill not to make `wax scan` or `wax validate` depend on AI decisions.

## Verification

Visual verification is the source of truth for this plan:

```bash
open skills/wax-scan/templates/report.html
```

Expected: the report opens as a standalone HTML file and clearly shows the approved adoption hero, split-area trend, project/package bars, action bars, and secondary diagnostics.
