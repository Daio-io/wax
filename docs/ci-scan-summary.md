# Wax scan summaries in GitHub Actions

Commit `.wax/wax.config.json`, `.wax/wax.lock.json`, and repository-local registry files. CI must install the exact language-pack versions in the lockfile before running a scan with automatic installation disabled.

The following job runs on `ubuntu-latest`, validates repository inputs, and writes JSON, graph, Markdown, and offline HTML reports:

```yaml
jobs:
  wax-scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Wax
        run: curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash

      - name: Install locked language packs
        shell: bash
        run: |
          jq -r '.languages | to_entries[] | "\(.key)@\(.value.version)"' .wax/wax.lock.json |
            while IFS= read -r pack; do
              wax language install "$pack"
            done

      - name: Validate and scan
        shell: bash
        run: |
          wax validate
          wax scan \
            --no-auto-install \
            --strict \
            --format quiet \
            --output json-summary=.wax/out/scan-summary.json \
            --output markdown=.wax/out/scan-summary.md \
            --output graph-data=.wax/out/scan-graph.json \
            --output html=.wax/out/report/index.html

      - name: Upload Wax reports
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: wax-scan
          if-no-files-found: warn
          path: |
            .wax/out/scan-summary.json
            .wax/out/scan-summary.md
            .wax/out/scan-graph.json
            .wax/out/report/
```

## Compare with a baseline

Download or restore a prior `scan-summary.json` (or `scan-merged.json`) before the scan, then pass its path with `--baseline`. JSON-summary and Markdown outputs will include metric and diagnostic deltas. Quiet remains suitable for stdout because requested summary artifacts still trigger delta computation.

```yaml
      - name: Scan with main-branch baseline
        run: |
          wax scan \
            --no-auto-install \
            --strict \
            --format quiet \
            --baseline .wax/baselines/main-scan-summary.json \
            --output json-summary=.wax/out/scan-summary.json \
            --output markdown=.wax/out/scan-summary.md \
            --output graph-data=.wax/out/scan-graph.json \
            --output html=.wax/out/report/index.html
```

Without `--baseline`, the JSON `deltas` field and Markdown `## Changes` section are omitted. A missing or unsupported baseline is an error when a JSON-summary or Markdown output needs deltas.

## Optional pull-request comment

For a workflow triggered by `pull_request`, grant `pull-requests: write` and post the generated Markdown file. Keep this step optional for forked pull requests or other contexts where the token cannot write comments.

```yaml
permissions:
  contents: read
  pull-requests: write

# Add after the scan step.
- name: Comment with Wax summary
  if: github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository
  env:
    GH_TOKEN: ${{ github.token }}
    PR_NUMBER: ${{ github.event.pull_request.number }}
  run: gh pr comment "$PR_NUMBER" --body-file .wax/out/scan-summary.md
```

This documentation is a recipe only; Wax does not install a mandatory workflow in consuming repositories.
