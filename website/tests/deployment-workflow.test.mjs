import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';

const escapeRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

test('Pages workflow verifies PRs and deploys main via artifacts', async () => {
  const workflow = await readFile(
    new URL('../../.github/workflows/docs.yml', import.meta.url),
    'utf8',
  );

  for (const required of [
    'pull_request:',
    'branches: [main]',
    'npm test',
    'pages: read',
    'actions/configure-pages@v5',
    'actions/upload-pages-artifact@v4',
    'path: website/build',
    'needs: build',
    'pages: write',
    'id-token: write',
    'actions/deploy-pages@v4',
  ]) {
    assert.match(workflow, new RegExp(escapeRegExp(required)));
  }

  assert.doesNotMatch(workflow, /push origin HEAD:gh-pages|docusaurus deploy/);
});
