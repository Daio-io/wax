import assert from 'node:assert/strict';
import {access, readFile} from 'node:fs/promises';
import test from 'node:test';

const build = new URL('../build/', import.meta.url);

test('build emits the GitHub project-page routes', async () => {
  const home = await readFile(new URL('index.html', build), 'utf8');
  const start = await readFile(
    new URL('docs/getting-started/index.html', build),
    'utf8',
  );

  // Docusaurus HTML minification may omit quotes around attribute values.
  assert.match(home, /href=["']?\/wax\/docs\/getting-started\/["']?/);
  assert.match(home, /https:\/\/github\.com\/Daio-io\/wax/);
  assert.match(start, /npm install -g @waxhq\/wax@alpha/);
  assert.doesNotMatch(
    `${home}\n${start}`,
    /href=["']?\/docs\/|src=["']?\/assets\//,
  );

  await access(new URL('404.html', build));
  await access(new URL('img/wax-full-logo.png', build));
});
