import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';

test('package.json omits type module so Docusaurus webpack can transform require.resolveWeak', async () => {
  const pkg = JSON.parse(
    await readFile(new URL('../package.json', import.meta.url), 'utf8'),
  );

  assert.notEqual(
    pkg.type,
    'module',
    'Docusaurus webpack expects ambiguous module mode; "type": "module" leaves require.resolveWeak untransformed and breaks SSG',
  );
});
