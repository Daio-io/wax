---
title: Agent skills
sidebar_position: 1
---

# Choose a workflow

Wax ships three task-focused agent skills. Install all of them from the
repository, or review the source first:

```bash
npx skills add https://github.com/Daio-io/wax
```

| Task | Skill | Boundary |
| --- | --- | --- |
| Select a component or token for implementation | [`wax-search`](./wax-search.md) | Read snapshot-pinned registry facts; do not run analytics or mutate sources. |
| Produce fresh adoption analytics | [`wax-scan`](./wax-scan.md) | Validate and scan afresh; report deterministic evidence without registry writes. |
| Maintain a publisher registry | [`wax-registry-discover`](./wax-registry-discover.md) | Preview and review lossless changes; write only with explicit approval. |

The workflows complement one another: registry maintainers publish reviewed
facts, search consumes an exact snapshot for implementation, and scan measures
observed application usage. A search result is not scan evidence, and scan
inference is not permission to edit a registry.

Canonical sources:

- [`skills/wax-search/SKILL.md`](https://github.com/Daio-io/wax/blob/main/skills/wax-search/SKILL.md)
- [`skills/wax-scan/SKILL.md`](https://github.com/Daio-io/wax/blob/main/skills/wax-scan/SKILL.md)
- [`skills/wax-registry-discover/SKILL.md`](https://github.com/Daio-io/wax/blob/main/skills/wax-registry-discover/SKILL.md)

Wax also publishes optional
[`marketplace.json`](https://github.com/Daio-io/wax/blob/main/.claude-plugin/marketplace.json)
and [`plugin.json`](https://github.com/Daio-io/wax/blob/main/.claude-plugin/plugin.json)
metadata. These describe the bundle; they do not change the portable install
command above or imply additional agent-specific commands.
