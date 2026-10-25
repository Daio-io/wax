import assert from 'node:assert/strict';
import {readdir, readFile} from 'node:fs/promises';
import test from 'node:test';

const website = new URL('../', import.meta.url);
const docs = new URL('docs/', website);
const build = new URL('build/docs/', website);
const buildRoot = new URL('build/', website);

const pages = [
  'getting-started/index',
  'getting-started/installation',
  'getting-started/initialize',
  'guides/registries',
  'guides/search',
  'guides/scan',
  'guides/ci',
  'guides/troubleshooting',
  'reference/configuration',
  'reference/registry',
  'reference/scan-output',
];

async function htmlFiles(directory) {
  const entries = await readdir(directory, {withFileTypes: true});
  const nested = await Promise.all(
    entries.map((entry) => {
      const url = new URL(entry.name + (entry.isDirectory() ? '/' : ''), directory);
      if (entry.isDirectory()) return htmlFiles(url);
      return entry.name.endsWith('.html') ? [url] : [];
    }),
  );
  return nested.flat();
}

test('guide sources and production routes cover the documented workflow', async () => {
  for (const page of pages) {
    const source = await readFile(new URL(`${page}.md`, docs), 'utf8');
    const route = page.endsWith('/index') ? page.slice(0, -'/index'.length) : page;
    const html = await readFile(new URL(`${route}/index.html`, build), 'utf8');
    assert.match(source, /^# /m, `${page} needs a page heading`);
    assert.match(html, /<main/, `${page} needs a server-rendered main landmark`);
  }

  const quickStart = await readFile(
    new URL('getting-started/index.md', docs),
    'utf8',
  );
  const commands = ['Install Wax', 'wax init', 'wax validate', 'wax scan'];
  const positions = commands.map((command) => quickStart.indexOf(command));
  assert.ok(positions.every((position) => position >= 0));
  assert.deepEqual(positions, [...positions].sort((left, right) => left - right));

  const installation = await readFile(
    new URL('getting-started/installation.md', docs),
    'utf8',
  );
  for (const expected of [
    'brew install wax',
    'npm install -g @waxhq/wax@alpha',
    'scripts/install.sh | bash',
    'cargo build --release -p wax-cli',
    'wax --version',
    'wax --help',
  ]) {
    assert.match(installation, new RegExp(expected.replaceAll('|', '\\|')));
  }

  const initialization = await readFile(
    new URL('getting-started/initialize.md', docs),
    'utf8',
  );
  assert.match(
    initialization,
    /wax init --non-interactive --language react --language compose/,
  );
  assert.match(initialization, /\.wax\/wax\.config\.json/);
  assert.match(initialization, /\.wax\/wax\.lock\.json/);
  assert.match(initialization, /\.wax\/(?:<[^>]+>|\w+)\.registry\.json/);
  assert.doesNotMatch(initialization, /registry scaffolds under [`']?\.wax\/registries\//);

  const configuration = await readFile(
    new URL('reference/configuration.md', docs),
    'utf8',
  );
  assert.match(configuration, /schema_version[^\n]*2/);
  assert.match(configuration, /waxrc\.schema\.json/);
  assert.match(configuration, /"registry":\s*"\.wax\/react\.registry\.json"/);
  assert.doesNotMatch(
    configuration,
    /"registry":\s*"\.wax\/registries\/react\.json"/,
  );

  assert.match(quickStart, /~\/\.wax\//);
  assert.match(quickStart, /github\.com\/Daio-io\/wax/);

  const troubleshooting = await readFile(
    new URL('guides/troubleshooting.md', docs),
    'utf8',
  );
  assert.match(troubleshooting, /github\.com\/Daio-io\/wax/);

  const registry = await readFile(
    new URL('reference/registry.md', docs),
    'utf8',
  );
  assert.match(registry, /schema_version[^\n]*1/);
  assert.match(registry, /registry\.schema\.json/);

  const ci = await readFile(new URL('guides/ci.md', docs), 'utf8');
  assert.match(ci, /wax scan --no-auto-install --strict/);

  const allHtml = await Promise.all(
    (await htmlFiles(buildRoot)).map((file) => readFile(file, 'utf8')),
  );
  assert.doesNotMatch(allHtml.join('\n'), /href=["']?\/docs\//);
});
