# Example: Component Alias/Tag Enrichment

Golden workflow for proposing and applying search-oriented component metadata after deterministic discovery. Companion to [../SKILL.md](../SKILL.md).

## Setup

- Deterministic `wax registry discover --dry-run` found no component identity changes.
- The publisher registry already contains `ds.navigation-bar`, including an authored alias and tag that must survive rediscovery.
- The component symbol, id, package path, and documentation in `details` all support common navigation vocabulary.

## Before registry

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.navigation-bar",
      "symbol": "NavigationBar",
      "aliases": ["site navigation"],
      "tags": ["layout"],
      "package": "com.example.ds.navigation",
      "docs": "Top-level application navigation displayed across the top of a screen."
    }
  ],
  "tokens": []
}
```

## Proposed diff

The deterministic fields remain unchanged. The following group is review-only and has not been written:

```text
Component changes: (none)
Aliases/tags proposed:
  - ds.navigation-bar
    aliases: add ["navbar", "nav bar", "topbar", "app bar"]
    tags: add ["navigation"]
    rationale: NavigationBar identity and docs describe top-level application navigation
Token additions: (none)
Values filled: (none)
Values changed: (none)
Potential removals: (none)
```

These aliases are terms a user may search for, not claims that `navbar`, `topbar`, or `app bar` are code symbols. The tag is a coarse grouping, not scan authority.

## Explicit approval

Stop and wait for explicit approval of the **Aliases/tags proposed** group. Discovery must not write the suggestions automatically.

After approval, patch only the approved additions into the publisher registry. Preserve the authored `site navigation` alias and `layout` tag. Approval to add metadata does not authorize removing or replacing existing metadata.

## After registry

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.navigation-bar",
      "symbol": "NavigationBar",
      "aliases": ["site navigation", "navbar", "nav bar", "topbar", "app bar"],
      "tags": ["layout", "navigation"],
      "package": "com.example.ds.navigation",
      "docs": "Top-level application navigation displayed across the top of a screen."
    }
  ],
  "tokens": []
}
```

Only the approved aliases and tag were appended. The component id, symbol, package, docs, existing alias, existing tag, and token list are unchanged.

## Synonym search

After the approved publisher registry is available to the consumer, the synonym finds the canonical component:

```text
$ wax search "navbar"
components:
  - ds.navigation-bar (NavigationBar)
    matched: alias "navbar"
```

Search returns the canonical `ds.navigation-bar` identity. Scans continue to resolve the real `NavigationBar` symbol according to the language pack and registry package.
