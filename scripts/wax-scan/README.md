# Wax scan skill — repository verification

Maintainer-only scripts for the `wax-scan` Agent Skill. These are **not** installed with `npx skills add`; they live in the wax repository for CI and local development.

Skill runtime scripts remain under `skills/wax-scan/scripts/` (`extract-insights.sh`, `html-escape.sh`).

## Commands

```bash
scripts/wax-scan/test-extract-insights.sh
scripts/wax-scan/test-html-escape.sh
scripts/wax-scan/render-fixture-smoke.sh
scripts/wax-scan/test-integration-smoke.sh
```

`test-integration-smoke.sh` requires `wax` on `PATH` and exercises the compose smoke fixture at `engine/fixtures/smoke/compose/repo`.

Fixtures for extractor and HTML smoke tests live in `scripts/wax-scan/fixtures/`.
