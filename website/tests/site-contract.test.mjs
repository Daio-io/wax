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
  const cli = await readFile(
    new URL('docs/reference/cli/index.html', build),
    'utf8',
  );
  const skills = await readFile(
    new URL('docs/skills/index.html', build),
    'utf8',
  );

  // Docusaurus HTML minification may omit quotes around attribute values.
  assert.match(home, /href=["']?\/wax\/docs\/getting-started\/["']?/);
  assert.match(home, /href=["']?\/wax\/docs\/reference\/cli\/["']?/);
  assert.match(home, /href=["']?\/wax\/docs\/skills\/["']?/);
  assert.match(home, /href=["']?\/wax\/docs\/guides\/search\/["']?/);
  assert.match(home, /href=["']?\/wax\/docs\/guides\/scan\/["']?/);
  assert.match(home, /Find the right component/);
  assert.match(home, /Measure adoption/);
  assert.match(home, /Work with agents/);
  assert.match(home, /https:\/\/github\.com\/Daio-io\/wax/);
  assert.match(start, /Install Wax/);
  assert.match(start, /wax init/);
  assert.match(start, /wax validate/);
  assert.match(start, /wax scan/);
  assert.match(home, /<main/);
  assert.match(home, /<section[^>]+aria-labelledby=/);
  assert.match(cli, /CLI reference/);
  assert.match(skills, /Choose a workflow/);
  assert.doesNotMatch(
    `${home}\n${start}\n${cli}\n${skills}`,
    /href=["']?\/docs\/|src=["']?\/assets\//,
  );

  await access(new URL('404.html', build));
  await access(new URL('img/wax-full-logo.png', build));
});
